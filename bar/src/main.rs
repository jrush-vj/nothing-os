// nothing-bar: the nothing-os top bar for Hyprland.
//
// Wlr-layer-shell surfaces drawn on the CPU with tiny-skia into wl_shm buffers: the bar ("nothing-bar"), and while
// it's open the control centre ("nothing-bar-panel") plus a transparent full-screen click catcher behind it
// ("nothing-bar-catcher": a 1x1 buffer stretched by wp_viewporter) so a click anywhere else closes the panel.
// The loop sleeps in poll() until something happens: a Wayland event, a background thread (woken through an
// eventfd), or the next timer. While an animation runs it draws on every frame callback, so it moves at the
// display's refresh rate; when nothing moves it draws nothing.
//
// Settings: ~/.config/nothing-bar/config (see config.rs). SIGUSR1: re-check turbo (Hyprland animations off ->
// bar animations off). SIGUSR2: hide / show. One instance per session.
// Debugging: NOTHING_BAR_DEBUG=1 logs frame times; NOTHING_BAR_OPEN=main|settings|wifi|bluetooth|notifications opens the control
// centre on that page at start (for screenshots).
mod anim;
mod bar;
mod canvas;
mod cc;
mod config;
mod hypr;
mod icons;
mod media;
mod notif;
mod panel;
mod status;
mod sys;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    reexports::protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Proxy, QueueHandle,
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

static AUDIO_CHANGED: AtomicBool = AtomicBool::new(false);
static MEDIA_CHANGED: AtomicBool = AtomicBool::new(false);
static CC_CHANGED: AtomicBool = AtomicBool::new(false);
static NOTIFY_CHANGED: AtomicBool = AtomicBool::new(true);

fn poke(fd: i32) {
    let one: u64 = 1;
    // SAFETY: writes 8 bytes from a valid u64 to our eventfd.
    unsafe { libc::write(fd, &one as *const u64 as *const libc::c_void, 8) };
}

/// User data for protocol objects whose events we don't need (viewporter).
struct Noop;
impl<I: Proxy> Dispatch2<I, App> for Noop {
    fn event(&self, _: &mut App, _: &I, _: I::Event, _: &Connection, _: &QueueHandle<App>) {}
}

/// Pointer input on the control centre, handled in the main loop.
enum PanelInput {
    Press(f64, f64, u32),
    Release,
    Scroll(f64, f64, f64),
}

/// A tooltip under the bar: its own small overlay surface, sized to the text.
struct Tip {
    layer: LayerSurface,
    text: String,
    size: (u32, u32),
    configured: bool,
    drawn: bool,
}

/// The control centre's surfaces while it exists.
struct Cc {
    panel: LayerSurface,
    configured: bool,
    frame_pending: bool,
    /// Removed as soon as the panel starts closing, so clicks reach windows again at once.
    catcher: Option<(LayerSurface, WpViewport)>,
    catcher_size: Option<(u32, u32)>,
    catcher_drawn: bool,
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
    /// Pointer input on the bar, collected during dispatch.
    clicks: Vec<(f64, u32)>,
    scrolls: Vec<(f64, f64)>,
    pointer_x: Option<f64>,
    pointer_moved: bool,
    hidden: bool,
    cc: Option<Cc>,
    cc_dirty: bool,
    panel_input: Vec<PanelInput>,
    panel_pointer: Option<(f64, f64)>,
    panel_moved: bool,
    catcher_clicked: bool,
    tip: Option<Tip>,
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
    let load_fonts = |family: &str| match canvas::Fonts::load(family) {
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
    let viewporter: Option<WpViewporter> = globals.bind(&qh, 1..=1, Noop).ok();
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some("nothing-bar"), None);
    place(&layer, &cfg, cfg.shape);
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
        cc: None,
        cc_dirty: false,
        panel_input: Vec::new(),
        panel_pointer: None,
        panel_moved: false,
        catcher_clicked: false,
        tip: None,
        exit: false,
    };
    while !app.configured {
        queue.blocking_dispatch(&mut app).expect("Wayland dispatch");
    }

    let mut pool = SlotPool::new(4 * 1920 * 64, &app.shm).expect("shm pool");
    let mut cv = canvas::Canvas::new(app.size.0, app.size.1, load_fonts(&cfg.font));
    let mut pcv = canvas::Canvas::new(panel::SURFACE_W, panel::SURFACE_H, load_fonts(&cfg.font));
    let mut bar = bar::Bar::new(cfg.clone());
    let mut panel = panel::Panel::new(cfg);
    bar.still = turbo();

    // Background threads report through shared state + an eventfd that wakes the poll() below.
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
    let cc_state: cc::Shared = Arc::default();
    let cc_done = move || {
        CC_CHANGED.store(true, Ordering::Relaxed);
        poke(wake_fd);
    };
    let dgpu_watts: Arc<Mutex<Option<f32>>> = Arc::default();
    notif::watch(move || {
        NOTIFY_CHANGED.store(true, Ordering::Relaxed);
        poke(wake_fd);
    });
    // Notifications already there at start are not "new".
    let mut seen_id = notif::read().first().map_or(0, |n| n.id);
    let mut cleared_id = 0u32;
    let mut cpu = sys::Cpu::new();
    let mut net = status::NetMeter::new();
    bar.set_stats(None, sys::ram_used_gib());
    let v = status::volume();
    bar.set_volume(v);
    panel.vol = v;
    panel.bright = status::brightness();
    bar.set_clock();

    let mut last_tick = Instant::now();
    let mut last_frame: Vec<u8> = Vec::new();
    let mut panel_last: Vec<u8> = Vec::new();
    let mut panel_tick = Instant::now();
    let mut slow_until: Option<Instant> = None;
    let mut next_clock = Instant::now();
    let mut next_stats = Instant::now() + Duration::from_millis(500);
    let mut next_config = Instant::now() + Duration::from_secs(1);
    let mut next_cc = Instant::now();
    let mut last_slider_set: Option<(panel::Slider, f32)> = None;
    let mut debug_open = std::env::var("NOTHING_BAR_OPEN").ok();
    let mut hover_since: Option<(bar::Hit, Instant)> = None;
    // What the bar surface is currently placed as, and the auto-hide input state.
    let mut placed = (bar.cfg.height, bar.cfg.position, bar.cfg.shape);
    let mut strip_input = false;
    let mut hide_at: Option<Instant> = None;
    let mut tip_cv = canvas::Canvas::new(1, 1, load_fonts(&bar.cfg.font));

    while !app.exit && !sys::QUIT.load(Ordering::Relaxed) {
        let now = Instant::now();
        // ---------------------------------------------------------------- inputs
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
            let t = media.lock().unwrap().clone();
            panel.track = t.clone();
            bar.set_track(t);
            app.dirty = true;
            app.cc_dirty = true;
        }
        if AUDIO_CHANGED.swap(false, Ordering::Relaxed) {
            let v = status::volume();
            bar.set_volume(v);
            panel.vol = v;
            app.dirty = true;
            app.cc_dirty = true;
        }
        if NOTIFY_CHANGED.swap(false, Ordering::Relaxed) {
            let notes = notif::read();
            bar.unread = notes.iter().filter(|n| n.id > seen_id).count();
            panel.notes = notes.into_iter().filter(|n| n.id > cleared_id).take(panel::MAX_NOTES).collect();
            app.dirty = true;
            app.cc_dirty = true;
        }
        if CC_CHANGED.swap(false, Ordering::Relaxed) {
            panel.state = cc_state.lock().unwrap().clone();
            app.cc_dirty = true;
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
                place(&app.layer, &bar.cfg, bar.shape());
                app.configured = false;
                app.layer.commit();
            }
        }
        if sys::USR1.swap(false, Ordering::Relaxed) {
            bar.still = turbo();
            panel.turbo = bar.still;
            app.dirty = true;
            app.cc_dirty = true;
        }
        if let Some(page) = debug_open.take()
            && let Some(vp) = &viewporter
        {
            open_cc(&mut app, &layer_shell, &compositor, vp, &qh, &bar.cfg);
            panel.show(bar.still);
            panel.page = match page.as_str() {
                "settings" => panel::Page::Settings,
                "wifi" => panel::Page::Wifi,
                "bluetooth" => panel::Page::Bluetooth,
                "notifications" => panel::Page::Notifications,
                _ => panel::Page::Main,
            };
            panel.graphs.disk = cc::disk_usage();
            cc::refresh(&cc_state, panel.page != panel::Page::Main, cc_done);
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
                (Some(Hit::Bell), BTN_LEFT) => {
                    if panel.is_open() {
                        close_cc(&mut app, &mut panel, bar.still);
                    } else if let Some(vp) = &viewporter {
                        open_cc(&mut app, &layer_shell, &compositor, vp, &qh, &bar.cfg);
                        panel.show(bar.still);
                        panel.page = panel::Page::Notifications;
                        seen_id = panel.notes.first().map_or(seen_id, |n| n.id.max(seen_id));
                        bar.unread = 0;
                        panel_last.clear();
                        panel_tick = Instant::now();
                        app.dirty = true;
                    }
                }
                (Some(Hit::Net | Hit::Brightness | Hit::Volume | Hit::Battery), BTN_LEFT) => {
                    if panel.is_open() {
                        close_cc(&mut app, &mut panel, bar.still);
                    } else if let Some(vp) = &viewporter {
                        open_cc(&mut app, &layer_shell, &compositor, vp, &qh, &bar.cfg);
                        panel.show(bar.still);
                        panel.turbo = bar.still;
                        panel.cfg = bar.cfg.clone();
                        panel.graphs.disk = cc::disk_usage();
                        cc::refresh(&cc_state, false, cc_done);
                        panel_last.clear();
                        panel_tick = Instant::now();
                        next_cc = now + Duration::from_secs(5);
                    }
                }
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
                    let b = status::brightness();
                    bar.set_brightness(b);
                    panel.bright = b;
                    app.dirty = true;
                }
                _ if xs >= bar.ws_span.0 && xs < bar.ws_span.1 + 24.0 => {
                    let dir = if amount > 0.0 { "m+1" } else { "m-1" };
                    hypr::dispatch(&format!("hl.dsp.focus({{ workspace = \"{dir}\" }})"));
                }
                _ => {}
            }
        }
        // ---------------------------------------------------------------- control centre input
        if std::mem::take(&mut app.catcher_clicked) && panel.is_open() {
            close_cc(&mut app, &mut panel, bar.still);
        }
        let ps = app.scale as f32;
        if app.panel_moved {
            app.panel_moved = false;
            panel.pointer = app.panel_pointer.map(|(x, y)| (x as f32 * ps, y as f32 * ps));
            if let (Some(which), Some((x, _))) = (panel.drag, panel.pointer) {
                set_slider(&mut panel, &mut bar, which, x, &mut last_slider_set);
            }
            app.cc_dirty = true;
        }
        for input in std::mem::take(&mut app.panel_input) {
            use panel::{PHit, Page, Session};
            match input {
                PanelInput::Release => panel.drag = None,
                PanelInput::Scroll(x, y, amount) => match panel.hit(x as f32 * ps, y as f32 * ps) {
                    Some(PHit::Slider(panel::Slider::Volume)) => status::change_volume(amount < 0.0),
                    Some(PHit::Slider(panel::Slider::Brightness)) => {
                        status::change_brightness(amount < 0.0);
                        panel.bright = status::brightness();
                        bar.set_brightness(panel.bright);
                        app.dirty = true;
                    }
                    _ => {}
                },
                PanelInput::Press(x, y, button) if button == BTN_LEFT => {
                    let (x, y) = (x as f32 * ps, y as f32 * ps);
                    let st = panel.state.clone();
                    let act = |args: Vec<String>, lists: bool| {
                        let shared = cc_state.clone();
                        std::thread::spawn(move || {
                            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
                            cc::run_wait(&argv);
                            *shared.lock().unwrap() = cc::read_state(lists);
                            cc_done();
                        });
                    };
                    let on_off = |on: bool| if on { "off".to_string() } else { "on".to_string() };
                    match panel.hit(x, y) {
                        Some(PHit::Wifi) => act(vec!["nmcli".into(), "radio".into(), "wifi".into(), on_off(st.wifi_on)], panel.page == Page::Wifi),
                        Some(PHit::Bt) => act(vec!["bluetoothctl".into(), "power".into(), on_off(st.bt_on)], panel.page == Page::Bluetooth),
                        Some(PHit::Mic) => act(vec!["wpctl".into(), "set-mute".into(), "@DEFAULT_AUDIO_SOURCE@".into(), "toggle".into()], false),
                        Some(PHit::Turbo) => {
                            // The same toggle as SUPER+F12 (keys.lua); it signals us back with SIGUSR1.
                            hypr::request("eval TOGGLE_TURBO()");
                        }
                        Some(PHit::Profile(i)) => act(vec!["powerprofilesctl".into(), "set".into(), panel::PROFILES[i].1.into()], false),
                        Some(PHit::Networks) => {
                            panel.page = Page::Wifi;
                            cc::refresh(&cc_state, true, cc_done);
                        }
                        Some(PHit::Devices) => {
                            panel.page = Page::Bluetooth;
                            cc::refresh(&cc_state, true, cc_done);
                        }
                        Some(PHit::Slider(which)) => {
                            panel.drag = Some(which);
                            set_slider(&mut panel, &mut bar, which, x, &mut last_slider_set);
                        }
                        Some(PHit::MediaPrev) => media::command("previous"),
                        Some(PHit::MediaToggle) => media::command("play-pause"),
                        Some(PHit::MediaNext) => media::command("next"),
                        Some(PHit::Session(s)) => {
                            if let Some(s) = panel.press_session(s) {
                                close_cc(&mut app, &mut panel, bar.still);
                                match s {
                                    Session::Lock => cc::run(&["loginctl", "lock-session"]),
                                    Session::Logout => hypr::dispatch("hl.dsp.exit()"),
                                    Session::Reboot => cc::run(&["systemctl", "reboot"]),
                                    Session::Off => cc::run(&["systemctl", "poweroff"]),
                                }
                            }
                        }
                        Some(PHit::Settings) => panel.page = Page::Settings,
                        Some(PHit::Back) => panel.page = Page::Main,
                        Some(PHit::Setting(i)) => {
                            let mut c = bar.cfg.clone();
                            c.cycle(i);
                            if let Err(e) = c.save() {
                                eprintln!("nothing-bar: saving settings: {e}");
                            }
                            apply_config(&mut app, &mut bar, &mut cv, c.clone());
                            panel.cfg = c;
                        }
                        Some(PHit::Net(i)) => {
                            if let Some(n) = st.networks.get(i) {
                                cc::connect_wifi(n, &st.saved);
                                next_cc = now + Duration::from_secs(3);
                            }
                        }
                        Some(PHit::ClearAll) => {
                            notif::clear_all();
                            cleared_id = panel.notes.first().map_or(cleared_id, |n| n.id.max(cleared_id));
                            panel.notes.clear();
                        }
                        Some(PHit::Note(_)) => {}
                        Some(PHit::Dev(i)) => {
                            if let Some(d) = st.devices.get(i) {
                                let verb = if d.connected { "disconnect" } else { "connect" };
                                act(vec!["bluetoothctl".into(), verb.into(), d.mac.clone()], true);
                            }
                        }
                        None => {}
                    }
                    app.cc_dirty = true;
                }
                PanelInput::Press(..) => {}
            }
        }
        // ---------------------------------------------------------------- geometry and auto-hide
        let want = (bar.cfg.height, bar.cfg.position, bar.shape());
        if want != placed && !app.hidden {
            place(&app.layer, &bar.cfg, want.2);
            app.layer.commit();
            placed = want;
            app.dirty = true;
        }
        if bar.cfg.position == config::Position::AutoHide {
            let inside = app.pointer_x.is_some() || panel.is_open();
            if inside {
                hide_at = None;
                if !bar.revealed() {
                    bar.set_revealed(true);
                    app.dirty = true;
                }
            } else if bar.revealed() {
                let t = *hide_at.get_or_insert(now + Duration::from_millis(600));
                if now >= t {
                    bar.set_revealed(false);
                    hide_at = None;
                    app.dirty = true;
                }
            }
        } else if !bar.revealed() {
            bar.set_revealed(true);
            app.dirty = true;
        }
        let want_strip = bar.cfg.position == config::Position::AutoHide && !bar.revealed();
        if want_strip != strip_input {
            set_input(&app.layer, &compositor, app.size.0, want_strip);
            app.layer.commit();
            strip_input = want_strip;
        }

        // ---------------------------------------------------------------- tooltip
        let hovered = if panel.is_open() || !app.clicks.is_empty() { None } else { bar.tooltip() };
        match (&hovered, hover_since) {
            (Some((hit, ..)), Some((h, _))) if *hit == h => {}
            (Some((hit, ..)), _) => {
                hover_since = Some((*hit, now));
                app.tip = None;
            }
            (None, _) => {
                hover_since = None;
                app.tip = None;
            }
        }
        if let (Some((_, cx, text)), Some((_, since))) = (&hovered, hover_since)
            && now.duration_since(since) >= Duration::from_millis(500)
            && app.tip.as_ref().is_none_or(|t| &t.text != text)
        {
            let s = app.scale.max(1) as f32;
            let w = (tip_cv.fonts.width(canvas::Weight::Regular, 11.0 * s, text) / s + 24.0).ceil() as u32;
            let h = 26u32;
            let left = ((cx / s) - w as f32 / 2.0).clamp(6.0, app.size.0 as f32 - w as f32 - 6.0);
            let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("nothing-bar-tooltip"), None);
            let top = bar.cfg.position.at_top();
            layer.set_anchor(if top { Anchor::TOP } else { Anchor::BOTTOM } | Anchor::LEFT);
            layer.set_size(w, h);
            if top {
                layer.set_margin(4, 0, 0, left as i32);
            } else {
                layer.set_margin(0, 0, 4, left as i32);
            }
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.commit();
            app.tip = Some(Tip { layer, text: text.clone(), size: (w, h), configured: false, drawn: false });
        }
        if let Some(t) = &mut app.tip
            && t.configured
            && !t.drawn
        {
            let s = app.scale.max(1);
            tip_cv.resize(t.size.0 * s as u32, t.size.1 * s as u32);
            let sf = s as f32;
            tip_cv.clear(canvas::Rgba(0.0, 0.0, 0.0, 0.0));
            let (w, h) = (tip_cv.pix.width() as f32, tip_cv.pix.height() as f32);
            tip_cv.round_rect(0.0, 0.0, w, h, 9.0 * sf, canvas::Rgba::hex(0x0d0d0d).alpha(0.92));
            tip_cv.stroke_round_rect(0.5, 0.5, w - 1.0, h - 1.0, 9.0 * sf, 1.0, canvas::Rgba(1.0, 1.0, 1.0, 0.1));
            let base = ((h + tip_cv.fonts.cap_height(canvas::Weight::Regular, 11.0 * sf)) / 2.0).round();
            tip_cv.text(&t.text, 12.0 * sf, base, 11.0 * sf, canvas::Weight::Regular, bar::WHITE, None, 0.0);
            let mut none = Vec::new();
            present(&t.layer, app.scale, &mut pool, &tip_cv, &mut none, &qh, false);
            t.drawn = true;
        }

        // ---------------------------------------------------------------- timers
        if now >= next_clock {
            bar.set_clock();
            app.dirty = true;
            next_clock = now + until_tick(bar.cfg.clock == config::ClockFormat::HhMmSs);
        }
        if now >= next_stats {
            let (c, r) = (cpu.sample(), sys::ram_used_gib());
            bar.set_stats(c, r);
            bar.net = net.sample();
            let b = status::brightness();
            bar.set_brightness(b);
            bar.battery = status::battery();
            bar.dgpu = status::dgpu_awake();
            // Graph history is kept even while the panel is closed, so it has something to show on open.
            let open = panel.is_open();
            let g = &mut panel.graphs;
            g.cpu.push(c.unwrap_or(0.0));
            g.ram.push(r.unwrap_or(0.0));
            g.temp.push(cc::cpu_temp().unwrap_or(0.0));
            g.igpu.push(cc::igpu_load().unwrap_or(0.0));
            g.rx.push(bar.net.as_ref().map_or(0.0, |n| n.rx_rate as f32));
            g.tx.push(bar.net.as_ref().map_or(0.0, |n| n.tx_rate as f32));
            g.dgpu_awake = bar.dgpu;
            if open {
                g.disk = cc::disk_usage();
                if bar.dgpu {
                    // nvidia-smi can take a moment; it's only asked while the GPU is awake anyway.
                    let w = dgpu_watts.clone();
                    std::thread::spawn(move || *w.lock().unwrap() = cc::dgpu_watts());
                    if let Some(watts) = *dgpu_watts.lock().unwrap() {
                        g.dgpu.push(watts);
                    }
                }
                app.cc_dirty = true;
            }
            panel.bright = b;
            app.dirty = true;
            next_stats = now + Duration::from_secs(2);
        }
        if panel.is_open() && now >= next_cc {
            use panel::Page;
            cc::refresh(&cc_state, matches!(panel.page, Page::Wifi | Page::Bluetooth), cc_done);
            next_cc = now + Duration::from_secs(5);
        }
        if now >= next_config {
            if let Some(c) = watcher.poll() {
                apply_config(&mut app, &mut bar, &mut cv, c.clone());
                panel.cfg = c;
            }
            next_config = now + Duration::from_secs(1);
        }

        // ---------------------------------------------------------------- draw the bar
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
            let surface = app.layer.clone();
            if present(&surface, app.scale, &mut pool, &cv, &mut last_frame, &qh, motion == bar::Motion::Fast) {
                app.frame_pending = true;
            }
            app.dirty = motion != bar::Motion::None;
            slow_until = (motion == bar::Motion::Slow).then(|| Instant::now() + Duration::from_secs_f32(bar.slow_interval));
        } else if !app.dirty {
            last_tick = Instant::now();
        }

        // ---------------------------------------------------------------- draw the control centre
        if let Some(c) = &mut app.cc {
            if let (Some((w, h)), false, Some((_, vp))) = (c.catcher_size, c.catcher_drawn, &c.catcher) {
                // A 1x1 transparent buffer, stretched over the screen: it only exists to receive clicks.
                if let Ok((buffer, bytes)) = pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888) {
                    bytes.fill(0);
                    let (layer, _) = c.catcher.as_ref().unwrap();
                    vp.set_destination(w as i32, h as i32);
                    if buffer.attach_to(layer.wl_surface()).is_ok() {
                        layer.commit();
                        c.catcher_drawn = true;
                    }
                }
            }
        }
        let cc_ready = app.cc.as_ref().is_some_and(|c| c.configured && !c.frame_pending);
        if cc_ready && app.cc_dirty {
            let dt = panel_tick.elapsed().as_secs_f32().min(0.1);
            panel_tick = Instant::now();
            let animating = panel.tick(dt);
            let s = app.scale.max(1) as u32;
            pcv.resize(panel::SURFACE_W * s, panel::SURFACE_H * s);
            panel.draw(&mut pcv, s as f32);
            let c = app.cc.as_mut().unwrap();
            let layer = c.panel.clone();
            if present(&layer, app.scale, &mut pool, &pcv, &mut panel_last, &qh, animating) {
                c.frame_pending = true;
            }
            app.cc_dirty = animating;
            if panel.gone() {
                app.cc = None; // dropping the LayerSurface destroys it
                panel_last.clear();
            }
        } else if !app.cc_dirty {
            panel_tick = Instant::now();
        }

        // ---------------------------------------------------------------- sleep until something happens
        let mut wake = next_clock.min(next_stats).min(next_config);
        if panel.is_open() {
            wake = wake.min(next_cc);
        }
        if let Some((_, since)) = hover_since
            && app.tip.is_none()
        {
            wake = wake.min(since + Duration::from_millis(510));
        }
        if let Some(t) = hide_at {
            wake = wake.min(t);
        }
        if app.dirty && !app.frame_pending && app.configured && !app.hidden {
            wake = slow_until.unwrap_or(now).min(wake);
        }
        if app.cc_dirty && app.cc.as_ref().is_some_and(|c| c.configured && !c.frame_pending) {
            wake = now;
        }
        let timeout = wake.saturating_duration_since(Instant::now()).min(Duration::from_secs(1));
        if let Err(e) = wait(&conn, &mut queue, &mut app, wake_fd, timeout) {
            eprintln!("nothing-bar: Wayland connection lost: {e}");
            break;
        }
    }
}

/// Applies a (changed) config: re-places the bar if its geometry changed, reloads the font if needed.
fn apply_config(app: &mut App, bar: &mut bar::Bar, cv: &mut canvas::Canvas, c: config::Config) {
    let refont = c.font != bar.cfg.font;
    bar.cfg = c;
    // Geometry (height, position, shape) is re-placed by the main loop when it differs from what's placed.
    if refont && let Ok(f) = canvas::Fonts::load(&bar.cfg.font) {
        cv.fonts = f;
    }
    bar.set_clock();
    app.dirty = true;
}

/// Sets volume or brightness from a slider position (skipping repeats of the same whole percent).
fn set_slider(panel: &mut panel::Panel, bar: &mut bar::Bar, which: panel::Slider, x: f32, last: &mut Option<(panel::Slider, f32)>) {
    let Some(v) = panel.slider_value(which, x) else { return };
    let pct = (v * 100.0).round();
    if *last == Some((which, pct)) {
        return;
    }
    *last = Some((which, pct));
    match which {
        panel::Slider::Volume => {
            cc::run_wait(&["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", pct / 100.0)]);
            let muted = panel.vol.is_some_and(|(_, m)| m);
            panel.vol = Some((pct, muted));
            bar.set_volume(panel.vol);
        }
        panel::Slider::Brightness => {
            cc::run_wait(&["brightnessctl", "-q", "set", &format!("{pct}%")]);
            panel.bright = Some(pct);
            bar.set_brightness(panel.bright);
        }
    }
}

fn open_cc(app: &mut App, shell: &LayerShell, compositor: &CompositorState, viewporter: &WpViewporter, qh: &QueueHandle<App>, cfg: &config::Config) {
    let edge = if cfg.position.at_top() { Anchor::TOP } else { Anchor::BOTTOM };
    // Created first, so it sits below the panel on the overlay layer.
    let catcher = shell.create_layer_surface(qh, compositor.create_surface(qh), Layer::Overlay, Some("nothing-bar-catcher"), None);
    catcher.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    catcher.set_exclusive_zone(-1);
    catcher.set_keyboard_interactivity(KeyboardInteractivity::None);
    catcher.commit();
    let viewport = viewporter.get_viewport(catcher.wl_surface(), qh, Noop);

    let panel = shell.create_layer_surface(qh, compositor.create_surface(qh), Layer::Overlay, Some("nothing-bar-panel"), None);
    panel.set_anchor(edge | Anchor::RIGHT);
    panel.set_size(panel::SURFACE_W, panel::SURFACE_H);
    // The surface has a transparent margin for the shadow; pull it in so the card sits 6 px from the bar and 8 px
    // from the screen edge.
    let m = panel::MARGIN as i32;
    if cfg.position.at_top() {
        panel.set_margin(6 - m, 8 - m, 0, 0);
    } else {
        panel.set_margin(0, 8 - m, 6 - m, 0);
    }
    panel.set_keyboard_interactivity(KeyboardInteractivity::None);
    panel.commit();
    app.cc = Some(Cc { panel, configured: false, frame_pending: false, catcher: Some((catcher, viewport)), catcher_size: None, catcher_drawn: false });
    app.cc_dirty = true;
}

/// Starts the close animation; the click catcher goes at once.
fn close_cc(app: &mut App, panel: &mut panel::Panel, still: bool) {
    panel.hide(still);
    if let Some(c) = &mut app.cc
        && let Some((_, vp)) = c.catcher.take()
    {
        vp.destroy();
    }
    app.cc_dirty = true;
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

/// Anchors and sizes the bar for its position and shape (floating and island shapes sit off the edge).
fn place(layer: &LayerSurface, cfg: &config::Config, shape: config::Shape) {
    use config::Shape;
    let top = cfg.position.at_top();
    layer.set_anchor(if top { Anchor::TOP } else { Anchor::BOTTOM } | Anchor::LEFT | Anchor::RIGHT);
    layer.set_size(0, cfg.height);
    let (edge, side) = match shape {
        Shape::Strip => (0, 0),
        Shape::Floating => (6, 8),
        Shape::Islands | Shape::Island => (5, 6),
    };
    if top {
        layer.set_margin(edge, side, 0, side);
    } else {
        layer.set_margin(0, side, edge, side);
    }
    // Auto-hide reserves no space: windows use the full height and the bar slides over them.
    layer.set_exclusive_zone(if cfg.position == config::Position::AutoHide { 0 } else { cfg.height as i32 });
}

/// Auto-hide: while tucked away only a thin strip along the top edge takes the pointer; otherwise the whole bar.
fn set_input(layer: &LayerSurface, compositor: &CompositorState, width: u32, strip: bool) {
    let surface = layer.wl_surface();
    if strip {
        if let Ok(region) = Region::new(compositor) {
            region.add(0, 0, width as i32, 2);
            surface.set_input_region(Some(region.wl_region()));
        }
    } else {
        surface.set_input_region(None);
    }
}

/// Copies the pixmap (premultiplied RGBA) into a shm buffer (ARGB8888 = BGRA bytes) and commits it. A frame
/// identical to the last one only asks for the next frame callback, without a new buffer or damage.
/// Returns true if a frame callback was requested.
fn present(layer: &LayerSurface, scale: i32, pool: &mut SlotPool, cv: &canvas::Canvas, last: &mut Vec<u8>, qh: &QueueHandle<App>, want_frame: bool) -> bool {
    let (w, h) = (cv.pix.width() as i32, cv.pix.height() as i32);
    let surface = layer.wl_surface();
    if want_frame {
        surface.frame(qh, FrameCallbackData(surface.clone()));
    }
    if last.as_slice() == cv.pix.data() {
        if want_frame {
            layer.commit();
        }
        return want_frame;
    }
    last.clear();
    last.extend_from_slice(cv.pix.data());
    let Ok((buffer, bytes)) = pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888) else {
        eprintln!("nothing-bar: no shm buffer");
        return false;
    };
    for (dst, src) in bytes.chunks_exact_mut(4).zip(cv.pix.data().chunks_exact(4)) {
        dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
    }
    surface.set_buffer_scale(scale.max(1));
    surface.damage_buffer(0, 0, w, h);
    if buffer.attach_to(surface).is_ok() {
        layer.commit();
    }
    want_frame
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

impl App {
    fn is_panel(&self, s: &wl_surface::WlSurface) -> bool {
        self.cc.as_ref().is_some_and(|c| c.panel.wl_surface() == s)
    }
    fn is_catcher(&self, s: &wl_surface::WlSurface) -> bool {
        self.cc.as_ref().and_then(|c| c.catcher.as_ref()).is_some_and(|(l, _)| l.wl_surface() == s)
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, s: &wl_surface::WlSurface, factor: i32) {
        if s == self.layer.wl_surface() && factor != self.scale {
            self.scale = factor;
            self.dirty = true;
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, s: &wl_surface::WlSurface, _: u32) {
        if s == self.layer.wl_surface() {
            self.frame_pending = false;
        } else if let Some(c) = &mut self.cc
            && c.panel.wl_surface() == s
        {
            c.frame_pending = false;
        }
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
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if layer == &self.layer {
            self.exit = true;
        } else if self.tip.as_ref().is_some_and(|t| &t.layer == layer) {
            self.tip = None;
        } else {
            self.cc = None;
        }
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        if layer == &self.layer {
            self.size = (c.new_size.0.max(1), c.new_size.1.max(1));
            self.configured = true;
            self.dirty = true;
        } else if let Some(t) = self.tip.as_mut().filter(|t| &t.layer == layer) {
            t.configured = true;
        } else if let Some(cc) = &mut self.cc {
            if &cc.panel == layer {
                cc.configured = true;
                self.cc_dirty = true;
            } else if cc.catcher.as_ref().is_some_and(|(l, _)| l == layer) {
                cc.catcher_size = Some((c.new_size.0.max(1), c.new_size.1.max(1)));
                cc.catcher_drawn = false;
            }
        }
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

/// One wheel notch (discrete) or 10 px of touchpad scrolling = one step.
fn scroll_amount(v: &smithay_client_toolkit::seat::pointer::AxisScroll) -> f64 {
    if v.discrete != 0 { v.discrete as f64 } else { (v.absolute / 10.0).trunc() }
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            let (x, y) = e.position;
            if &e.surface == self.layer.wl_surface() {
                match e.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        self.pointer_x = Some(x);
                        self.pointer_moved = true;
                    }
                    PointerEventKind::Leave { .. } => {
                        self.pointer_x = None;
                        self.pointer_moved = true;
                    }
                    PointerEventKind::Press { button, .. } => self.clicks.push((x, button)),
                    PointerEventKind::Axis { ref vertical, .. } => self.scrolls.push((x, scroll_amount(vertical))),
                    _ => {}
                }
            } else if self.is_panel(&e.surface) {
                match e.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        self.panel_pointer = Some((x, y));
                        self.panel_moved = true;
                    }
                    PointerEventKind::Leave { .. } => {
                        self.panel_pointer = None;
                        self.panel_moved = true;
                    }
                    PointerEventKind::Press { button, .. } => self.panel_input.push(PanelInput::Press(x, y, button)),
                    PointerEventKind::Release { .. } => self.panel_input.push(PanelInput::Release),
                    PointerEventKind::Axis { ref vertical, .. } => self.panel_input.push(PanelInput::Scroll(x, y, scroll_amount(vertical))),
                }
            } else if self.is_catcher(&e.surface) && matches!(e.kind, PointerEventKind::Press { .. }) {
                self.catcher_clicked = true;
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
