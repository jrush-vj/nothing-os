// The runner for nothing-bar's pop-up widgets (`nothing-bar notes|clip|player|wall`). Each is its own short-lived
// process named nothing-<mode>, so `pkill -x nothing-bar` (bar restarts, turbo, hide) never touches it. Running the
// same mode again closes the open one (a toggle for keybinds).
//
// A widget gets a card on the overlay layer with keyboard focus. The card opens like the control centre: a short
// drop, a dot-matrix screen sweeping away and labels decoding; it closes with a quick fade and the process exits.
// A widget can also ask to become full-screen (the wallpaper transition).
//
// Debugging: NOTHING_PREVIEW=out.png renders one frame to a PNG instead (no Wayland). In debug builds only,
// NOTHING_KEYS="text|<enter>|<down>…" types keys at start (no input tools on the box).
use crate::anim::Tween;
use crate::bar::{GREY, WHITE};
use crate::canvas::{Canvas, Rgba};
use crate::config::Config;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::os::fd::AsRawFd;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

/// Transparent room around the card for its shadow, logical px.
const MARGIN: f32 = 18.0;
pub const RADIUS: f32 = 22.0;
pub const CARD: Rgba = Rgba::hex(0x0d0d0d);
pub const DIM: Rgba = Rgba::hex(0x555555);

#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Text(String),
    /// Ctrl + a letter (lowercase).
    Ctrl(char),
    Back,
    /// Ctrl + Backspace.
    BackWord,
    Delete,
    Enter,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Tab,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub caps: bool,
}

/// Translates a key press; None for keys the widgets don't use (modifiers alone, function keys).
pub fn key_of(ev: &KeyEvent, m: Mods) -> Option<Key> {
    let k = match ev.keysym {
        Keysym::BackSpace if m.ctrl => Key::BackWord,
        Keysym::BackSpace => Key::Back,
        Keysym::Delete | Keysym::KP_Delete => Key::Delete,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::Escape => Key::Esc,
        Keysym::Up => Key::Up,
        Keysym::Down => Key::Down,
        Keysym::Left => Key::Left,
        Keysym::Right => Key::Right,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::Tab | Keysym::ISO_Left_Tab => Key::Tab,
        Keysym::Page_Up => Key::PageUp,
        Keysym::Page_Down => Key::PageDown,
        s if m.ctrl => Key::Ctrl(s.key_char()?.to_ascii_lowercase()),
        _ => {
            let t = ev.utf8.clone().filter(|t| !t.is_empty() && !t.chars().any(char::is_control))?;
            Key::Text(t)
        }
    };
    Some(k)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ptr {
    Move,
    Leave,
    Press(u32),
    Release,
    Scroll(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Stay,
    Close,
    /// Become a full-screen surface without a card (keyboard focus goes back to windows).
    Fullscreen,
}

/// Where the card sits: layer-shell anchors (none = centred) and the gap to the anchored edges, logical px.
pub struct Place {
    pub anchor: Anchor,
    pub gap: i32,
    /// Card size, logical px at 100%.
    pub size: (f32, f32),
}

/// What a widget draws with: the card's top-left and size in buffer px, the scale, open (fade) and reveal progress.
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub s: f32,
    pub a: f32,
    pub reveal: f32,
    pub now: f32,
}

impl Frame {
    /// Decode progress for text at height `y` (buffer px): lower text starts a little later.
    pub fn decode_at(&self, y: f32) -> f32 {
        let lag = ((y - self.y) / self.h.max(1.0)).clamp(0.0, 1.0) * 0.35;
        ((self.reveal - lag) / 0.65).clamp(0.0, 1.0)
    }
}

pub trait Widget {
    fn place(&self) -> Place;
    fn key(&mut self, k: &Key, m: Mods, now: f32) -> Act;
    /// Pointer at (x, y) in buffer px relative to the card's top-left.
    fn pointer(&mut self, _x: f32, _y: f32, _p: Ptr, _now: f32) -> Act {
        Act::Stay
    }
    /// Advances time; true while something animates (draw every frame).
    fn tick(&mut self, _now: f32) -> bool {
        false
    }
    /// How often to redraw while idle (a blinking cursor, a moving progress line, background loads).
    fn period(&self) -> Option<f32> {
        None
    }
    fn draw(&mut self, cv: &mut Canvas, f: &Frame);
    /// Full-screen mode only: true when the widget is done and the process can exit.
    fn finished(&self) -> bool {
        false
    }
    /// Last chance to save before the process exits.
    fn close(&mut self) {}
}

pub enum Claim {
    /// This process is the only `name`; keep the file open for as long as it runs.
    Mine(std::fs::File),
    /// Another `name` is running (and was asked to close, if `toggle`).
    Taken,
    /// Could not tell (no runtime dir, unwritable): the caller must not assume anything is running.
    Failed,
}

/// Names the process (what pkill -x and pgrep see) and makes it single-instance through a lock file in
/// $XDG_RUNTIME_DIR (private to the user). With `toggle`, a second launch asks the running one to close.
pub fn claim(name: &str, toggle: bool) -> Claim {
    use std::io::{Read, Seek, Write};
    if let Ok(c) = std::ffi::CString::new(name) {
        // SAFETY: PR_SET_NAME with a valid NUL-terminated string (the kernel truncates to 15 bytes).
        unsafe { libc::prctl(libc::PR_SET_NAME, c.as_ptr() as libc::c_ulong, 0, 0, 0) };
    }
    let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") else { return Claim::Failed };
    let Ok(mut f) = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(format!("{dir}/{name}.lock")) else {
        return Claim::Failed;
    };
    // SAFETY: flock on a file descriptor we own.
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let mut pid = String::new();
        let _ = f.read_to_string(&mut pid);
        // Only a real process that is the same pop-up: never pid 0/-1 (process groups, everything).
        let comm = |p: i32| std::fs::read_to_string(format!("/proc/{p}/comm")).unwrap_or_default();
        if toggle
            && let Ok(pid) = pid.trim().parse::<i32>()
            && pid > 1
            && comm(pid).trim_end() == &name[..name.len().min(15)]
        {
            // SAFETY: plain kill(2) on a checked pid.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
        return Claim::Taken;
    }
    let _ = f.set_len(0);
    let _ = f.rewind();
    let _ = write!(f, "{}", std::process::id());
    Claim::Mine(f)
}

pub fn turbo() -> bool {
    crate::hypr::request("getoption animations:enabled").is_some_and(|r| r.contains("bool: false") || r.contains("int: 0"))
}

fn load_config() -> Config {
    crate::config::Watcher::new().poll().unwrap_or_default()
}

/// Draws the standard card behind a widget: soft shadow, frosted near-black, 1 px light edge.
pub fn card(cv: &mut Canvas, x: f32, y: f32, w: f32, h: f32, s: f32, a: f32) {
    let r = RADIUS * s;
    for i in 0..6 {
        let g = (i as f32 + 1.0) * 2.5 * s;
        cv.round_rect(x - g, y - g + 6.0 * s, w + 2.0 * g, h + 2.0 * g, r + g, Rgba(0.0, 0.0, 0.0, 0.05 * a));
    }
    cv.round_rect(x, y, w, h, r, CARD.alpha(0.94 * a));
    cv.stroke_round_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r, 1.0, Rgba(1.0, 1.0, 1.0, 0.1 * a));
}

/// A key hint row at the bottom of a card: "ENTER copy · DEL delete …" with the keys in white.
pub fn hints(cv: &mut Canvas, items: &[(&str, &str)], x: f32, base: f32, s: f32, a: f32) {
    let px = 9.5 * s;
    let mut pen = x;
    for (i, (k, what)) in items.iter().enumerate() {
        if i > 0 {
            pen = cv.text("  ·  ", pen, base, px, crate::canvas::Weight::Regular, DIM.alpha(a), None, 0.0);
        }
        pen = cv.text(k, pen, base, px, crate::canvas::Weight::Bold, WHITE.alpha(0.8 * a), None, 0.0);
        pen = cv.text(&format!(" {what}"), pen, base, px, crate::canvas::Weight::Regular, GREY.alpha(a), None, 0.0);
    }
}

/// Copies the pixmap (premultiplied RGBA) into an ARGB8888 buffer (BGRA bytes).
pub fn to_argb(cv: &Canvas, dst: &mut [u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(cv.pix.data().chunks_exact(4)) {
        d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
    }
}

/// Renders one fully open frame of `w` into a PNG (NOTHING_PREVIEW), for checking layouts without a session.
fn preview(w: &mut dyn Widget, cv: &mut Canvas, s: f32, path: &str) {
    let p = w.place();
    let (cw, ch) = (p.size.0 * s, p.size.1 * s);
    let m = MARGIN * s;
    cv.resize((cw + 2.0 * m) as u32, (ch + 2.0 * m) as u32);
    cv.clear(Rgba::hex(0x000000));
    let f = Frame { x: m, y: m, w: cw, h: ch, s, a: 1.0, reveal: 1.0, now: 10.0 };
    card(cv, f.x, f.y, f.w, f.h, s, 1.0);
    w.tick(10.0);
    w.draw(cv, &f);
    match cv.pix.save_png(path) {
        Ok(()) => eprintln!("preview: {path}"),
        Err(e) => eprintln!("preview: {e}"),
    }
}

enum Ev {
    Key(Key, Mods),
    Ptr(f64, f64, Ptr),
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    size: (u32, u32),
    scale: i32,
    configured: bool,
    frame_pending: bool,
    mods: Mods,
    events: Vec<Ev>,
    /// The key held down, for repeat (the compositor only sends the press).
    held: Option<(u32, Key, Mods, Instant)>,
    closed: bool,
}

/// Runs `widget` as the pop-up `name` until it closes. Returns the process exit code.
pub fn run(name: &str, mut widget: Box<dyn Widget>) -> i32 {
    let cfg = load_config();
    let fonts = match crate::canvas::Fonts::load(&cfg.font) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{name}: font: {e}");
            return 1;
        }
    };
    let mut cv = Canvas::new(1, 1, fonts);
    if let Ok(path) = std::env::var("NOTHING_PREVIEW") {
        preview(widget.as_mut(), &mut cv, cfg.size, &path);
        return 0;
    }
    let _lock = match claim(name, true) {
        Claim::Mine(f) => f,
        Claim::Taken => return 0,
        Claim::Failed => return 1,
    };
    crate::sys::install_signals();
    let still = turbo();

    let Ok(conn) = Connection::connect_to_env() else {
        eprintln!("{name}: not in a Wayland session");
        return 1;
    };
    let Ok((globals, mut queue)) = registry_queue_init(&conn) else { return 1 };
    let qh = queue.handle();
    let (Ok(compositor), Ok(shell), Ok(shm)) = (CompositorState::bind(&globals, &qh), LayerShell::bind(&globals, &qh), Shm::bind(&globals, &qh)) else {
        eprintln!("{name}: needs wl_compositor, wl_shm and wlr-layer-shell");
        return 1;
    };
    let layer = shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some(name), None);
    let place = widget.place();
    let z = cfg.size;
    let m = (MARGIN * z).round() as i32;
    let surface_size = |p: &Place| (((p.size.0 + 2.0 * MARGIN) * z).round() as u32, ((p.size.1 + 2.0 * MARGIN) * z).round() as u32);
    layer.set_anchor(place.anchor);
    layer.set_size(surface_size(&place).0, surface_size(&place).1);
    // The shadow margin is pulled in so the card itself keeps `gap` from the anchored edges.
    let g = place.gap - m;
    layer.set_margin(g, g, g, g);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.commit();

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        layer,
        pointer: None,
        keyboard: None,
        size: (0, 0),
        scale: 1,
        configured: false,
        frame_pending: false,
        mods: Mods::default(),
        events: Vec::new(),
        held: None,
        closed: false,
    };
    let mut pool = match SlotPool::new(4 * 800 * 600, &app.shm) {
        Ok(p) => p,
        Err(_) => return 1,
    };
    #[cfg(debug_assertions)]
    let mut script = std::env::var("NOTHING_KEYS").ok().map(|s| parse_script(&s)).unwrap_or_default();

    let t0 = Instant::now();
    let mut open = Tween::new(0.0);
    let mut reveal = Tween::new(if still { 1.0 } else { 0.0 });
    let mut opened = false;
    let mut closing = false;
    let mut fullscreen = false;
    let mut dirty = true;
    let mut last_period = Instant::now();
    loop {
        let now = t0.elapsed().as_secs_f32();
        if crate::sys::QUIT.load(Ordering::Relaxed) || app.closed {
            closing = true;
        }
        if app.configured && !opened {
            opened = true;
            open.to(1.0, now, if still { 0.0 } else { 0.18 });
            reveal.to(1.0, now, if still { 0.0 } else { 0.3 });
        }
        // Inputs
        #[cfg(debug_assertions)]
        if opened && now > 0.5 && !script.is_empty() {
            app.events.push(Ev::Key(script.remove(0), Mods::default()));
        }
        if let Some((_, k, m, at)) = &mut app.held
            && Instant::now() >= *at
        {
            *at = Instant::now() + Duration::from_millis(25);
            app.events.push(Ev::Key(k.clone(), *m));
        }
        let s = app.scale.max(1) as f32 * z;
        for ev in std::mem::take(&mut app.events) {
            if closing {
                break;
            }
            let act = match ev {
                Ev::Key(k, m) => widget.key(&k, m, now),
                Ev::Ptr(x, y, p) => {
                    let (x, y) = (x as f32 * app.scale as f32, y as f32 * app.scale as f32);
                    let (ox, oy) = if fullscreen { (0.0, 0.0) } else { (MARGIN * s, MARGIN * s) };
                    widget.pointer(x - ox, y - oy, p, now)
                }
            };
            match act {
                Act::Stay => {}
                Act::Close => closing = true,
                Act::Fullscreen if !fullscreen => {
                    fullscreen = true;
                    app.held = None;
                    app.layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
                    app.layer.set_size(0, 0);
                    app.layer.set_margin(0, 0, 0, 0);
                    app.layer.set_exclusive_zone(-1);
                    app.layer.set_keyboard_interactivity(KeyboardInteractivity::None);
                    app.layer.commit();
                    app.configured = false;
                }
                Act::Fullscreen => {}
            }
            dirty = true;
        }
        if closing && open.target() > 0.0 {
            open.to(0.0, now, if still || fullscreen { 0.0 } else { 0.12 });
            dirty = true;
        }
        if let Some(p) = widget.period()
            && last_period.elapsed().as_secs_f32() >= p
        {
            last_period = Instant::now();
            dirty = true;
        }
        // Draw
        if dirty && app.configured && !app.frame_pending {
            let animating = widget.tick(now) | open.moving(now) | reveal.moving(now);
            let sc = app.scale.max(1) as u32;
            cv.resize(app.size.0 * sc, app.size.1 * sc);
            cv.clear(Rgba(0.0, 0.0, 0.0, 0.0));
            let a = open.get(now);
            if fullscreen {
                let f = Frame { x: 0.0, y: 0.0, w: cv.pix.width() as f32, h: cv.pix.height() as f32, s, a: 1.0, reveal: 1.0, now };
                widget.draw(&mut cv, &f);
            } else if a > 0.0 {
                let p = widget.place();
                let slide = (1.0 - a) * 10.0 * s;
                let dy = if p.anchor.contains(Anchor::BOTTOM) { slide } else { -slide };
                let (x, y) = (MARGIN * s, MARGIN * s + dy);
                let (w, h) = (p.size.0 * s, p.size.1 * s);
                card(&mut cv, x, y, w, h, s, a);
                let r = reveal.get(now);
                let f = Frame { x, y, w, h, s, a, reveal: r, now };
                widget.draw(&mut cv, &f);
                let from = if p.anchor.contains(Anchor::TOP) {
                    (if p.anchor.contains(Anchor::LEFT) { 0.0 } else if p.anchor.contains(Anchor::RIGHT) { 1.0 } else { 0.5 }, 0.0)
                } else {
                    (0.5, 0.5)
                };
                crate::fx::dot_screen(&mut cv, x, y, w, h, 10.0 * s, RADIUS * s, r, from, CARD.alpha(0.95 * a));
            }
            let surface = app.layer.wl_surface().clone();
            if animating {
                surface.frame(&qh, FrameCallbackData(surface.clone()));
                app.frame_pending = true;
            }
            let (bw, bh) = (cv.pix.width() as i32, cv.pix.height() as i32);
            if let Ok((buffer, bytes)) = pool.create_buffer(bw, bh, bw * 4, wl_shm::Format::Argb8888) {
                to_argb(&cv, bytes);
                surface.set_buffer_scale(app.scale.max(1));
                surface.damage_buffer(0, 0, bw, bh);
                if buffer.attach_to(&surface).is_ok() {
                    app.layer.commit();
                }
            }
            dirty = animating;
        }
        if (closing && !open.moving(now) && open.target() == 0.0 && !fullscreen) || (fullscreen && widget.finished()) {
            widget.close();
            return 0;
        }
        // Sleep until something happens
        let mut timeout = Duration::from_millis(if dirty { 4 } else { 1000 });
        if let Some(p) = widget.period() {
            timeout = timeout.min(Duration::from_secs_f32(p));
        }
        if let Some((.., at)) = &app.held {
            timeout = timeout.min(at.saturating_duration_since(Instant::now()));
        }
        #[cfg(debug_assertions)]
        if !script.is_empty() {
            timeout = timeout.min(Duration::from_millis(8));
        }
        if closing {
            timeout = timeout.min(Duration::from_millis(16));
        }
        if wait(&conn, &mut queue, &mut app, timeout).is_err() {
            widget.close();
            return 1;
        }
    }
}

/// "abc|<enter>|<down>|<ctrl-n>" -> keys (debug builds: scripted input for testing on a remote box).
#[cfg(debug_assertions)]
pub fn parse_script(s: &str) -> Vec<Key> {
    let mut out = Vec::new();
    for part in s.split('|') {
        let k = match part {
            "<enter>" => Key::Enter,
            "<esc>" => Key::Esc,
            "<back>" => Key::Back,
            "<del>" => Key::Delete,
            "<up>" => Key::Up,
            "<down>" => Key::Down,
            "<left>" => Key::Left,
            "<right>" => Key::Right,
            "<tab>" => Key::Tab,
            p if p.starts_with("<ctrl-") && p.len() == 8 => Key::Ctrl(p.as_bytes()[6] as char),
            p if p.starts_with("<wait") => continue,
            p => {
                out.extend(p.chars().map(|c| Key::Text(c.to_string())));
                continue;
            }
        };
        out.push(k);
    }
    out
}

/// Dispatches Wayland events, then sleeps in poll() on the Wayland socket.
pub fn wait<D>(conn: &Connection, queue: &mut wayland_client::EventQueue<D>, app: &mut D, timeout: Duration) -> Result<(), String> {
    queue.dispatch_pending(app).map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    let Some(guard) = queue.prepare_read() else {
        return queue.dispatch_pending(app).map(|_| ()).map_err(|e| e.to_string());
    };
    let mut fd = libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
    // SAFETY: one valid pollfd for the duration of the call.
    let r = unsafe { libc::poll(&mut fd, 1, timeout.as_millis() as i32) };
    if r > 0 && fd.revents & libc::POLLIN != 0 {
        match guard.read() {
            Ok(_) => {}
            Err(wayland_client::backend::WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
    } else {
        drop(guard);
    }
    queue.dispatch_pending(app).map_err(|e| e.to_string())?;
    Ok(())
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, factor: i32) {
        self.scale = factor;
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.closed = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        self.size = (c.new_size.0.max(1), c.new_size.1.max(1));
        self.configured = true;
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        }
        if cap == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {
        self.held = None;
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        if let Some(k) = key_of(&ev, self.mods) {
            // Repeat like the input settings (250 ms, then 40/s), except for keys that act once.
            let repeats = !matches!(k, Key::Enter | Key::Esc | Key::Tab | Key::Ctrl(_));
            self.held = repeats.then(|| (ev.raw_code, k.clone(), self.mods, Instant::now() + Duration::from_millis(250)));
            self.events.push(Ev::Key(k, self.mods));
        }
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        if self.held.as_ref().is_some_and(|h| h.0 == ev.raw_code) {
            self.held = None;
        }
    }
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: RawModifiers, _: u32) {
        self.mods = Mods { ctrl: m.ctrl, shift: m.shift, caps: m.caps_lock };
    }
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            let (x, y) = e.position;
            let p = match e.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => Ptr::Move,
                PointerEventKind::Leave { .. } => Ptr::Leave,
                PointerEventKind::Press { button, .. } => Ptr::Press(button),
                PointerEventKind::Release { .. } => Ptr::Release,
                PointerEventKind::Axis { ref vertical, .. } => {
                    Ptr::Scroll(if vertical.discrete != 0 { vertical.discrete as f64 } else { (vertical.absolute / 10.0).trunc() })
                }
            };
            self.events.push(Ev::Ptr(x, y, p));
        }
    }
}

smithay_client_toolkit::delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(debug_assertions)]
    #[test]
    fn scripts_become_keys() {
        assert_eq!(parse_script("ab|<enter>|<ctrl-n>"), [Key::Text("a".into()), Key::Text("b".into()), Key::Enter, Key::Ctrl('n')]);
    }
}
