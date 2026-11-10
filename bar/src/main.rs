// nothing-bar: the nothing-os top bar for Hyprland.
//
// A wlr-layer-shell surface (namespace "nothing-bar") drawn on the CPU with tiny-skia into wl_shm buffers. The
// loop sleeps in poll() until something happens: a Wayland event, a Hyprland event (woken through an eventfd),
// or the next timer (clock second, stats every 2 s, config check every second). While an animation runs it draws
// on every frame callback, so it moves at the display's refresh rate; when nothing moves it draws nothing.
//
// Settings: ~/.config/nothing-bar/config (see config.rs). SIGUSR1: re-check turbo (Hyprland animations off ->
// bar animations off). SIGUSR2: hide / show. One instance per session.
mod anim;
mod bar;
mod canvas;
mod config;
mod hypr;
mod icons;
mod media;
mod status;
mod sys;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

static AUDIO_CHANGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MEDIA_CHANGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn poke(fd: i32) {
    let one: u64 = 1;
    // SAFETY: writes 8 bytes from a valid u64 to our eventfd.
    unsafe { libc::write(fd, &one as *const u64 as *const libc::c_void, 8) };
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    /// Logical size from the compositor.
    size: (u32, u32),
    scale: i32,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    /// Pointer input collected during dispatch, handled in the main loop.
    clicks: Vec<(f64, u32)>,
    /// (x, amount) of wheel scrolls over the bar.
    scrolls: Vec<(f64, f64)>,
    pointer_x: Option<f64>,
    pointer_moved: bool,
    hidden: bool,
    exit: bool,
}

fn main() {
    let _lock = match sys::single_instance() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("nothing-bar: {e}");
            std::process::exit(0);
        }
    };
    sys::install_signals();
    let debug = std::env::var_os("NOTHING_BAR_DEBUG").is_some();

    let mut watcher = config::Watcher::new();
    let cfg = watcher.poll().unwrap_or_default();
    let fonts = match canvas::Fonts::load(&cfg.font) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("nothing-bar: font: {e} (install ttf-jetbrains-mono)");
            std::process::exit(1);
        }
    };

    let conn = Connection::connect_to_env().expect("not in a Wayland session");
    let (globals, mut queue) = registry_queue_init(&conn).expect("Wayland registry");
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("wlr-layer-shell (needs Hyprland or another wlroots-style compositor)");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some("nothing-bar"), None);
    place(&layer, &cfg);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        layer,
        pointer: None,
        size: (0, 0),
        scale: 1,
        configured: false,
        frame_pending: false,
        dirty: true,
        clicks: Vec::new(),
        scrolls: Vec::new(),
        pointer_x: None,
        pointer_moved: false,
        hidden: false,
        exit: false,
    };
    while !app.configured {
        queue.blocking_dispatch(&mut app).expect("Wayland dispatch");
    }

    let mut pool = SlotPool::new(4 * 1920 * 64, &app.shm).expect("shm pool");
    let mut cv = canvas::Canvas::new(app.size.0, app.size.1, fonts);
    let mut bar = bar::Bar::new(cfg);
    bar.still = turbo();

    // Hyprland events arrive on a thread; an eventfd wakes the poll() below.
    // SAFETY: plain eventfd creation; the fd lives for the whole process.
    let wake_fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    let events: Arc<Mutex<Vec<hypr::Event>>> = Arc::default();
    {
        let events = events.clone();
        hypr::watch(move |e| {
            events.lock().unwrap().push(e);
            poke(wake_fd);
        });
    }
    status::watch_audio(move || {
        AUDIO_CHANGED.store(true, Ordering::Relaxed);
        poke(wake_fd);
    });
    if let Some(s) = hypr::snapshot() {
        bar.apply(s);
    }
    let media = media::watch(move || {
        MEDIA_CHANGED.store(true, Ordering::Relaxed);
        poke(wake_fd);
    });
    let mut cpu = sys::Cpu::new();
    let mut net = status::NetMeter::new();
    bar.set_stats(None, sys::ram_used_gib());
    bar.set_volume(status::volume());
    bar.set_clock();

    let mut last_tick = Instant::now();
    let mut last_frame: Vec<u8> = Vec::new();
    let mut slow_until: Option<Instant> = None;
    let mut next_clock = Instant::now();
    let mut next_stats = Instant::now() + Duration::from_millis(500);
    let mut next_config = Instant::now() + Duration::from_secs(1);

    while !app.exit && !sys::QUIT.load(Ordering::Relaxed) {
        let now = Instant::now();
        // -- inputs
        let pending: Vec<hypr::Event> = std::mem::take(&mut *events.lock().unwrap());
        if !pending.is_empty() {
            let mut changed = false;
            for e in pending {
                match e {
                    hypr::Event::Changed => changed = true,
                    hypr::Event::Urgent(a) => {
                        if let Some(s) = hypr::snapshot() {
                            bar.apply(s);
                        }
                        bar.urgent(&a);
                    }
                }
            }
            if changed && let Some(s) = hypr::snapshot() {
                bar.apply(s);
            }
            app.dirty = true;
        }
        if MEDIA_CHANGED.swap(false, Ordering::Relaxed) {
            bar.set_track(media.lock().unwrap().clone());
            app.dirty = true;
        }
        if AUDIO_CHANGED.swap(false, Ordering::Relaxed) {
            bar.set_volume(status::volume());
            app.dirty = true;
        }
        if app.pointer_moved {
            app.pointer_moved = false;
            bar.hover_x = app.pointer_x.map(|x| x as f32 * app.scale as f32);
            app.dirty = true;
        }
        if sys::USR2.swap(false, Ordering::Relaxed) {
            app.hidden = !app.hidden;
            if app.hidden {
                // A null buffer unmaps the layer surface; its space goes back to windows.
                app.layer.set_exclusive_zone(0);
                app.layer.wl_surface().attach(None, 0, 0);
                app.layer.commit();
                last_frame.clear();
            } else {
                // Remapping starts over: commit without a buffer, then draw after the new configure.
                place(&app.layer, &bar.cfg);
                app.configured = false;
                app.layer.commit();
            }
        }
        if sys::USR1.swap(false, Ordering::Relaxed) {
            bar.still = turbo();
            app.dirty = true;
        }
        for (x, button) in std::mem::take(&mut app.clicks) {
            use bar::Hit;
            match (bar.hit(x as f32 * app.scale as f32), button) {
                (Some(Hit::Workspace(id)), BTN_LEFT) => hypr::dispatch(&format!("hl.dsp.focus({{ workspace = {id} }})")),
                (Some(Hit::Stats), BTN_LEFT) => sys::spawn("kitty --class btop -e btop"),
                (Some(Hit::Volume), BTN_MIDDLE) => status::toggle_mute(),
                (Some(Hit::Volume), BTN_RIGHT) => sys::spawn("pavucontrol"),
                (Some(Hit::MediaPrev), BTN_LEFT) => media::command("previous"),
                (Some(Hit::MediaToggle), BTN_LEFT) => media::command("play-pause"),
                (Some(Hit::MediaNext), BTN_LEFT) => media::command("next"),
                _ => {}
            }
        }
        for (x, amount) in std::mem::take(&mut app.scrolls) {
            let xs = x as f32 * app.scale as f32;
            // Wheel up (negative) = louder / brighter / previous workspace.
            match bar.hit(xs) {
                Some(bar::Hit::Volume) => status::change_volume(amount < 0.0),
                Some(bar::Hit::Brightness) => {
                    status::change_brightness(amount < 0.0);
                    bar.set_brightness(status::brightness());
                    app.dirty = true;
                }
                _ if xs >= bar.ws_span.0 && xs < bar.ws_span.1 + 24.0 => {
                    let dir = if amount > 0.0 { "m+1" } else { "m-1" };
                    hypr::dispatch(&format!("hl.dsp.focus({{ workspace = \"{dir}\" }})"));
                }
                _ => {}
            }
        }
        // -- timers
        if now >= next_clock {
            bar.set_clock();
            app.dirty = true;
            next_clock = now + until_tick(bar.cfg.clock == config::ClockFormat::HhMmSs);
        }
        if now >= next_stats {
            bar.set_stats(cpu.sample(), sys::ram_used_gib());
            bar.net = net.sample();
            bar.set_brightness(status::brightness());
            bar.battery = status::battery();
            bar.dgpu = status::dgpu_awake();
            app.dirty = true;
            next_stats = now + Duration::from_secs(2);
        }
        if now >= next_config {
            if let Some(c) = watcher.poll() {
                if (c.height, c.position) != (bar.cfg.height, bar.cfg.position) {
                    place(&app.layer, &c);
                    app.layer.commit();
                }
                let refont = c.font != bar.cfg.font;
                bar.cfg = c;
                if refont && let Ok(f) = canvas::Fonts::load(&bar.cfg.font) {
                    cv.fonts = f;
                }
                bar.set_clock();
                app.dirty = true;
            }
            next_config = now + Duration::from_secs(1);
        }

        // -- draw: at once when idle, otherwise on the next frame callback
        let slow_ok = slow_until.is_none_or(|t| now >= t);
        if app.dirty && !app.frame_pending && slow_ok && app.configured && !app.hidden {
            let dt = last_tick.elapsed().as_secs_f32().min(0.1);
            last_tick = Instant::now();
            let motion = bar.tick(dt);
            let s = app.scale.max(1);
            cv.resize(app.size.0 * s as u32, app.size.1 * s as u32);
            let t0 = Instant::now();
            bar.draw(&mut cv, s as f32);
            if debug {
                eprintln!("nothing-bar: draw {:.3} ms ({motion:?})", t0.elapsed().as_secs_f64() * 1000.0);
            }
            present(&mut app, &mut pool, &cv, &mut last_frame, &qh, motion == bar::Motion::Fast);
            app.dirty = motion != bar::Motion::None;
            slow_until = (motion == bar::Motion::Slow).then(|| Instant::now() + Duration::from_secs_f32(bar.slow_interval));
        } else if !app.dirty {
            last_tick = Instant::now();
        }

        // -- sleep until something happens
        let mut wake = next_clock.min(next_stats).min(next_config);
        if app.dirty && !app.frame_pending && app.configured && !app.hidden {
            wake = slow_until.unwrap_or(now).min(wake);
        }
        let timeout = wake.saturating_duration_since(Instant::now()).min(Duration::from_secs(1));
        if let Err(e) = wait(&conn, &mut queue, &mut app, wake_fd, timeout) {
            eprintln!("nothing-bar: Wayland connection lost: {e}");
            break;
        }
    }
}

/// Time until the next wall-clock second (or minute), plus a few ms so the new value is surely there.
fn until_tick(seconds: bool) -> Duration {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_millis() as u64;
    let (_, _, sec, ..) = sys::local_time();
    let left = if seconds { 1000 - ms } else { (59 - sec as u64) * 1000 + (1000 - ms) };
    Duration::from_millis(left + 5)
}

fn turbo() -> bool {
    hypr::request("getoption animations:enabled").is_some_and(|r| r.contains("bool: false") || r.contains("int: 0"))
}

fn place(layer: &LayerSurface, cfg: &config::Config) {
    let edge = match cfg.position {
        config::Position::Top => Anchor::TOP,
        config::Position::Bottom => Anchor::BOTTOM,
    };
    layer.set_anchor(edge | Anchor::LEFT | Anchor::RIGHT);
    layer.set_size(0, cfg.height);
    layer.set_exclusive_zone(cfg.height as i32);
}

/// Copies the pixmap (premultiplied RGBA) into a shm buffer (ARGB8888 = BGRA bytes) and commits it.
/// A frame identical to the last one (e.g. a tween between two values that print the same) only asks for the
/// next frame callback, without a new buffer or damage.
fn present(app: &mut App, pool: &mut SlotPool, cv: &canvas::Canvas, last: &mut Vec<u8>, qh: &QueueHandle<App>, want_frame: bool) {
    let (w, h) = (cv.pix.width() as i32, cv.pix.height() as i32);
    let surface = app.layer.wl_surface();
    if want_frame {
        surface.frame(qh, FrameCallbackData(surface.clone()));
        app.frame_pending = true;
    }
    if last.as_slice() == cv.pix.data() {
        if want_frame {
            app.layer.commit();
        }
        return;
    }
    last.clear();
    last.extend_from_slice(cv.pix.data());
    let Ok((buffer, bytes)) = pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888) else {
        eprintln!("nothing-bar: no shm buffer");
        return;
    };
    for (dst, src) in bytes.chunks_exact_mut(4).zip(cv.pix.data().chunks_exact(4)) {
        dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
    }
    let surface = app.layer.wl_surface();
    surface.set_buffer_scale(app.scale.max(1));
    surface.damage_buffer(0, 0, w, h);
    if buffer.attach_to(surface).is_ok() {
        app.layer.commit();
    }
}

/// Dispatches Wayland events, then sleeps in poll() on the Wayland socket and the wake eventfd.
fn wait(conn: &Connection, queue: &mut wayland_client::EventQueue<App>, app: &mut App, wake_fd: i32, timeout: Duration) -> Result<(), String> {
    queue.dispatch_pending(app).map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    let Some(guard) = queue.prepare_read() else {
        return queue.dispatch_pending(app).map(|_| ()).map_err(|e| e.to_string());
    };
    let mut fds = [
        libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: wake_fd, events: libc::POLLIN, revents: 0 },
    ];
    // SAFETY: two valid pollfds for the duration of the call.
    let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout.as_millis() as i32) };
    if r > 0 && fds[0].revents & libc::POLLIN != 0 {
        match guard.read() {
            Ok(_) => {}
            // Spurious wake-up: nothing to read after all.
            Err(wayland_client::backend::WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
    } else {
        drop(guard);
    }
    if r > 0 && fds[1].revents & libc::POLLIN != 0 {
        let mut v: u64 = 0;
        // SAFETY: reads 8 bytes into a valid u64 from our nonblocking eventfd.
        unsafe { libc::read(wake_fd, &mut v as *mut u64 as *mut libc::c_void, 8) };
    }
    queue.dispatch_pending(app).map_err(|e| e.to_string())?;
    Ok(())
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, factor: i32) {
        if factor != self.scale {
            self.scale = factor;
            self.dirty = true;
        }
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
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        self.size = (c.new_size.0.max(1), c.new_size.1.max(1));
        self.configured = true;
        self.dirty = true;
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
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            match e.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.pointer_x = Some(e.position.0);
                    self.pointer_moved = true;
                }
                PointerEventKind::Leave { .. } => {
                    self.pointer_x = None;
                    self.pointer_moved = true;
                }
                PointerEventKind::Press { button, .. } => self.clicks.push((e.position.0, button)),
                PointerEventKind::Axis { vertical, .. } => {
                    // One switch per wheel notch (discrete) or per 10 px of touchpad scrolling.
                    let amount = if vertical.discrete != 0 { vertical.discrete as f64 } else { (vertical.absolute / 10.0).trunc() };
                    self.scrolls.push((e.position.0, amount));
                }
                _ => {}
            }
        }
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);
