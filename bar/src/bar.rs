// The bar's content: workspaces · app · title on the left, the clock in the centre, the status cluster on the right.
// Layout and drawing happen together every frame (it's ~20 glyphs); `hits` records where things landed for clicks.
use crate::anim::{Spring, Tween};
use crate::canvas::{Canvas, Rgba, Weight};
use crate::config::{ClockFormat, Config, Material, Position, Shape, WsStyle};
use crate::hypr::Snapshot;
use crate::icons::{self, Icon};
use crate::media::Track;
use crate::status::{Battery, Net};

pub const RED: Rgba = Rgba::hex(0xd71921);
pub const WHITE: Rgba = Rgba::hex(0xffffff);
pub const GREY: Rgba = Rgba::hex(0x999999);
pub const RAISED: Rgba = Rgba::hex(0x1a1a1a);
pub const HOVER: Rgba = Rgba::hex(0x242424);

/// How often the bar needs drawing: every display refresh (springs, rolls), ~30 fps (number tweens, the urgent
/// glow: they change a few pixels slowly), or not at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    None,
    Slow,
    Fast,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Workspace(i32),
    Stats,
    Net,
    Brightness,
    Volume,
    Battery,
    MediaPrev,
    MediaToggle,
    MediaNext,
    Bell,
}

/// One drawable bit of a status item.
enum Piece {
    Text(String, Rgba),
    Icon(Icon, Rgba),
    Battery,
}

struct Ws {
    id: i32,
    /// 0..1: how much of its slot it takes (grows in, shrinks out).
    appear: Spring,
    /// Scale bump when it becomes active.
    pop: Spring,
    /// 0 white .. 1 red.
    red: Tween,
    urgent: bool,
    leaving: bool,
}

pub struct Bar {
    pub cfg: Config,
    /// Turbo: Hyprland's animations are off, so the bar's are too.
    pub still: bool,
    now: f32,
    born: f32,
    ws: Vec<Ws>,
    active: i32,
    class: String,
    title: String,
    windows: Vec<(String, i32)>,
    clock: String,
    clock_prev: String,
    roll: Tween,
    cpu: Tween,
    ram: Tween,
    pub net: Option<Net>,
    bright: Option<Tween>,
    vol: Option<(Tween, bool)>,
    pub battery: Option<Battery>,
    pub dgpu: bool,
    /// Notifications that arrived since the panel was last opened.
    pub unread: usize,
    /// Pointer x in buffer pixels while it's over the bar.
    pub hover_x: Option<f32>,
    pill_x: Spring,
    pill_w: Spring,
    pill_a: Tween,
    /// The track being shown (kept while it fades out after the player stops).
    track: Option<Track>,
    media_t: Tween,
    /// How long until the bar next needs drawing when only slow things move (seconds).
    pub slow_interval: f32,
    pub hits: Vec<(f32, f32, Hit)>,
    /// The x range of the workspace numbers (scroll there switches workspace).
    pub ws_span: (f32, f32),
    /// Text colours for the current material (dark or light).
    fg: Rgba,
    dim: Rgba,
    /// The active window is maximized/fullscreen (the "shrink" position collapses to the island).
    busy: bool,
    /// Auto-hide: 0 tucked away above the screen edge .. 1 shown.
    reveal: Spring,
    /// Where the left content ended last frame (the left island's width).
    left_end: f32,
    /// The sliding red pill of the "pill" workspace style.
    wpill_x: Spring,
    wpill_w: Spring,
}

const POP_KICK: f32 = 7.0; // initial scale velocity: peaks at ~1.2x with the medium spring
const ROLL_S: f32 = 0.35;
const CASCADE: [f32; 3] = [0.0, 0.15, 0.3];

impl Bar {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            still: false,
            now: 0.0,
            born: 0.0,
            ws: Vec::new(),
            active: 0,
            class: String::new(),
            title: String::new(),
            windows: Vec::new(),
            clock: String::new(),
            clock_prev: String::new(),
            roll: Tween::new(1.0),
            cpu: Tween::new(0.0),
            ram: Tween::new(0.0),
            net: None,
            bright: None,
            vol: None,
            battery: None,
            dgpu: false,
            unread: 0,
            hover_x: None,
            pill_x: Spring::new(0.0),
            pill_w: Spring::smooth(0.0),
            pill_a: Tween::new(0.0),
            track: None,
            media_t: Tween::new(0.0),
            slow_interval: 1.0 / 30.0,
            hits: Vec::new(),
            ws_span: (0.0, 0.0),
            fg: WHITE,
            dim: GREY,
            busy: false,
            reveal: Spring::smooth(1.0),
            left_end: 0.0,
            wpill_x: Spring::new(0.0),
            wpill_w: Spring::smooth(0.0),
        }
    }

    pub fn apply(&mut self, s: Snapshot) {
        let now = self.now;
        for id in &s.workspaces {
            if let Some(w) = self.ws.iter_mut().find(|w| w.id == *id) {
                w.leaving = false;
                w.appear.set(1.0);
            } else {
                let mut appear = Spring::smooth(0.0);
                appear.set(1.0);
                let pos = self.ws.iter().position(|w| w.id > *id).unwrap_or(self.ws.len());
                self.ws.insert(pos, Ws { id: *id, appear, pop: Spring::new(1.0), red: Tween::new(0.0), urgent: false, leaving: false });
            }
        }
        for w in self.ws.iter_mut().filter(|w| !s.workspaces.contains(&w.id) && w.id != s.active) {
            w.leaving = true;
            w.appear.set(0.0);
        }
        // The active workspace always shows, even while it's still empty.
        if !self.ws.iter().any(|w| w.id == s.active) {
            let pos = self.ws.iter().position(|w| w.id > s.active).unwrap_or(self.ws.len());
            let mut appear = Spring::smooth(0.0);
            appear.set(1.0);
            self.ws.insert(pos, Ws { id: s.active, appear, pop: Spring::new(1.0), red: Tween::new(0.0), urgent: false, leaving: false });
        }
        if s.active != self.active {
            for w in &mut self.ws {
                if w.id == s.active {
                    w.red.to(1.0, now, 0.15);
                    w.pop.velocity += POP_KICK;
                    w.urgent = false;
                } else {
                    w.red.to(0.0, now, 0.15);
                }
            }
            self.active = s.active;
        }
        if let Some(w) = self.ws.iter_mut().find(|w| w.id == s.active) {
            w.leaving = false;
            w.appear.set(1.0);
        }
        (self.class, self.title, self.windows, self.busy) = (s.class, s.title, s.windows, s.busy);
        if self.still {
            self.settle();
        }
    }

    pub fn urgent(&mut self, addr: &str) {
        if let Some((_, ws)) = self.windows.iter().find(|(a, _)| a == addr)
            && *ws != self.active
            && let Some(w) = self.ws.iter_mut().find(|w| w.id == *ws)
        {
            w.urgent = true;
        }
    }

    pub fn set_clock(&mut self) {
        let (h, m, s, wd, d, mo) = crate::sys::local_time();
        const DAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
        const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
        let text = match self.cfg.clock {
            ClockFormat::HhMm => format!("{h:02}:{m:02}"),
            ClockFormat::HhMmSs => format!("{h:02}:{m:02}:{s:02}"),
            ClockFormat::Date => format!("{} {d} {}  {h:02}:{m:02}", DAYS[wd as usize % 7], MONTHS[mo as usize % 12]),
            ClockFormat::TwelveHour => format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" }),
        };
        if text != self.clock {
            self.clock_prev = std::mem::replace(&mut self.clock, text);
            if self.clock_prev.is_empty() || self.still {
                self.roll.snap(1.0);
            } else {
                self.roll.snap(0.0);
                self.roll.to(1.0, self.now, ROLL_S);
            }
        }
    }

    pub fn set_stats(&mut self, cpu: Option<f32>, ram: Option<f32>) {
        let first = self.cpu.target() == 0.0 && self.ram.target() == 0.0;
        if let Some(c) = cpu {
            self.cpu.to(c, self.now, if first || self.still { 0.0 } else { 0.4 });
        }
        if let Some(r) = ram {
            self.ram.to(r, self.now, if first || self.still { 0.0 } else { 0.4 });
        }
    }

    pub fn set_brightness(&mut self, v: Option<f32>) {
        let (now, d) = (self.now, if self.still { 0.0 } else { 0.25 });
        match (v, &mut self.bright) {
            (Some(v), Some(t)) => t.to(v, now, d),
            (Some(v), None) => self.bright = Some(Tween::new(v)),
            (None, _) => self.bright = None,
        }
    }

    pub fn set_volume(&mut self, v: Option<(f32, bool)>) {
        let (now, d) = (self.now, if self.still { 0.0 } else { 0.25 });
        match (v, &mut self.vol) {
            (Some((v, m)), Some((t, muted))) => {
                t.to(v, now, d);
                *muted = m;
            }
            (Some((v, m)), None) => self.vol = Some((Tween::new(v), m)),
            (None, _) => self.vol = None,
        }
    }

    pub fn set_track(&mut self, t: Option<Track>) {
        let d = if self.still { 0.0 } else { 0.35 };
        self.media_t.to(if t.is_some() { 1.0 } else { 0.0 }, self.now, d);
        if t.is_some() {
            self.track = t;
        }
    }

    /// Jumps every animation to its end (turbo).
    fn settle(&mut self) {
        for w in &mut self.ws {
            w.appear.snap(w.appear.target);
            w.pop.snap(1.0);
            w.red.snap(w.red.target());
        }
        self.ws.retain(|w| !w.leaving);
        self.roll.snap(1.0);
        self.cpu.snap(self.cpu.target());
        self.ram.snap(self.ram.target());
        if let Some(b) = &mut self.bright {
            b.snap(b.target());
        }
        if let Some((v, _)) = &mut self.vol {
            v.snap(v.target());
        }
        self.pill_x.snap(self.pill_x.target);
        self.pill_w.snap(self.pill_w.target);
        self.pill_a.snap(self.pill_a.target());
        self.media_t.snap(self.media_t.target());
        self.reveal.snap(self.reveal.target);
        self.wpill_x.snap(self.wpill_x.target);
        self.wpill_w.snap(self.wpill_w.target);
        self.born = -10.0;
    }

    /// Advances animations by `dt` and says how often the caller should draw next.
    pub fn tick(&mut self, dt: f32) -> Motion {
        self.now += dt;
        if self.still {
            self.settle();
            return Motion::None;
        }
        let now = self.now;
        let (mut fast, mut slow) = (false, false);
        for w in &mut self.ws {
            w.appear.step(dt);
            w.pop.step(dt);
            fast |= w.appear.moving() || w.pop.moving() || w.red.moving(now);
            slow |= w.urgent;
        }
        self.ws.retain(|w| !(w.leaving && !w.appear.moving() && w.appear.value < 0.01));
        self.pill_x.step(dt);
        self.pill_w.step(dt);
        self.reveal.step(dt);
        self.wpill_x.step(dt);
        self.wpill_w.step(dt);
        fast |= self.reveal.moving() || self.wpill_x.moving() || self.wpill_w.moving();
        fast |= self.roll.moving(now) || now - self.born < CASCADE[2] + 0.4;
        fast |= self.pill_x.moving() || self.pill_w.moving() || self.pill_a.moving(now);
        slow |= self.cpu.moving(now) || self.ram.moving(now);
        slow |= self.bright.is_some_and(|b| b.moving(now)) || self.vol.is_some_and(|(v, _)| v.moving(now));
        slow |= self.battery.is_some_and(|b| b.charging);
        fast |= self.media_t.moving(now);
        if self.media_t.target() == 0.0 && !self.media_t.moving(now) {
            self.track = None;
        }
        // The progress line only needs a redraw when it has moved a whole pixel.
        self.slow_interval = 1.0 / 30.0;
        if let Some(t) = self.track.as_ref().filter(|t| t.playing && t.length > 0.0) {
            let per_px = (t.length / 1920.0) as f32;
            if !slow {
                self.slow_interval = per_px.clamp(1.0 / 30.0, 1.0);
            }
            slow = true;
        }
        if fast {
            Motion::Fast
        } else if slow {
            Motion::Slow
        } else {
            Motion::None
        }
    }

    /// Login cascade: (alpha, y offset in px) for section `i` (0 left, 1 centre, 2 right).
    fn cascade(&self, i: usize, s: f32) -> (f32, f32) {
        let t = ((self.now - self.born - CASCADE[i]) / 0.35).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        (e, (1.0 - e) * -6.0 * s)
    }

    /// The shape actually drawn: "shrink" collapses to the centre island while a window is maximized.
    pub fn shape(&self) -> Shape {
        if self.cfg.position == Position::Shrink && self.busy { Shape::Island } else { self.cfg.shape }
    }

    /// Auto-hide: slide in (true) or tuck away (false).
    pub fn set_revealed(&mut self, on: bool) {
        self.reveal.set(if on { 1.0 } else { 0.0 });
        if self.still {
            self.reveal.snap(self.reveal.target);
        }
    }

    pub fn revealed(&self) -> bool {
        self.reveal.target > 0.5
    }

    /// The fill behind the content for the material (None: transparent).
    fn surface(&self) -> Option<Rgba> {
        match self.cfg.material {
            Material::Solid => Some(RAISED),
            Material::Translucent => Some(RAISED.alpha(0.62)),
            Material::Frosted => Some(Rgba::hex(0x0d0d0d).alpha(0.55)),
            Material::Clear | Material::Fade => None,
            Material::Light => Some(Rgba::hex(0xf2f2f2)),
        }
    }

    /// Draws the whole bar into `cv` (buffer pixels; `s` = output scale).
    pub fn draw(&mut self, cv: &mut Canvas, s: f32) {
        let (w, h) = (cv.pix.width() as f32, cv.pix.height() as f32);
        let px = self.cfg.font_px * s;
        let top = self.cfg.position.at_top();
        let light = self.cfg.material == Material::Light;
        (self.fg, self.dim) = if light { (Rgba::hex(0x111111), Rgba::hex(0x6b6b6b)) } else { (WHITE, GREY) };
        let hover = if light { Rgba::hex(0xdcdcdc) } else { HOVER };
        let line = if light { Rgba::hex(0xc8c8c8) } else { Rgba::hex(0x2e2e2e) };
        let shape = self.shape();
        let clear = Rgba(0.0, 0.0, 0.0, 0.0);
        // Pills and floating cards need some fill even with "clear"/"fade" materials.
        let card = self.surface().unwrap_or(RAISED.alpha(0.62));
        let cap = cv.fonts.cap_height(Weight::Regular, px);
        let base = ((h + cap) / 2.0).round();
        let pad = if shape == Shape::Islands { 18.0 * s } else { 12.0 * s };
        let now = self.now;

        // ---- measure the centre and the right cluster first (backgrounds go behind them)
        let cpx = px * 13.0 / 12.0;
        let clock_w = cv.fonts.width(Weight::Bold, cpx, &self.clock);
        let items = if shape == Shape::Island { Vec::new() } else { self.status_items() };
        let (gap, inner, isz) = (14.0 * s, 5.0 * s, (px * 1.05).round());
        let piece_w = |cv: &Canvas, p: &Piece| match p {
            Piece::Text(t, _) => cv.fonts.width(Weight::Regular, px, t),
            Piece::Icon(..) => isz,
            Piece::Battery => 24.0 * s,
        };
        let item_w = |cv: &Canvas, ps: &[Piece]| ps.iter().map(|p| piece_w(cv, p)).sum::<f32>() + inner * ps.len().saturating_sub(1) as f32;
        let total: f32 = items.iter().map(|(_, ps)| item_w(cv, ps)).sum::<f32>() + gap * items.len().saturating_sub(1) as f32;
        let right_x0 = (w - pad - total).round();

        // The island: workspaces, clock and battery in one centred pill.
        let island_ws = if shape == Shape::Island { self.ws_width(cv, px, s) } else { 0.0 };
        let island_bat = if self.battery.is_some() { 24.0 * s + gap } else { 0.0 };
        let island_w = 16.0 * s + island_ws + gap + clock_w + island_bat + 16.0 * s;
        let island_x = ((w - island_w) / 2.0).round();
        let cx = if shape == Shape::Island { island_x + 16.0 * s + island_ws + gap } else { ((w - clock_w) / 2.0).round() };

        // ---- background
        match shape {
            Shape::Strip => match self.cfg.material {
                Material::Fade => {
                    cv.clear(clear);
                    for y in 0..h as i32 {
                        let t = y as f32 / h;
                        cv.rect(0.0, y as f32, w, 1.0, Rgba(0.0, 0.0, 0.0, if top { 1.0 - t * 0.9 } else { 0.1 + t * 0.9 }));
                    }
                }
                _ => cv.clear(self.surface().unwrap_or(clear)),
            },
            Shape::Floating => {
                cv.clear(clear);
                cv.round_rect(0.0, 0.0, w, h, 10.0 * s, card);
                cv.stroke_round_rect(0.5, 0.5, w - 1.0, h - 1.0, 10.0 * s, 1.0, line);
            }
            Shape::Islands => {
                cv.clear(clear);
                let pills = [
                    (pad - 10.0 * s, self.left_end.max(pad + 20.0 * s) + 10.0 * s),
                    (cx - 14.0 * s, cx + clock_w + 14.0 * s),
                    (right_x0 - 12.0 * s, w - pad + 12.0 * s),
                ];
                for (a, b) in pills {
                    cv.round_rect(a, 0.0, b - a, h, h / 2.0, card);
                    cv.stroke_round_rect(a + 0.5, 0.5, b - a - 1.0, h - 1.0, h / 2.0, 1.0, line);
                }
            }
            Shape::Island => {
                cv.clear(clear);
                cv.round_rect(island_x, 0.0, island_w, h, h / 2.0, card);
                cv.stroke_round_rect(island_x + 0.5, 0.5, island_w - 1.0, h - 1.0, h / 2.0, 1.0, line);
            }
        }
        if self.cfg.hairline && shape == Shape::Strip {
            cv.hairline(if top { h - 1.0 } else { 0.0 }, 0.0, w, self.fg.alpha(0.45));
        }

        // Hover pill, placed from where things were last frame; it glides between items and fades in and out.
        self.update_pill(s);
        let pa = self.pill_a.get(now);
        if pa > 0.01 {
            let ph = h - 8.0 * s;
            cv.round_rect(self.pill_x.value, (h - ph) / 2.0, self.pill_w.value.max(0.0), ph, 6.0 * s, hover.alpha(pa));
        }
        self.hits.clear();

        // ---- centre: clock
        let (ca, cdy) = self.cascade(1, s);
        self.draw_clock(cv, cx, base + cdy, cpx, ca);

        if shape == Shape::Island {
            let (la, ldy) = self.cascade(0, s);
            self.draw_workspaces(cv, island_x + 16.0 * s, base + ldy, px, cap, h, s, la);
            if self.battery.is_some() {
                let bx = cx + clock_w + gap;
                self.draw_battery(cv, bx, h / 2.0 + ldy, s, la);
                self.hits.push((bx, bx + 24.0 * s, Hit::Battery));
            }
            self.finish(cv, s);
            return;
        }

        // ---- right: status cluster, right-aligned
        let (ra, rdy) = self.cascade(2, s);
        let mut x = right_x0;
        let mut right_hits = Vec::new();
        for (hit, ps) in &items {
            let x0 = x;
            for p in ps {
                let pw = piece_w(cv, p);
                match p {
                    Piece::Text(t, c) => {
                        cv.text(t, x, base + rdy, px, Weight::Regular, c.alpha(ra), None, 0.0);
                    }
                    Piece::Icon(i, c) => icons::draw(cv, *i, x, ((h - isz) / 2.0).round() + rdy, isz, c.alpha(ra)),
                    Piece::Battery => self.draw_battery(cv, x, h / 2.0 + rdy, s, ra),
                }
                x += pw + inner;
            }
            x += gap - inner;
            if let Some(hit) = hit {
                right_hits.push((x0, x - gap, *hit));
            }
        }

        // ---- left: workspaces, then app · title, then media
        let (la, ldy) = self.cascade(0, s);
        let mut x = self.draw_workspaces(cv, pad, base + ldy, px, cap, h, s, la);
        x += 4.0 * s;
        let (fg, dim) = (self.fg, self.dim);
        // In islands the left pill must not reach the clock's pill.
        let limit = cx - if shape == Shape::Islands { 44.0 * s } else { 24.0 * s };
        // Media takes up to `media_w` at the end of the left side; the title gets what's left.
        let mt = self.media_t.get(now);
        let misz = (px * 0.95).round();
        let label = self.track.as_ref().map(|t| t.label()).unwrap_or_default();
        let label_w = cv.fonts.width(Weight::Regular, px, &label).min(260.0 * s);
        let media_w = if self.track.is_some() { (3.0 * misz + 3.0 * 8.0 * s + label_w + 16.0 * s) * mt } else { 0.0 };
        let title_limit = (limit - media_w).max(x);
        if !self.class.is_empty() && x < title_limit {
            let app = pretty_class(&self.class);
            let clip = Some((0.0, title_limit));
            x = cv.text(&app, x, base + ldy, px, Weight::Bold, fg.alpha(la), clip, 24.0 * s);
            let short = short_title(&self.title, &self.class);
            if !short.is_empty() {
                x = cv.text(" · ", x, base + ldy, px, Weight::Regular, dim.alpha(la), clip, 24.0 * s);
                x = cv.text(&short, x, base + ldy, px, Weight::Regular, dim.alpha(la), clip, 24.0 * s);
            }
            x = x.min(title_limit);
        }
        if let Some(track) = self.track.clone() {
            // Slides in from the left while fading in.
            let e = mt;
            let mut mx = x + 16.0 * s - (1.0 - e) * 16.0 * s;
            let a = la * e;
            let iy = ((h - misz) / 2.0).round() + ldy;
            let play = if track.playing { Icon::Pause } else { Icon::Play };
            for (icon, hit) in [(Icon::Prev, Hit::MediaPrev), (play, Hit::MediaToggle), (Icon::Next, Hit::MediaNext)] {
                icons::draw(cv, icon, mx, iy, misz, fg.alpha(a));
                self.hits.push((mx - 3.0 * s, mx + misz + 3.0 * s, hit));
                mx += misz + 8.0 * s;
            }
            let end = (mx + label_w).min(limit);
            cv.text(&label, mx, base + ldy, px, Weight::Regular, dim.alpha(a), Some((0.0, end)), 24.0 * s);
            x = x.max(end);
            if let Some(p) = track.progress() {
                let ly = if top { h - 2.0 * s } else { 0.0 };
                let (lx, lw) = if shape == Shape::Strip { (0.0, w) } else { (pad, (self.left_end - pad).max(0.0)) };
                cv.rect(lx, ly, (lw * p).round(), 2.0 * s, RED.alpha(e));
            }
        }
        self.left_end = x;
        self.hits.extend(right_hits);
        self.finish(cv, s);
    }

    /// Auto-hide: shifts everything up by the hidden part of the bar.
    fn finish(&mut self, cv: &mut Canvas, _s: f32) {
        let hidden = 1.0 - self.reveal.value.clamp(0.0, 1.0);
        if hidden <= 0.0 {
            return;
        }
        let (w, h) = (cv.pix.width() as usize, cv.pix.height() as usize);
        let dy = ((hidden * h as f32).round() as usize).min(h);
        let data = cv.pix.data_mut();
        data.copy_within(dy * w * 4.., 0);
        data[(h - dy) * w * 4..].fill(0);
    }

    /// Width the workspace indicator takes (for centring the island).
    fn ws_width(&self, cv: &Canvas, px: f32, s: f32) -> f32 {
        let dots = self.cfg.workspaces == WsStyle::Dots;
        let gap = if dots { 7.0 * s } else { 10.0 * s };
        self.ws
            .iter()
            .map(|w| {
                let lw = if dots { 7.0 * s } else { cv.fonts.width(Weight::Bold, px, &w.id.to_string()) };
                (lw + gap) * w.appear.value.clamp(0.0, 1.0)
            })
            .sum::<f32>()
            .max(gap)
            - gap
    }

    /// Draws the workspace indicator in the configured style from x; returns where it ends.
    #[allow(clippy::too_many_arguments)]
    fn draw_workspaces(&mut self, cv: &mut Canvas, x0: f32, base: f32, px: f32, cap: f32, h: f32, s: f32, la: f32) -> f32 {
        let now = self.now;
        let style = self.cfg.workspaces;
        let (fg, light) = (self.fg, self.cfg.material == Material::Light);
        let ink = if light { WHITE } else { Rgba::hex(0x111111) };
        let gap = if style == WsStyle::Dots { 7.0 * s } else { 10.0 * s };
        let breathe = 0.65 + 0.35 * (now * std::f32::consts::TAU / 1.8).cos();
        // Layout first, so the sliding pill can go behind the numbers.
        let mut slots = Vec::with_capacity(self.ws.len());
        let mut x = x0;
        for wsi in &self.ws {
            let a = wsi.appear.value.clamp(0.0, 1.0);
            let lw = if style == WsStyle::Dots { 7.0 * s } else { cv.fonts.width(Weight::Bold, px, &wsi.id.to_string()) };
            slots.push((x, lw, a));
            x += (lw + gap) * a;
        }
        let end = x - gap.min(x - x0);
        if style == WsStyle::Pill
            && let Some(i) = self.ws.iter().position(|w| w.id == self.active)
        {
            let (sx, lw, _) = slots[i];
            let (tx, tw) = (sx - 6.0 * s, lw + 12.0 * s);
            if self.wpill_w.value <= 0.0 || self.still {
                self.wpill_x.snap(tx);
                self.wpill_w.snap(tw);
            } else {
                self.wpill_x.set(tx);
                self.wpill_w.set(tw);
            }
            let ph = h - 10.0 * s;
            cv.round_rect(self.wpill_x.value, (h - ph) / 2.0, self.wpill_w.value, ph, ph / 2.0, RED.alpha(la));
        }
        let mut hits = Vec::with_capacity(self.ws.len());
        for (wsi, &(sx, lw, a)) in self.ws.iter().zip(&slots) {
            let r = wsi.red.get(now);
            let alpha = ((a - 0.4) / 0.6).clamp(0.0, 1.0) * la;
            if style == WsStyle::Dots {
                let c = if wsi.urgent { RED.alpha(breathe) } else { fg.mix(RED, r) };
                let rad = 3.5 * s * wsi.pop.value;
                cv.round_rect(sx + lw / 2.0 - rad, h / 2.0 - rad, 2.0 * rad, 2.0 * rad, rad, c.alpha(alpha));
            } else {
                let (mut color, scale) = match style {
                    WsStyle::Block => {
                        let bh = h - 10.0 * s;
                        cv.round_rect(sx - 4.0 * s, (h - bh) / 2.0, lw + 8.0 * s, bh, 4.0 * s, fg.alpha(alpha * r));
                        (fg.mix(ink, r), 1.0)
                    }
                    WsStyle::Dot => {
                        let d = 2.0 * s;
                        cv.round_rect(sx + lw / 2.0 - d, base + 3.0 * s, 2.0 * d, 2.0 * d, d, RED.alpha(alpha * r));
                        (fg, 1.0)
                    }
                    WsStyle::Pill => (fg.mix(WHITE, r), 1.0),
                    _ => (fg.mix(RED, r), wsi.pop.value),
                };
                if wsi.urgent {
                    color = RED.alpha(breathe);
                }
                let label = wsi.id.to_string();
                let lpx = px * scale;
                let lw_s = cv.fonts.width(Weight::Bold, lpx, &label);
                cv.text(&label, sx + (lw - lw_s) / 2.0, base + (scale - 1.0) * cap / 2.0, lpx, Weight::Bold, color.alpha(alpha), None, 0.0);
            }
            hits.push((sx - gap / 2.0, sx + lw + gap / 2.0, Hit::Workspace(wsi.id)));
        }
        self.hits.extend(hits);
        self.ws_span = (x0 - 12.0 * s, end);
        end
    }

    /// The right-hand items, left to right: (what a click hits, pieces).
    fn status_items(&self) -> Vec<(Option<Hit>, Vec<Piece>)> {
        let now = self.now;
        let mut items = Vec::new();
        if self.dgpu {
            items.push((None, vec![Piece::Icon(Icon::Gpu, RED), Piece::Text("DGPU".into(), RED)]));
        }
        if self.still {
            items.push((None, vec![Piece::Icon(Icon::Bolt, RED), Piece::Text("TURBO".into(), RED)]));
        }
        // Values are padded on the right to a fixed width (monospace font), so labels and icons stay put while
        // numbers tween, and the slack goes into the gap after the value.
        items.push((
            Some(Hit::Stats),
            vec![
                Piece::Text("CPU".into(), self.dim),
                Piece::Text(format!("{:<4}", format!("{:.0}%", self.cpu.get(now))), self.fg),
                Piece::Text("RAM".into(), self.dim),
                Piece::Text(format!("{:<5}", format!("{:.1}G", self.ram.get(now))), self.fg),
            ],
        ));
        match &self.net {
            Some(n) => items.push((
                Some(Hit::Net),
                vec![Piece::Icon(if n.wifi { Icon::Wifi } else { Icon::Eth }, self.fg), Piece::Text(format!("{:<6}", crate::status::fmt_rate(n.rx_rate)), self.fg)],
            )),
            None => items.push((Some(Hit::Net), vec![Piece::Icon(Icon::WifiOff, self.dim)])),
        }
        if let Some(b) = &self.bright {
            items.push((Some(Hit::Brightness), vec![Piece::Icon(Icon::Sun, self.fg), Piece::Text(format!("{:<3.0}", b.get(now)), self.fg)]));
        }
        if let Some((v, muted)) = &self.vol {
            let c = if *muted { self.dim } else { self.fg };
            items.push((Some(Hit::Volume), vec![Piece::Icon(if *muted { Icon::Mute } else { Icon::Vol }, c), Piece::Text(format!("{:<3.0}", v.get(now)), c)]));
        }
        if self.battery.is_some() {
            items.push((Some(Hit::Battery), vec![Piece::Battery]));
        }
        if self.unread > 0 {
            items.push((Some(Hit::Bell), vec![Piece::Icon(Icon::Bell, self.fg), Piece::Text(self.unread.min(99).to_string(), self.fg)]));
        }
        items
    }

    /// Five cells that empty as the battery drains; red under 20%; a sweep fills them while charging.
    fn draw_battery(&self, cv: &mut Canvas, x: f32, cy: f32, s: f32, alpha: f32) {
        let Some(b) = self.battery else { return };
        let (bw, bh) = (21.0 * s, 10.0 * s);
        let y = (cy - bh / 2.0).round();
        let low = b.percent < 20.0 && !b.charging;
        let c = if low { RED } else { self.fg };
        cv.stroke_round_rect(x + 0.5 * s, y + 0.5 * s, bw - s, bh - s, 2.5 * s, s, c.alpha(alpha));
        cv.round_rect(x + bw, y + 3.0 * s, 2.0 * s, bh - 6.0 * s, 0.8 * s, c.alpha(alpha));
        let level = (b.percent / 20.0).ceil().clamp(0.0, 5.0) as usize;
        let sweep = if b.charging { ((self.now % 1.5) / 1.5 * 6.0) as usize } else { 0 };
        let (cw, cg, inset) = (2.6 * s, 0.9 * s, 2.2 * s);
        for i in 0..level.max(sweep).min(5) {
            cv.rect(x + inset + i as f32 * (cw + cg), y + inset, cw, bh - 2.0 * inset, c.alpha(alpha));
        }
    }

    /// Points the hover pill at whatever hoverable thing is under the pointer.
    fn update_pill(&mut self, s: f32) {
        let now = self.now;
        let d = if self.still { 0.0 } else { 0.12 };
        let target = self.hover_x.and_then(|x| self.hits.iter().find(|(a, b, _)| x >= *a && x < *b)).map(|(a, b, _)| (*a, *b));
        match target {
            Some((a, b)) => {
                let (tx, tw) = (a - 6.0 * s, b - a + 12.0 * s);
                if self.pill_a.target() == 0.0 {
                    // Appears where it is, rather than gliding in from its last spot.
                    self.pill_x.snap(tx);
                    self.pill_w.snap(tw);
                } else {
                    self.pill_x.set(tx);
                    self.pill_w.set(tw);
                }
                self.pill_a.to(1.0, now, d);
            }
            None => self.pill_a.to(0.0, now, d),
        }
    }

    /// The clock; characters that changed roll up (old one out the top, new one in from below).
    fn draw_clock(&mut self, cv: &mut Canvas, x: f32, base: f32, px: f32, alpha: f32) {
        let r = self.roll.get(self.now);
        let rise = cv.fonts.cap_height(Weight::Bold, px) * 1.1;
        let prev: Vec<char> = self.clock_prev.chars().collect();
        let same_len = prev.len() == self.clock.chars().count();
        let mut pen = x;
        for (i, ch) in self.clock.chars().enumerate() {
            let adv = cv.fonts.advance(Weight::Bold, px, ch);
            let old = prev.get(i).copied().filter(|&o| same_len && o != ch && r < 1.0);
            let mut buf = [0u8; 4];
            match old {
                Some(o) => {
                    let mut ob = [0u8; 4];
                    cv.text(o.encode_utf8(&mut ob), pen, base - rise * r, px, Weight::Bold, self.fg.alpha(alpha * (1.0 - r)), None, 0.0);
                    cv.text(ch.encode_utf8(&mut buf), pen, base + rise * (1.0 - r), px, Weight::Bold, self.fg.alpha(alpha * r), None, 0.0);
                }
                None => {
                    cv.text(ch.encode_utf8(&mut buf), pen, base, px, Weight::Bold, self.fg.alpha(alpha), None, 0.0);
                }
            }
            pen += adv;
        }
    }

    pub fn hit(&self, x: f32) -> Option<Hit> {
        self.hits.iter().find(|(a, b, _)| x >= *a && x < *b).map(|h| h.2)
    }

    /// The hovered item, its centre x (buffer px) and a short tooltip for it.
    pub fn tooltip(&self) -> Option<(Hit, f32, String)> {
        let x = self.hover_x?;
        let (a, b, hit) = *self.hits.iter().find(|(a, b, _)| x >= *a && x < *b)?;
        let now = self.now;
        let text = match hit {
            Hit::Workspace(id) => format!("Workspace {id} · scroll to switch"),
            Hit::Stats => format!("CPU {:.0}% · RAM {:.1} G · click for btop", self.cpu.get(now), self.ram.get(now)),
            Hit::Net => match &self.net {
                Some(n) => format!(
                    "{} · {} {} · click for the control centre",
                    if n.wifi { "Wi-Fi" } else { "Ethernet" },
                    crate::status::fmt_rate(n.rx_rate),
                    crate::status::fmt_rate(n.tx_rate).replace('↓', "↑")
                ),
                None => "Offline · click for the control centre".into(),
            },
            Hit::Brightness => format!("Brightness {:.0}% · scroll to change", self.bright.map_or(0.0, |b| b.target())),
            Hit::Volume => {
                let (v, muted) = self.vol.map_or((0.0, false), |(v, m)| (v.target(), m));
                format!("Volume {v:.0}%{} · scroll · middle-click mutes · right-click mixer", if muted { " (muted)" } else { "" })
            }
            Hit::Battery => match self.battery {
                Some(b) => format!("Battery {:.0}% · {}", b.percent, if b.charging { "charging" } else if b.percent >= 99.0 { "full" } else { "on battery" }),
                None => return None,
            },
            Hit::MediaPrev => "Previous".into(),
            Hit::MediaToggle => "Play / pause".into(),
            Hit::MediaNext => "Next".into(),
            Hit::Bell => format!("{} new notification{}", self.unread, if self.unread == 1 { "" } else { "s" }),
        };
        Some((hit, (a + b) / 2.0, text))
    }
}

/// "firefox" -> "Firefox", "org.gnome.Nautilus" -> "Nautilus", "kitty" -> "Kitty"
pub fn pretty_class(class: &str) -> String {
    let last = class.rsplit('.').next().unwrap_or(class);
    let mut c = last.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The useful part of a window title: drop a trailing " — App" / " - App" that repeats the app name.
pub fn short_title(title: &str, class: &str) -> String {
    let app = pretty_class(class).to_lowercase();
    for sep in [" — ", " - ", " – "] {
        if let Some((head, tail)) = title.rsplit_once(sep)
            && tail.to_lowercase().contains(&app)
        {
            return head.trim().to_string();
        }
    }
    if title.to_lowercase() == app { String::new() } else { title.trim().to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(ws: &[i32], active: i32) -> Snapshot {
        Snapshot { workspaces: ws.to_vec(), active, ..Default::default() }
    }

    #[test]
    fn titles_lose_the_app_suffix() {
        assert_eq!(short_title("GitHub — Mozilla Firefox", "firefox"), "GitHub");
        assert_eq!(short_title("main.rs - nvim", "kitty"), "main.rs - nvim");
        assert_eq!(short_title("kitty", "kitty"), "");
        assert_eq!(pretty_class("org.gnome.Nautilus"), "Nautilus");
    }

    #[test]
    fn workspaces_grow_in_and_shrink_out() {
        let mut b = Bar::new(Config::default());
        b.apply(snap(&[1, 2], 1));
        assert_eq!(b.ws.iter().map(|w| w.id).collect::<Vec<_>>(), vec![1, 2]);
        for _ in 0..200 {
            b.tick(1.0 / 144.0);
        }
        assert!(b.ws.iter().all(|w| w.appear.value == 1.0));
        b.apply(snap(&[1], 1));
        assert_eq!(b.tick(1.0 / 144.0), Motion::Fast, "shrinking out animates");
        for _ in 0..400 {
            b.tick(1.0 / 144.0);
        }
        assert_eq!(b.ws.len(), 1);
    }

    #[test]
    fn active_workspace_pops_red_and_clears_urgent() {
        let mut b = Bar::new(Config::default());
        let mut s = snap(&[1, 3], 1);
        s.windows = vec![("0xabc".into(), 3)];
        b.apply(s.clone());
        b.urgent("0xabc");
        assert!(b.ws[1].urgent);
        s.active = 3;
        b.apply(s);
        assert!(!b.ws[1].urgent && b.ws[1].pop.velocity > 0.0);
        for _ in 0..300 {
            b.tick(1.0 / 144.0);
        }
        assert_eq!(b.ws[1].red.get(b.now), 1.0);
        assert_eq!(b.ws[0].red.get(b.now), 0.0);
        assert_eq!(b.tick(1.0 / 144.0), Motion::None, "settles, so the bar stops drawing");
    }

    #[test]
    fn turbo_settles_everything_at_once() {
        let mut b = Bar::new(Config::default());
        b.still = true;
        b.apply(snap(&[1, 2], 2));
        assert_eq!(b.tick(0.001), Motion::None);
        assert!(b.ws.iter().all(|w| w.appear.value == 1.0 && w.pop.value == 1.0));
    }
}
