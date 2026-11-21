// The lock screen (`nothing-bar lock`, process nothing-lock): ext-session-lock, one surface per output, the
// password checked by PAM (service "hyprlock", already installed) on a background thread.
//
// A field of dots that ripples out from the password row on every key, a dot-matrix clock, one dot per typed
// character, a scanning wave while PAM checks, red on a wrong password. It opens with the dot reveal and folds back
// into dots on unlock. If the compositor refuses the lock the process exits 1 (the caller falls back to hyprlock).
// If this process dies while locked, Hyprland keeps the session locked (misc:allow_session_lock_restore lets a
// new locker take over).
//
// NOTHING_PREVIEW=out.png renders a frame without locking. Debug builds only: NOTHING_LOCK_TEST_UNLOCK=<s> unlocks
// by itself after <s> seconds, and NOTHING_KEYS types keys (for testing on a remote box).
use crate::anim::Tween;
use crate::bar::{GREY, RED, WHITE};
use crate::canvas::{Canvas, Fonts, Rgba, Weight};
use crate::fx;
use crate::shell::{self, Key, Mods, DIM};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        Capability, SeatHandler, SeatState,
    },
    session_lock::{SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface, SessionLockSurfaceConfigure},
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

const RIPPLE_S: f32 = 1.4;
const MAX_DOTS: usize = 32;
const MAX_CHARS: usize = 256;
/// Room for MAX_CHARS of any UTF-8, so the password buffer never reallocates (and leaves no copies behind).
const PW_CAP: usize = MAX_CHARS * 4;
/// A PAM check that takes longer than this (a stuck module) gives the prompt back.
const CHECK_TIMEOUT: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    Checking(f32),
    Failed(f32),
    Done(f32),
}

/// Everything the lock screen shows, independent of Wayland (so it can be previewed and tested).
pub struct Scene {
    pw: String,
    /// When each typed character's dot appeared (for its pop).
    born: Vec<f32>,
    /// Ripples: (start, red).
    ripples: Vec<(f32, bool)>,
    state: State,
    caps: bool,
    reveal: Tween,
    user: String,
    /// PAM's answer, tagged with the attempt it belongs to (answers to abandoned attempts are ignored).
    result: Arc<Mutex<Option<(u32, bool)>>>,
    attempt: u32,
    fails: u32,
}

/// Zeroes a string's whole buffer (not only its current length: Backspace leaves bytes behind), then empties it.
fn wipe(s: &mut String) {
    // SAFETY: writes zeros within the allocation; zero bytes are valid UTF-8 and the length is reset right after.
    unsafe {
        let v = s.as_mut_vec();
        std::ptr::write_bytes(v.as_mut_ptr(), 0, v.capacity());
        v.set_len(0);
    }
}

fn wipe_vec(v: &mut Vec<u8>) {
    // SAFETY: zeroes within the allocation, then empties the vector.
    unsafe {
        std::ptr::write_bytes(v.as_mut_ptr(), 0, v.capacity());
        v.set_len(0);
    }
}

impl Scene {
    pub fn new(user: &str, still: bool) -> Self {
        let mut reveal = Tween::new(if still { 1.0 } else { 0.0 });
        reveal.to(1.0, 0.0, if still { 0.0 } else { 0.6 });
        let pw = String::with_capacity(PW_CAP);
        Self { pw, born: Vec::new(), ripples: Vec::new(), state: State::Idle, caps: false, reveal, user: user.to_string(), result: Arc::default(), attempt: 0, fails: 0 }
    }

    fn clear(&mut self) {
        wipe(&mut self.pw);
        self.born.clear();
    }

    /// Handles a key; returns true when PAM should be asked (Enter with a password).
    pub fn key(&mut self, k: &Key, now: f32) -> bool {
        if matches!(self.state, State::Checking(_) | State::Done(_)) {
            return false;
        }
        match k {
            Key::Text(t) => {
                if self.pw.chars().count() < MAX_CHARS && self.pw.len() + t.len() <= PW_CAP {
                    self.pw.push_str(t);
                    self.born.push(now);
                    self.ripples.push((now, false));
                }
            }
            Key::Back => {
                self.pw.pop();
                self.born.pop();
            }
            Key::BackWord | Key::Esc | Key::Ctrl('u') => self.clear(),
            Key::Enter if !self.pw.is_empty() => {
                self.state = State::Checking(now);
                return true;
            }
            _ => {}
        }
        if matches!(self.state, State::Failed(_)) {
            self.state = State::Idle;
        }
        false
    }

    /// Starts PAM on a background thread; `poll` picks up the answer.
    fn check(&mut self) {
        // The buffer itself moves to the PAM thread (no copy); a fresh one takes its place.
        let pw = std::mem::replace(&mut self.pw, String::with_capacity(PW_CAP));
        self.attempt += 1;
        let (user, result, attempt) = (self.user.clone(), self.result.clone(), self.attempt);
        std::thread::spawn(move || {
            let ok = pam::auth("hyprlock", &user, pw);
            *result.lock().unwrap() = Some((attempt, ok));
        });
    }

    /// Picks up PAM's answer. Returns true when it's time to unlock (the fold-away animation has finished).
    pub fn poll(&mut self, now: f32) -> bool {
        let answer = self.result.lock().unwrap().take().filter(|(a, _)| *a == self.attempt).map(|(_, ok)| ok);
        // A check that never answers: give the prompt back (its late answer is then ignored).
        let timed_out = matches!(self.state, State::Checking(t) if now - t > CHECK_TIMEOUT);
        if timed_out {
            self.attempt += 1;
        }
        if let Some(ok) = answer.or(timed_out.then_some(false)) {
            if ok {
                self.state = State::Done(now);
                self.reveal.snap(1.0);
                self.reveal.to(0.0, now, 0.35);
            } else {
                self.state = State::Failed(now);
                self.fails += 1;
                self.ripples.push((now, true));
            }
            self.clear();
        }
        self.ripples.retain(|(t, _)| now - t < RIPPLE_S);
        matches!(self.state, State::Done(t) if now - t >= 0.35)
    }

    pub fn animating(&self, now: f32) -> bool {
        !self.ripples.is_empty()
            || self.reveal.moving(now)
            || matches!(self.state, State::Checking(_) | State::Done(_))
            || matches!(self.state, State::Failed(t) if now - t < 0.6)
            || self.born.last().is_some_and(|t| now - t < 0.2)
    }

    pub fn draw(&self, cv: &mut Canvas, dot_font: &mut Fonts, s: f32, now: f32) {
        let (w, h) = (cv.pix.width() as f32, cv.pix.height() as f32);
        cv.clear(Rgba::hex(0x000000));
        let (cx, row_y) = (w / 2.0, h * 0.64);
        // The dot field and its ripples.
        let pitch = 28.0 * s;
        let (nx, ny) = ((w / pitch) as i32 + 1, (h / pitch) as i32 + 1);
        let (ox, oy) = ((w - (nx - 1) as f32 * pitch) / 2.0, (h - (ny - 1) as f32 * pitch) / 2.0);
        let speed = 900.0 * s;
        let band = 70.0 * s;
        for j in 0..ny {
            for i in 0..nx {
                let (x, y) = (ox + i as f32 * pitch, oy + j as f32 * pitch);
                let d = (x - cx).hypot(y - row_y);
                let mut boost: f32 = 0.0;
                let mut red: f32 = 0.0;
                for &(t0, is_red) in &self.ripples {
                    let age = now - t0;
                    let k = (1.0 - ((d - age * speed).abs() / band)).max(0.0) * (1.0 - age / RIPPLE_S).max(0.0);
                    if is_red {
                        red = red.max(k);
                    }
                    boost = boost.max(k);
                }
                let r = (1.5 + 1.8 * boost) * s;
                let c = Rgba::hex(0x1c1c1c).mix(Rgba::hex(0x6a6a6a), boost).mix(RED, red);
                cv.round_rect(x - r, y - r, 2.0 * r, 2.0 * r, r, c);
            }
        }
        // Clock (dot-matrix font) with the red point, then the date.
        let (hh, mm, _, wd, day, mon) = crate::sys::local_time();
        let time = format!("{hh:02}:{mm:02}");
        let px = 150.0 * s;
        std::mem::swap(&mut cv.fonts, dot_font);
        let tw = cv.fonts.width(Weight::Regular, px, &time);
        // The red point is a real round dot (the font's full stop is square).
        let dot = px * 0.11;
        let base = h * 0.40;
        let pen = cv.text(&time, cx - (tw + dot * 1.6) / 2.0, base, px, Weight::Regular, WHITE, None, 0.0);
        cv.round_rect(pen + dot * 0.6, base - dot, dot, dot, dot / 2.0, RED);
        std::mem::swap(&mut cv.fonts, dot_font);
        const DAYS: [&str; 7] = ["SUNDAY", "MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY", "SATURDAY"];
        const MONTHS: [&str; 12] = ["JANUARY", "FEBRUARY", "MARCH", "APRIL", "MAY", "JUNE", "JULY", "AUGUST", "SEPTEMBER", "OCTOBER", "NOVEMBER", "DECEMBER"];
        let date = format!("{}  {day:02} {}", DAYS[wd as usize % 7], MONTHS[mon as usize % 12]);
        let dpx = 14.0 * s;
        let dwid = cv.fonts.width(Weight::Regular, dpx, &date);
        let db = base + 44.0 * s;
        fx::decode_text(cv, &date, cx - dwid / 2.0, db, dpx, Weight::Regular, GREY, self.reveal.get(now), None, 0.0);

        // The password row: one dot per character (capped), centred.
        let n = self.born.len().min(MAX_DOTS);
        let (dsz, gap) = (11.0 * s, 9.0 * s);
        let row_w = n as f32 * dsz + n.saturating_sub(1) as f32 * gap;
        let x0 = cx - row_w / 2.0;
        let failed = matches!(self.state, State::Failed(t) if now - t < 0.6);
        let msg_b = row_y + 46.0 * s;
        match self.state {
            State::Checking(t0) => {
                // The dots that were typed stay, scanned by a wave left to right.
                let m = self.pw_len_hint().clamp(1, MAX_DOTS);
                let rw = m as f32 * dsz + (m - 1) as f32 * gap;
                for i in 0..m {
                    let ph = ((now - t0) * 3.0 - i as f32 / m as f32).rem_euclid(1.0);
                    let c = WHITE.mix(Rgba::hex(0x3a3a3a), ph);
                    let x = cx - rw / 2.0 + i as f32 * (dsz + gap);
                    cv.round_rect(x, row_y - dsz / 2.0, dsz, dsz, dsz / 2.0, c);
                }
                centred(cv, "CHECKING", cx, msg_b, 10.5 * s, GREY);
            }
            _ => {
                for (i, t) in self.born.iter().rev().take(n).rev().enumerate() {
                    // Pop: starts 60% bigger and settles (ease-out, no bounce).
                    let k = ((now - t) / 0.15).clamp(0.0, 1.0);
                    let grow = 1.0 + 0.6 * (1.0 - k) * (1.0 - k);
                    let d = dsz * grow;
                    let x = x0 + i as f32 * (dsz + gap) + (dsz - d) / 2.0;
                    cv.round_rect(x, row_y - d / 2.0, d, d, d / 2.0, if failed { RED } else { WHITE });
                }
                if n == 0 {
                    let (msg, c) = if matches!(self.state, State::Failed(_)) { ("WRONG PASSWORD", RED) } else { ("TYPE TO UNLOCK", DIM) };
                    let mw = cv.fonts.width(Weight::Regular, 12.0 * s, msg);
                    fx::decode_text(cv, msg, cx - mw / 2.0, row_y + 5.0 * s, 12.0 * s, Weight::Regular, c, self.reveal.get(now), None, 0.0);
                }
            }
        }
        if self.caps {
            centred(cv, "CAPS LOCK", cx, msg_b + 20.0 * s, 10.5 * s, RED);
        }
        // Bottom: who is locked, and the keys.
        let ub = h - 48.0 * s;
        centred(cv, &self.user.to_uppercase(), cx, ub, 11.0 * s, GREY);
        let hint_w = 260.0 * s;
        shell::hints(cv, &[("ENTER", "unlock"), ("ESC", "clear")], cx - hint_w / 2.0 + 40.0 * s, ub + 22.0 * s, s, 1.0);

        // Opening and unlocking: the dot screen over everything (black, from the centre).
        let p = self.reveal.get(now);
        fx::dot_screen(cv, 0.0, 0.0, w, h, 24.0 * s, 0.0, p, (0.5, 0.64), Rgba::hex(0x000000));
    }

    /// How many dots to scan while checking (the password itself has moved to the PAM thread).
    fn pw_len_hint(&self) -> usize {
        self.born.len()
    }
}

fn centred(cv: &mut Canvas, t: &str, cx: f32, base: f32, px: f32, c: Rgba) {
    let w = cv.fonts.width(Weight::Regular, px, t);
    cv.text(t, cx - w / 2.0, base, px, Weight::Regular, c, None, 0.0);
}

fn user_name() -> String {
    // SAFETY: getpwuid returns a pointer into static storage or null; the name is copied out at once.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() {
            return std::env::var("USER").unwrap_or_default();
        }
        std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned()
    }
}

/// PAM through libpam directly (no extra crate): one conversation that answers every prompt with the password.
pub mod pam {
    use libc::{c_char, c_int, c_void};
    use std::ffi::CString;

    #[repr(C)]
    struct Message {
        style: c_int,
        msg: *const c_char,
    }
    #[repr(C)]
    struct Response {
        resp: *mut c_char,
        retcode: c_int,
    }
    #[repr(C)]
    struct Conv {
        conv: extern "C" fn(c_int, *mut *const Message, *mut *mut Response, *mut c_void) -> c_int,
        data: *mut c_void,
    }

    #[link(name = "pam")]
    unsafe extern "C" {
        fn pam_start(service: *const c_char, user: *const c_char, conv: *const Conv, h: *mut *mut c_void) -> c_int;
        fn pam_authenticate(h: *mut c_void, flags: c_int) -> c_int;
        fn pam_setcred(h: *mut c_void, flags: c_int) -> c_int;
        fn pam_acct_mgmt(h: *mut c_void, flags: c_int) -> c_int;
        fn pam_end(h: *mut c_void, status: c_int) -> c_int;
    }

    const PAM_SUCCESS: c_int = 0;
    const PAM_BUF_ERR: c_int = 5;
    const PAM_PROMPT_ECHO_OFF: c_int = 1;
    const PAM_REFRESH_CRED: c_int = 0x10;

    extern "C" fn conv(n: c_int, msgs: *mut *const Message, resp: *mut *mut Response, data: *mut c_void) -> c_int {
        if n <= 0 || msgs.is_null() || resp.is_null() {
            return PAM_BUF_ERR;
        }
        // SAFETY: PAM hands us n messages (Linux-PAM: an array of pointers) and frees the replies itself, so they
        // are allocated with calloc/strdup. `data` is the NUL-terminated password owned by `auth`.
        unsafe {
            let replies = libc::calloc(n as usize, std::mem::size_of::<Response>()) as *mut Response;
            if replies.is_null() {
                return PAM_BUF_ERR;
            }
            for i in 0..n as usize {
                let m = *msgs.add(i);
                // Only hidden prompts get the password (never a visible one such as a user name or a code).
                if !m.is_null() && (*m).style == PAM_PROMPT_ECHO_OFF {
                    (*replies.add(i)).resp = libc::strdup(data as *const c_char);
                }
            }
            *resp = replies;
        }
        PAM_SUCCESS
    }

    /// True if `password` is right for `user` and the account may log in. The password is wiped from memory
    /// before returning.
    pub fn auth(service: &str, user: &str, password: String) -> bool {
        let mut bytes = password.into_bytes();
        let ok = (|| {
            let s = CString::new(service).ok()?;
            let u = CString::new(user).ok()?;
            let mut p = Vec::with_capacity(bytes.len() + 1);
            p.extend_from_slice(&bytes);
            p.push(0);
            let c = Conv { conv, data: p.as_mut_ptr() as *mut c_void };
            let mut h: *mut c_void = std::ptr::null_mut();
            // SAFETY: valid NUL-terminated strings and a conversation that outlives the handle.
            let r = unsafe {
                if pam_start(s.as_ptr(), u.as_ptr(), &c, &mut h) != PAM_SUCCESS {
                    super::wipe_vec(&mut p);
                    return None;
                }
                let mut r = pam_authenticate(h, 0);
                if r == PAM_SUCCESS {
                    // Expired or locked accounts must not unlock either.
                    r = pam_acct_mgmt(h, 0);
                }
                if r == PAM_SUCCESS {
                    pam_setcred(h, PAM_REFRESH_CRED);
                }
                pam_end(h, r);
                r
            };
            super::wipe_vec(&mut p);
            Some(r == PAM_SUCCESS)
        })()
        .unwrap_or(false);
        super::wipe_vec(&mut bytes);
        ok
    }
}

struct Surf {
    surface: SessionLockSurface,
    output: wl_output::WlOutput,
    size: (u32, u32),
    scale: i32,
    configured: bool,
    frame_pending: bool,
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    shm: Shm,
    lock: Option<SessionLock>,
    surfaces: Vec<Surf>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    mods: Mods,
    keys: Vec<Key>,
    held: Option<(u32, Key, Instant)>,
    locked: bool,
    refused: bool,
}

impl App {
    fn add_surface(&mut self, output: wl_output::WlOutput, qh: &QueueHandle<Self>) {
        if let Some(lock) = &self.lock
            && !self.surfaces.iter().any(|s| s.output == output)
        {
            let surface = lock.create_lock_surface(self.compositor.create_surface(qh), &output, qh);
            self.surfaces.push(Surf { surface, output, size: (0, 0), scale: 1, configured: false, frame_pending: false });
        }
    }
}

pub fn run() -> i32 {
    let cfg = crate::config::Watcher::new().poll().unwrap_or_default();
    let (Ok(fonts), Ok(mut dot_font)) = (Fonts::load(&cfg.font), Fonts::load("Doto").or_else(|_| Fonts::load(&cfg.font))) else {
        eprintln!("nothing-lock: fonts missing");
        return 1;
    };
    let mut cv = Canvas::new(1, 1, fonts);
    let mut scene = Scene::new(&user_name(), shell::turbo());
    if let Ok(path) = std::env::var("NOTHING_PREVIEW") {
        cv.resize(1920, 1080);
        for (i, c) in "secret".chars().enumerate() {
            scene.key(&Key::Text(c.to_string()), 9.0 + i as f32 * 0.1);
        }
        scene.draw(&mut cv, &mut dot_font, 1.0, 9.8);
        return match cv.pix.save_png(&path) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("preview: {e}");
                1
            }
        };
    }
    // Exit 0 only when another nothing-lock already holds the lock; any doubt exits 1 so hyprlock takes over.
    let _claim = match shell::claim("nothing-lock", false) {
        shell::Claim::Mine(f) => f,
        shell::Claim::Taken => return 0,
        shell::Claim::Failed => return 1,
    };
    crate::sys::install_signals();
    let Ok(conn) = Connection::connect_to_env() else { return 1 };
    let Ok((globals, mut queue)) = registry_queue_init(&conn) else { return 1 };
    let qh = queue.handle();
    let (Ok(compositor), Ok(shm)) = (CompositorState::bind(&globals, &qh), Shm::bind(&globals, &qh)) else { return 1 };
    let lock_state = SessionLockState::new(&globals, &qh);
    let Ok(lock) = lock_state.lock(&qh) else {
        eprintln!("nothing-lock: the compositor has no ext-session-lock");
        return 1;
    };
    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        shm,
        lock: Some(lock),
        surfaces: Vec::new(),
        keyboard: None,
        mods: Mods::default(),
        keys: Vec::new(),
        held: None,
        locked: false,
        refused: false,
    };
    // Outputs arrive with the first roundtrip.
    if queue.roundtrip(&mut app).is_err() {
        return 1;
    }
    for o in app.output_state.outputs().collect::<Vec<_>>() {
        app.add_surface(o, &qh);
    }
    let Ok(mut pool) = SlotPool::new(1920 * 1080 * 4, &app.shm) else { return 1 };
    #[cfg(debug_assertions)]
    let test_unlock: Option<f32> = std::env::var("NOTHING_LOCK_TEST_UNLOCK").ok().and_then(|v| v.parse().ok());
    #[cfg(debug_assertions)]
    let mut script = std::env::var("NOTHING_KEYS").ok().map(|s| shell::parse_script(&s)).unwrap_or_default();

    let t0 = Instant::now();
    let mut last_draw = Instant::now() - Duration::from_secs(5);
    loop {
        let now = t0.elapsed().as_secs_f32();
        if app.refused {
            eprintln!("nothing-lock: the compositor refused the lock");
            return 1;
        }
        #[cfg(debug_assertions)]
        {
            if app.locked && now > 1.0 && !script.is_empty() {
                app.keys.push(script.remove(0));
            }
            if test_unlock.is_some_and(|t| now > t) && !matches!(scene.state, State::Done(_)) {
                *scene.result.lock().unwrap() = Some((scene.attempt, true));
            }
        }
        if let Some((_, k, at)) = &mut app.held
            && Instant::now() >= *at
        {
            *at = Instant::now() + Duration::from_millis(25);
            app.keys.push(k.clone());
        }
        scene.caps = app.mods.caps;
        for k in std::mem::take(&mut app.keys) {
            if scene.key(&k, now) {
                app.held = None;
                scene.check();
            }
        }
        if scene.poll(now) {
            if let Some(l) = app.lock.take() {
                l.unlock();
            }
            let _ = conn.roundtrip();
            return 0;
        }
        // Draw every surface that is ready: every frame while animating, otherwise once a second (the clock).
        let animating = scene.animating(now);
        if animating || last_draw.elapsed() >= Duration::from_millis(1000) {
            let mut drew = false;
            for sf in app.surfaces.iter_mut().filter(|s| s.configured && !s.frame_pending) {
                let sc = sf.scale.max(1);
                cv.resize(sf.size.0 * sc as u32, sf.size.1 * sc as u32);
                let s = sc as f32 * cfg.size.max(1.0) * (sf.size.1 as f32 / 1080.0).clamp(0.6, 2.0);
                scene.draw(&mut cv, &mut dot_font, s, now);
                let surface = sf.surface.wl_surface();
                if animating {
                    surface.frame(&qh, FrameCallbackData(surface.clone()));
                    sf.frame_pending = true;
                }
                let (bw, bh) = (cv.pix.width() as i32, cv.pix.height() as i32);
                if let Ok((buffer, bytes)) = pool.create_buffer(bw, bh, bw * 4, wl_shm::Format::Argb8888) {
                    shell::to_argb(&cv, bytes);
                    surface.set_buffer_scale(sc);
                    surface.damage_buffer(0, 0, bw, bh);
                    if buffer.attach_to(surface).is_ok() {
                        surface.commit();
                    }
                }
                drew = true;
            }
            if drew {
                last_draw = Instant::now();
            }
        }
        let mut timeout = if animating { Duration::from_millis(8) } else { Duration::from_millis(250) };
        if let Some((.., at)) = &app.held {
            timeout = timeout.min(at.saturating_duration_since(Instant::now()));
        }
        if shell::wait(&conn, &mut queue, &mut app, timeout).is_err() {
            return 1;
        }
    }
}

impl SessionLockHandler for App {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        self.locked = true;
    }
    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        // Refused (another locker is active), or the compositor ended our lock.
        if !self.locked {
            self.refused = true;
        }
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: SessionLockSurface, c: SessionLockSurfaceConfigure, _: u32) {
        if let Some(s) = self.surfaces.iter_mut().find(|s| s.surface.wl_surface() == surface.wl_surface()) {
            s.size = (c.new_size.0.max(1), c.new_size.1.max(1));
            s.configured = true;
            s.frame_pending = false;
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, factor: i32) {
        if let Some(s) = self.surfaces.iter_mut().find(|s| s.surface.wl_surface() == surface) {
            s.scale = factor;
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(s) = self.surfaces.iter_mut().find(|s| s.surface.wl_surface() == surface) {
            s.frame_pending = false;
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        // A monitor plugged in while locked gets its own lock surface.
        self.add_surface(output, qh);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.surfaces.retain(|s| s.output != output);
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
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
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
        if let Some(k) = shell::key_of(&ev, self.mods) {
            // Only Backspace repeats here: a held letter would type the password many times over.
            self.held = (k == Key::Back).then(|| (ev.raw_code, k.clone(), Instant::now() + Duration::from_millis(250)));
            self.keys.push(k);
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

    #[test]
    fn typing_checking_and_failing() {
        let mut s = Scene::new("me", true);
        for c in ["a", "b"] {
            assert!(!s.key(&Key::Text(c.into()), 0.0));
        }
        s.key(&Key::Back, 0.1);
        assert_eq!((s.pw.as_str(), s.born.len()), ("a", 1));
        assert!(s.key(&Key::Enter, 0.2));
        assert!(!s.key(&Key::Text("x".into()), 0.3)); // ignored while checking
        *s.result.lock().unwrap() = Some((s.attempt, false));
        assert!(!s.poll(0.4));
        assert!(matches!(s.state, State::Failed(_)) && s.pw.is_empty() && s.fails == 1);
        s.key(&Key::Text("z".into()), 0.5);
        *s.result.lock().unwrap() = Some((s.attempt + 7, true)); // an old attempt's answer: ignored
        assert!(!s.poll(0.6) && !matches!(s.state, State::Done(_)));
        *s.result.lock().unwrap() = Some((s.attempt, true));
        assert!(!s.poll(1.0)); // folding away first
        assert!(s.poll(1.4));
        assert!(!s.key(&Key::Enter, 0.0)); // nothing to check
    }

    #[test]
    fn pam_rejects_a_wrong_password() {
        // Talks to the real PAM stack ("hyprlock" -> login); a wrong password must never pass.
        assert!(!pam::auth("hyprlock", &user_name(), "definitely-not-the-password-∅".to_string()));
    }
}
