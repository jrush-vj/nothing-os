// The control centre: a one-column, phone-style panel under the right end of the bar (frosted, 22 px corners,
// soft shadow). Opens with a 180 ms drop + fade, closes with a quick fade. Pages: main, bar settings, Wi-Fi,
// Bluetooth. Layout and drawing happen together; `hits` records the clickable rectangles.
use crate::anim::Tween;
use crate::bar::{GREY, HOVER, RED, WHITE};
use crate::canvas::{Canvas, Rgba, Weight};
use crate::cc::{Graphs, Series, State};
use crate::config::Config;
use crate::icons::{self, Icon};
use crate::media::Track;
use crate::notif::Note;

/// Panel width and the transparent margin around it (room for the shadow), logical px.
pub const W: f32 = 320.0;
pub const MARGIN: f32 = 18.0;
/// The surface is a fixed size; content is drawn from the top.
pub const SURFACE_W: u32 = (W + 2.0 * MARGIN) as u32;
pub const SURFACE_H: u32 = 640;
const PAD: f32 = 14.0;
const RADIUS: f32 = 22.0;
const TILE_BG: Rgba = Rgba(1.0, 1.0, 1.0, 0.07);
const BLACK: Rgba = Rgba(0.0, 0.0, 0.0, 1.0);
pub const MAX_NOTES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Page {
    Main,
    Settings,
    Wifi,
    Bluetooth,
    Notifications,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Session {
    Lock,
    Logout,
    Reboot,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slider {
    Volume,
    Brightness,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PHit {
    Wifi,
    Bt,
    Mic,
    Turbo,
    Profile(usize),
    Networks,
    Devices,
    Slider(Slider),
    MediaPrev,
    MediaToggle,
    MediaNext,
    Session(Session),
    Settings,
    Back,
    Setting(usize),
    Net(usize),
    Dev(usize),
    ClearAll,
    Note(usize),
}

pub const PROFILES: [(&str, &str); 3] = [("SAVER", "power-saver"), ("BALANCED", "balanced"), ("PERF", "performance")];

pub struct Panel {
    pub page: Page,
    open: Tween,
    now: f32,
    /// Pointer in buffer pixels while over the panel.
    pub pointer: Option<(f32, f32)>,
    hits: Vec<([f32; 4], PHit)>,
    /// A session button clicked once (it needs a second click within 3 s).
    pub armed: Option<(Session, f32)>,
    pub drag: Option<Slider>,
    pub state: State,
    pub turbo: bool,
    pub vol: Option<(f32, bool)>,
    pub bright: Option<f32>,
    pub track: Option<Track>,
    pub graphs: Graphs,
    pub notes: Vec<Note>,
    pub cfg: Config,
}

impl Panel {
    pub fn new(cfg: Config) -> Self {
        Self {
            page: Page::Main,
            open: Tween::new(0.0),
            now: 0.0,
            pointer: None,
            hits: Vec::new(),
            armed: None,
            drag: None,
            state: State::default(),
            turbo: false,
            vol: None,
            bright: None,
            track: None,
            graphs: Graphs::default(),
            notes: Vec::new(),
            cfg,
        }
    }

    pub fn show(&mut self, still: bool) {
        self.page = Page::Main;
        self.armed = None;
        self.open.to(1.0, self.now, if still { 0.0 } else { 0.18 });
    }

    pub fn hide(&mut self, still: bool) {
        self.drag = None;
        self.open.to(0.0, self.now, if still { 0.0 } else { 0.1 });
    }

    pub fn is_open(&self) -> bool {
        self.open.target() > 0.0
    }

    /// Fully faded out: the caller can destroy the surface.
    pub fn gone(&self) -> bool {
        self.open.target() == 0.0 && !self.open.moving(self.now)
    }

    /// Advances time; true while the open/close animation runs.
    pub fn tick(&mut self, dt: f32) -> bool {
        self.now += dt;
        if self.armed.is_some_and(|(_, t)| self.now - t > 3.0) {
            self.armed = None;
        }
        self.open.moving(self.now)
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<PHit> {
        self.hits.iter().find(|(r, _)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]).map(|h| h.1)
    }

    /// 0..1 along a slider's track for pointer x.
    pub fn slider_value(&self, which: Slider, x: f32) -> Option<f32> {
        let (r, _) = self.hits.iter().find(|(_, h)| *h == PHit::Slider(which))?;
        Some(((x - r[0]) / r[2]).clamp(0.0, 1.0))
    }

    /// Arms a session button, or returns it if it was already armed (second click).
    pub fn press_session(&mut self, s: Session) -> Option<Session> {
        if self.armed.is_some_and(|(a, _)| a == s) {
            self.armed = None;
            Some(s)
        } else {
            self.armed = Some((s, self.now));
            None
        }
    }

    pub fn draw(&mut self, cv: &mut Canvas, s: f32) {
        cv.clear(Rgba(0.0, 0.0, 0.0, 0.0));
        self.hits.clear();
        let t = self.open.get(self.now);
        if t <= 0.0 {
            return;
        }
        let (x0, y0) = (MARGIN * s, MARGIN * s - (1.0 - t) * 10.0 * s);
        let w = W * s;
        let mut hits = std::mem::take(&mut self.hits);
        let mut d = Draw { cv, s, x0, y: y0 + PAD * s, w, a: t, hits: &mut hits, pointer: self.pointer, now: self.now };
        // Content first into a measuring pass would be cleaner; the height is known per page instead.
        let h = match self.page {
            Page::Main => main_height(self.track.is_some(), self.graphs.dgpu_awake) * s,
            Page::Settings => (PAD * 2.0 + 30.0 + 5.0 * 32.0) * s,
            Page::Wifi => (PAD * 2.0 + 30.0 + self.state.networks.len().max(1) as f32 * 30.0) * s,
            Page::Bluetooth => (PAD * 2.0 + 30.0 + self.state.devices.len().max(1) as f32 * 30.0) * s,
            Page::Notifications => (PAD * 2.0 + 30.0 + self.notes.len().clamp(1, MAX_NOTES) as f32 * 30.0) * s,
        };
        // Soft shadow: a few widening, faint rings below the panel.
        for i in 0..6 {
            let g = (i as f32 + 1.0) * 2.5 * s;
            d.cv.round_rect(x0 - g, y0 - g + 6.0 * s, w + 2.0 * g, h + 2.0 * g, (RADIUS * s) + g, Rgba(0.0, 0.0, 0.0, 0.05 * t));
        }
        d.cv.round_rect(x0, y0, w, h, RADIUS * s, Rgba::hex(0x0d0d0d).alpha(0.62 * t));
        d.cv.stroke_round_rect(x0 + 0.5, y0 + 0.5, w - 1.0, h - 1.0, RADIUS * s, 1.0, Rgba(1.0, 1.0, 1.0, 0.1 * t));
        match self.page {
            Page::Main => self.draw_main(&mut d),
            Page::Settings => {
                d.header("BAR SETTINGS");
                for (i, (label, value)) in self.cfg.rows().into_iter().enumerate() {
                    d.row(label, &format!("{value}  ›"), PHit::Setting(i), WHITE);
                }
            }
            Page::Wifi => {
                d.header("WI-FI");
                if self.state.networks.is_empty() {
                    d.row(if self.state.wifi_on { "SCANNING…" } else { "WI-FI IS OFF" }, "", PHit::Back, GREY);
                }
                for (i, n) in self.state.networks.iter().enumerate() {
                    let v = if n.active { "CONNECTED".to_string() } else { format!("{}%{}", n.signal, if n.secured { " ·" } else { "" }) };
                    d.row(&n.ssid, &v, PHit::Net(i), if n.active { WHITE } else { GREY });
                }
            }
            Page::Bluetooth => {
                d.header("BLUETOOTH");
                if self.state.devices.is_empty() {
                    d.row(if self.state.bt_on { "NO PAIRED DEVICES" } else { "BLUETOOTH IS OFF" }, "", PHit::Back, GREY);
                }
                for (i, dev) in self.state.devices.iter().enumerate() {
                    d.row(&dev.name, if dev.connected { "CONNECTED" } else { "PAIRED" }, PHit::Dev(i), if dev.connected { WHITE } else { GREY });
                }
            }
            Page::Notifications => {
                d.title("NOTIFICATIONS", if self.notes.is_empty() { "" } else { "CLEAR ALL" }, PHit::ClearAll);
                if self.notes.is_empty() {
                    d.row("NOTHING NEW", "", PHit::Back, GREY);
                }
                for (i, n) in self.notes.iter().take(MAX_NOTES).enumerate() {
                    d.row(&format!("{} · {}", crate::bar::pretty_class(&n.app), n.summary), "", PHit::Note(i), WHITE);
                }
            }
        }
        self.hits = hits;
    }

    fn draw_main(&self, d: &mut Draw) {
        let st = &self.state;
        d.tiles(&[
            (Icon::Wifi, "WI-FI", st.wifi_on, PHit::Wifi),
            (Icon::Bt, "BT", st.bt_on, PHit::Bt),
            (if st.mic_muted { Icon::MicOff } else { Icon::Mic }, "MIC", !st.mic_muted, PHit::Mic),
            (Icon::Bolt, "TURBO", self.turbo, PHit::Turbo),
        ]);
        let active = st.networks.iter().find(|n| n.active).map(|n| n.ssid.clone()).unwrap_or_else(|| if st.wifi_on { "—".into() } else { "OFF".into() });
        d.row("NETWORKS", &format!("{active}  ›"), PHit::Networks, WHITE);
        let connected = st.devices.iter().filter(|x| x.connected).count();
        d.row("DEVICES", &format!("{}  ›", if st.bt_on { format!("{connected} CONNECTED") } else { "OFF".into() }), PHit::Devices, WHITE);
        d.gap(4.0);
        let cur = PROFILES.iter().position(|(_, id)| *id == st.profile);
        d.segments(&PROFILES.map(|(l, _)| l), cur);
        let gpu = if self.graphs.dgpu_awake {
            self.graphs.dgpu.last().map_or("AWAKE".to_string(), |w| format!("AWAKE · {w:.0} W"))
        } else {
            "ASLEEP".to_string()
        };
        d.info(Icon::Gpu, "RTX 4060", &gpu, if self.graphs.dgpu_awake { RED } else { GREY });
        d.gap(4.0);
        let (vol, muted) = self.vol.unwrap_or((0.0, false));
        d.slider(if muted { Icon::Mute } else { Icon::Vol }, "VOLUME", vol / 100.0, Slider::Volume);
        if let Some(b) = self.bright {
            d.slider(Icon::Sun, "BRIGHTNESS", b / 100.0, Slider::Brightness);
        }
        if let Some(track) = &self.track {
            d.media(track);
        }
        d.gap(4.0);
        let g = &self.graphs;
        d.graph("CPU", &g.cpu, Some(100.0), g.cpu.last().map(|v| format!("{v:.0}%")));
        d.graph("RAM", &g.ram, None, g.ram.last().map(|v| format!("{v:.1}G")));
        d.graph("TEMP", &g.temp, Some(100.0), g.temp.last().map(|v| format!("{v:.0}°")));
        d.graph("IGPU", &g.igpu, Some(100.0), g.igpu.last().map(|v| format!("{v:.0}%")));
        if g.dgpu_awake {
            d.graph("4060", &g.dgpu, None, g.dgpu.last().map(|v| format!("{v:.0} W")));
        }
        let up = g.tx.last().map(|v| crate::status::fmt_rate(v as f64).replace('↓', "↑")).unwrap_or_default();
        d.graph("NET", &g.rx, None, g.rx.last().map(|v| format!("{} {up}", crate::status::fmt_rate(v as f64))));
        let (used, total) = g.disk;
        d.meter("DISK", if total > 0.0 { (used / total) as f32 } else { 0.0 }, &format!("{:.0}/{:.0}G", used / 1e9, total / 1e9));
        d.gap(4.0);
        let armed = self.armed.map(|(a, _)| a);
        d.session(armed);
        d.row("BAR SETTINGS", "›", PHit::Settings, WHITE);
    }
}

/// Main page height for the background card (must match draw_main).
fn main_height(media: bool, dgpu: bool) -> f32 {
    let rows = 2.0 * 30.0;
    let graphs = if dgpu { 6.0 } else { 5.0 } * 22.0 + 22.0; // + the disk meter
    PAD * 2.0 + 58.0 + rows + 4.0 + 38.0 + 30.0 + 4.0 + 2.0 * 34.0 + if media { 52.0 } else { 0.0 } + 4.0 + graphs + 4.0 + 42.0 + 30.0
}

/// Layout cursor: draws items top to bottom and records their hit rectangles.
struct Draw<'a> {
    cv: &'a mut Canvas,
    s: f32,
    x0: f32,
    y: f32,
    w: f32,
    a: f32,
    hits: &'a mut Vec<([f32; 4], PHit)>,
    pointer: Option<(f32, f32)>,
    now: f32,
}

impl Draw<'_> {
    fn inner(&self) -> (f32, f32) {
        (self.x0 + PAD * self.s, self.w - 2.0 * PAD * self.s)
    }

    fn hovered(&self, r: [f32; 4]) -> bool {
        self.pointer.is_some_and(|(x, y)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3])
    }

    fn gap(&mut self, px: f32) {
        self.y += px * self.s;
    }

    fn text(&mut self, t: &str, x: f32, base: f32, px: f32, w: Weight, c: Rgba) -> f32 {
        self.cv.text(t, x, base, px * self.s, w, c.alpha(self.a), None, 0.0)
    }

    fn text_right(&mut self, t: &str, right: f32, base: f32, px: f32, c: Rgba) {
        let tw = self.cv.fonts.width(Weight::Regular, px * self.s, t);
        self.text(t, right - tw, base, px, Weight::Regular, c);
    }

    fn baseline(&self, top: f32, h: f32, px: f32) -> f32 {
        (top + (h + self.cv.fonts.cap_height(Weight::Regular, px * self.s)) / 2.0).round()
    }

    fn header(&mut self, title: &str) {
        let (x, iw) = self.inner();
        let h = 30.0 * self.s;
        let r = [x - 6.0 * self.s, self.y, iw + 12.0 * self.s, h];
        if self.hovered(r) {
            self.cv.round_rect(r[0], r[1], r[2], r[3], 10.0 * self.s, HOVER.alpha(self.a));
        }
        let b = self.baseline(self.y, h, 12.0);
        let nx = self.text("‹  ", x, b, 12.0, Weight::Bold, WHITE);
        self.text(title, nx, b, 12.0, Weight::Bold, WHITE);
        self.hits.push((r, PHit::Back));
        self.y += h;
    }

    /// A page title with an optional action on the right (e.g. CLEAR ALL).
    fn title(&mut self, title: &str, action: &str, hit: PHit) {
        let (x, iw) = self.inner();
        let h = 30.0 * self.s;
        let b = self.baseline(self.y, h, 12.0);
        self.text(title, x, b, 12.0, Weight::Bold, WHITE);
        if !action.is_empty() {
            let aw = self.cv.fonts.width(Weight::Bold, 10.0 * self.s, action);
            let r = [x + iw - aw - 8.0 * self.s, self.y + 4.0 * self.s, aw + 16.0 * self.s, h - 8.0 * self.s];
            if self.hovered(r) {
                self.cv.round_rect(r[0], r[1], r[2], r[3], 8.0 * self.s, HOVER.alpha(self.a));
            }
            self.text(action, x + iw - aw, self.baseline(self.y, h, 10.0), 10.0, Weight::Bold, RED);
            self.hits.push((r, hit));
        }
        self.y += h;
    }

    fn row(&mut self, label: &str, value: &str, hit: PHit, label_c: Rgba) {
        let (x, iw) = self.inner();
        let h = 30.0 * self.s;
        let r = [x - 6.0 * self.s, self.y, iw + 12.0 * self.s, h];
        if self.hovered(r) {
            self.cv.round_rect(r[0], r[1] + 2.0 * self.s, r[2], r[3] - 4.0 * self.s, 10.0 * self.s, HOVER.alpha(self.a));
        }
        let b = self.baseline(self.y, h, 11.0);
        // Long names (SSIDs, devices) fade out before the value.
        let vw = self.cv.fonts.width(Weight::Regular, 11.0 * self.s, value);
        let clip = (x + iw - vw - 12.0 * self.s).max(x + 40.0 * self.s);
        self.cv.text(label, x, b, 11.0 * self.s, Weight::Regular, label_c.alpha(self.a), Some((0.0, clip)), 16.0 * self.s);
        self.text_right(value, x + iw, b, 11.0, GREY);
        self.hits.push((r, hit));
        self.y += h;
    }

    fn tiles(&mut self, tiles: &[(Icon, &str, bool, PHit)]) {
        let (x, iw) = self.inner();
        let s = self.s;
        let gap = 8.0 * s;
        let tw = (iw - gap * (tiles.len() as f32 - 1.0)) / tiles.len() as f32;
        let th = 54.0 * s;
        for (i, (icon, label, on, hit)) in tiles.iter().enumerate() {
            let tx = x + i as f32 * (tw + gap);
            let r = [tx, self.y, tw, th];
            let bg = if *on { WHITE } else if self.hovered(r) { Rgba(1.0, 1.0, 1.0, 0.12) } else { TILE_BG };
            self.cv.round_rect(tx, self.y, tw, th, 14.0 * s, bg.alpha(self.a));
            let fg = if *on { BLACK } else { WHITE };
            let isz = 16.0 * s;
            icons::draw(self.cv, *icon, (tx + (tw - isz) / 2.0).round(), self.y + 10.0 * s, isz, fg.alpha(self.a));
            let lw = self.cv.fonts.width(Weight::Bold, 9.5 * s, label);
            self.text(label, tx + (tw - lw) / 2.0, self.y + 44.0 * s, 9.5, Weight::Bold, fg);
            self.hits.push((r, *hit));
        }
        self.y += th + 4.0 * s;
    }

    fn segments(&mut self, labels: &[&str], active: Option<usize>) {
        let (x, iw) = self.inner();
        let s = self.s;
        let h = 30.0 * s;
        self.cv.round_rect(x, self.y, iw, h, 12.0 * s, TILE_BG.alpha(self.a));
        let sw = (iw - 6.0 * s) / labels.len() as f32;
        for (i, l) in labels.iter().enumerate() {
            let sx = x + 3.0 * s + i as f32 * sw;
            let r = [sx, self.y + 3.0 * s, sw, h - 6.0 * s];
            let on = active == Some(i);
            if on {
                self.cv.round_rect(r[0], r[1], r[2], r[3], 9.0 * s, WHITE.alpha(self.a));
            } else if self.hovered(r) {
                self.cv.round_rect(r[0], r[1], r[2], r[3], 9.0 * s, HOVER.alpha(self.a));
            }
            let lw = self.cv.fonts.width(Weight::Bold, 10.0 * s, l);
            let b = self.baseline(self.y, h, 10.0);
            self.text(l, sx + (sw - lw) / 2.0, b, 10.0, Weight::Bold, if on { BLACK } else { WHITE });
            self.hits.push((r, PHit::Profile(i)));
        }
        self.y += h + 8.0 * s;
    }

    fn info(&mut self, icon: Icon, label: &str, value: &str, value_c: Rgba) {
        let (x, iw) = self.inner();
        let h = 30.0 * self.s;
        let isz = 13.0 * self.s;
        icons::draw(self.cv, icon, x, self.y + (h - isz) / 2.0, isz, WHITE.alpha(self.a));
        let b = self.baseline(self.y, h, 11.0);
        self.text(label, x + isz + 7.0 * self.s, b, 11.0, Weight::Bold, WHITE);
        self.text_right(value, x + iw, b, 11.0, value_c);
        self.y += h;
    }

    fn slider(&mut self, icon: Icon, label: &str, v: f32, which: Slider) {
        let (x, iw) = self.inner();
        let s = self.s;
        let isz = 12.0 * s;
        icons::draw(self.cv, icon, x, self.y + 2.0 * s, isz, WHITE.alpha(self.a));
        let b = self.y + 12.0 * s;
        self.text(label, x + isz + 7.0 * s, b, 10.0, Weight::Regular, GREY);
        self.text_right(&format!("{:.0}", v * 100.0), x + iw, b, 10.0, WHITE);
        let ty = self.y + 20.0 * s;
        let th = 6.0 * s;
        self.cv.round_rect(x, ty, iw, th, 3.0 * s, TILE_BG.alpha(self.a * 1.6));
        self.cv.round_rect(x, ty, (iw * v.clamp(0.0, 1.0)).max(th), th, 3.0 * s, WHITE.alpha(self.a));
        // The hit area is taller than the track, for easy grabbing.
        self.hits.push(([x, ty - 8.0 * s, iw, th + 16.0 * s], PHit::Slider(which)));
        self.y += 34.0 * s;
    }

    fn media(&mut self, t: &Track) {
        let (x, iw) = self.inner();
        let s = self.s;
        let h = 44.0 * s;
        self.cv.round_rect(x, self.y, h, h, 10.0 * s, Rgba::hex(0x3a0a0a).alpha(self.a));
        icons::draw(self.cv, if t.playing { Icon::Play } else { Icon::Pause }, x + 14.0 * s, self.y + 14.0 * s, 16.0 * s, RED.alpha(self.a));
        let tx = x + h + 10.0 * s;
        let ctrl_w = 3.0 * 22.0 * s;
        let clip = Some((0.0, x + iw - ctrl_w - 8.0 * s));
        self.cv.text(&t.title, tx, self.y + 19.0 * s, 11.0 * s, Weight::Bold, WHITE.alpha(self.a), clip, 16.0 * s);
        self.cv.text(&t.artist, tx, self.y + 35.0 * s, 10.0 * s, Weight::Regular, GREY.alpha(self.a), clip, 16.0 * s);
        let mut cx = x + iw - ctrl_w;
        for (icon, hit) in [(Icon::Prev, PHit::MediaPrev), (if t.playing { Icon::Pause } else { Icon::Play }, PHit::MediaToggle), (Icon::Next, PHit::MediaNext)] {
            let r = [cx, self.y + 8.0 * s, 22.0 * s, 28.0 * s];
            if self.hovered(r) {
                self.cv.round_rect(r[0], r[1], r[2], r[3], 8.0 * s, HOVER.alpha(self.a));
            }
            icons::draw(self.cv, icon, cx + 5.0 * s, self.y + 15.0 * s, 12.0 * s, WHITE.alpha(self.a));
            self.hits.push((r, hit));
            cx += 22.0 * s;
        }
        if let Some(p) = t.progress() {
            self.cv.rect(tx, self.y + h - 3.0 * s, (x + iw - tx) * p, 2.0 * s, RED.alpha(self.a));
        }
        self.y += h + 8.0 * s;
    }

    /// Label, sparkline of the series, last value. `max` fixes the scale (percentages); None auto-scales.
    fn graph(&mut self, label: &str, series: &Series, max: Option<f32>, value: Option<String>) {
        let (x, iw) = self.inner();
        let s = self.s;
        let h = 22.0 * s;
        let b = self.baseline(self.y, h, 10.0);
        self.text(label, x, b, 10.0, Weight::Regular, GREY);
        self.text_right(value.as_deref().unwrap_or("—"), x + iw, b, 10.0, WHITE);
        let (gx, gw) = (x + 46.0 * s, iw - 46.0 * s - 70.0 * s);
        let (gy, gh) = (self.y + 4.0 * s, h - 8.0 * s);
        let vals: Vec<f32> = series.0.iter().copied().collect();
        if vals.len() >= 2 {
            let top = max.unwrap_or_else(|| vals.iter().copied().fold(1e-3, f32::max));
            let step = gw / (crate::cc::HISTORY - 1) as f32;
            let start = gx + gw - step * (vals.len() - 1) as f32;
            let mut pb = tiny_skia::PathBuilder::new();
            for (i, v) in vals.iter().enumerate() {
                let (px, py) = (start + i as f32 * step, gy + gh - (v / top).clamp(0.0, 1.0) * gh);
                if i == 0 {
                    pb.move_to(px, py);
                } else {
                    pb.line_to(px, py);
                }
            }
            if let Some(path) = pb.finish() {
                let mut paint = tiny_skia::Paint::default();
                paint.set_color(WHITE.alpha(0.85 * self.a).skia());
                paint.anti_alias = true;
                let stroke = tiny_skia::Stroke { width: 1.2 * s, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
                self.cv.pix.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
            }
        }
        self.y += h;
    }

    fn meter(&mut self, label: &str, frac: f32, value: &str) {
        let (x, iw) = self.inner();
        let s = self.s;
        let h = 22.0 * s;
        let b = self.baseline(self.y, h, 10.0);
        self.text(label, x, b, 10.0, Weight::Regular, GREY);
        self.text_right(value, x + iw, b, 10.0, WHITE);
        let (gx, gw) = (x + 46.0 * s, iw - 46.0 * s - 70.0 * s);
        let cells = 20;
        let cw = gw / cells as f32;
        for i in 0..cells {
            let on = (i as f32 + 0.5) / cells as f32 <= frac;
            let c = if on { if frac > 0.9 { RED } else { WHITE } } else { TILE_BG.alpha(1.6) };
            self.cv.rect(gx + i as f32 * cw, self.y + 8.0 * s, cw - 1.5 * s, 6.0 * s, c.alpha(self.a));
        }
        self.y += h;
    }

    fn session(&mut self, armed: Option<Session>) {
        let (x, iw) = self.inner();
        let s = self.s;
        let gap = 6.0 * s;
        let items = [(Session::Lock, "LOCK"), (Session::Logout, "LOGOUT"), (Session::Reboot, "REBOOT"), (Session::Off, "OFF")];
        let tw = (iw - 3.0 * gap) / 4.0;
        let th = 34.0 * s;
        for (i, (sess, label)) in items.iter().enumerate() {
            let tx = x + i as f32 * (tw + gap);
            let r = [tx, self.y, tw, th];
            let is_armed = armed == Some(*sess);
            let bg = if is_armed { RED } else if self.hovered(r) { Rgba(1.0, 1.0, 1.0, 0.12) } else { TILE_BG };
            self.cv.round_rect(tx, self.y, tw, th, 12.0 * s, bg.alpha(self.a));
            let text = if is_armed { "SURE?" } else { label };
            let c = if is_armed { WHITE } else if *sess == Session::Off { RED } else { WHITE };
            let lw = self.cv.fonts.width(Weight::Bold, 9.5 * s, text);
            let b = self.baseline(self.y, th, 9.5);
            self.text(text, tx + (tw - lw) / 2.0, b, 9.5, Weight::Bold, c);
            self.hits.push((r, PHit::Session(*sess)));
        }
        let _ = self.now;
        self.y += th + 8.0 * s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_needs_two_clicks_and_expires() {
        let mut p = Panel::new(Config::default());
        assert_eq!(p.press_session(Session::Off), None);
        assert_eq!(p.press_session(Session::Reboot), None, "a different button re-arms instead");
        p.tick(3.5);
        assert_eq!(p.press_session(Session::Reboot), None, "expired after 3 s");
        assert_eq!(p.press_session(Session::Reboot), Some(Session::Reboot));
    }

    #[test]
    fn opens_and_fades_out() {
        let mut p = Panel::new(Config::default());
        p.show(false);
        assert!(p.is_open() && p.tick(0.05));
        p.tick(0.2);
        p.hide(false);
        assert!(!p.gone());
        p.tick(0.2);
        assert!(p.gone());
    }

    #[test]
    fn main_page_fits_the_surface() {
        assert!(main_height(true, true) + 2.0 * MARGIN <= SURFACE_H as f32, "{}", main_height(true, true));
    }
}
