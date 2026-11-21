// Now playing (`nothing-bar player`, SUPER+P): a card under the clock. The album art is drawn as a dot matrix (each
// dot sized by brightness, Glyph style); hovering it brings the art's own colours in. A progress line you can click
// to seek, prev / play-pause / next, and the title decodes when the track changes.
// Keys: Space play/pause, ←/→ seek 5 s, Ctrl+←/→ previous/next, Esc close.
use crate::anim::Tween;
use crate::bar::{GREY, HOVER, RED, WHITE};
use crate::canvas::{Canvas, Rgba, Weight};
use crate::fx::{self, Img, Thumb};
use crate::icons::{self, Icon};
use crate::media::{self, Track};
use crate::shell::{Act, Frame, Key, Mods, Place, Ptr, Widget, DIM};
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use std::sync::{Arc, Mutex};

const W: f32 = 480.0;
const H: f32 = 204.0;
const PAD: f32 = 18.0;
const ART: f32 = 168.0;
const GRID: u32 = 28;

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Art,
    Seek,
    Prev,
    Toggle,
    Next,
}

type ArtSlot = Arc<Mutex<Option<(String, Option<Img>)>>>;

pub struct Player {
    media: media::Shared,
    track: Option<Track>,
    key: String,
    art: ArtSlot,
    colour: Tween,
    title: Tween,
    now: f32,
    hits: Vec<([f32; 4], Hit)>,
    pointer: Option<(f32, f32)>,
}

/// "file:///a%20b.png" -> "/a b.png"; http(s) URLs are downloaded into the cache first.
pub fn art_path(url: &str) -> Option<String> {
    if let Some(p) = url.strip_prefix("file://") {
        let p = percent_decode(p);
        return p.starts_with('/').then_some(p);
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut h);
        let dir = std::path::PathBuf::from(std::env::var("HOME").ok()?).join(".cache/nothing-bar/art");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join(format!("{:016x}", h.finish()));
        if !file.exists() {
            let ok = std::process::Command::new("curl").args(["-sfL", "--max-time", "8", "--max-filesize", "10M", "--proto", "=https,http", "--proto-redir", "=https,http", "-o"]).arg(&file).arg(url).status().is_ok_and(|s| s.success());
            if !ok {
                let _ = std::fs::remove_file(&file);
                return None;
            }
        }
        return Some(file.to_string_lossy().into_owned());
    }
    None
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

impl Player {
    pub fn new() -> Self {
        let media = media::watch(|| {});
        Self { media, track: None, key: String::new(), art: Arc::default(), colour: Tween::new(0.0), title: Tween::new(1.0), now: 0.0, hits: Vec::new(), pointer: None }
    }

    fn seek_to(&self, secs: f64) {
        crate::cc::run(&["playerctl", "position", &format!("{:.1}", secs.max(0.0))]);
    }

    fn seek_by(&self, d: f64) {
        if let Some(t) = &self.track {
            self.seek_to(t.position() + d);
        }
    }
}

impl Widget for Player {
    fn place(&self) -> Place {
        Place { anchor: Anchor::TOP, gap: 8, size: (W, H) }
    }

    fn key(&mut self, k: &Key, m: Mods, _now: f32) -> Act {
        match k {
            Key::Esc => return Act::Close,
            Key::Text(t) if t == " " => media::command("play-pause"),
            Key::Left if m.ctrl => media::command("previous"),
            Key::Right if m.ctrl => media::command("next"),
            Key::Left => self.seek_by(-5.0),
            Key::Right => self.seek_by(5.0),
            _ => {}
        }
        Act::Stay
    }

    fn pointer(&mut self, x: f32, y: f32, p: Ptr, now: f32) -> Act {
        self.pointer = (p != Ptr::Leave).then_some((x, y));
        let hit = self.hits.iter().find(|(r, _)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]).copied();
        self.colour.to(if matches!(hit, Some((_, Hit::Art))) { 1.0 } else { 0.0 }, now, 0.25);
        if p == Ptr::Press(0x110) {
            match hit {
                Some((r, Hit::Seek)) => {
                    if let Some(t) = self.track.as_ref().filter(|t| t.length > 0.0) {
                        self.seek_to(((x - r[0]) / r[2]).clamp(0.0, 1.0) as f64 * t.length);
                    }
                }
                Some((_, Hit::Prev)) => media::command("previous"),
                Some((_, Hit::Toggle)) => media::command("play-pause"),
                Some((_, Hit::Next)) => media::command("next"),
                _ => {}
            }
        }
        Act::Stay
    }

    fn tick(&mut self, now: f32) -> bool {
        self.now = now;
        self.track = self.media.lock().unwrap().clone();
        let key = self.track.as_ref().map_or(String::new(), |t| format!("{}\u{1}{}", t.artist, t.title));
        if key != self.key {
            self.key = key.clone();
            self.title.snap(0.0);
            self.title.to(1.0, now, 0.45);
            if !key.is_empty() {
                let slot = self.art.clone();
                std::thread::spawn(move || {
                    let url = std::process::Command::new("playerctl").args(["metadata", "mpris:artUrl"]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
                    let img = art_path(&url).and_then(|p| fx::thumb(Thumb::Path(p), GRID, GRID));
                    *slot.lock().unwrap() = Some((key, img));
                });
            }
        }
        self.colour.moving(now) || self.title.moving(now)
    }

    fn period(&self) -> Option<f32> {
        Some(0.25) // the progress line and time, and the art arriving
    }

    fn draw(&mut self, cv: &mut Canvas, f: &Frame) {
        let s = f.s;
        let a = f.a;
        let (x0, y0) = (f.x + PAD * s, f.y + PAD * s);
        self.hits.clear();
        let rel = |r: [f32; 4]| [r[0] - f.x, r[1] - f.y, r[2], r[3]];
        let hov = |r: [f32; 4]| self.pointer.is_some_and(|(x, y)| x + f.x >= r[0] && x + f.x < r[0] + r[2] && y + f.y >= r[1] && y + f.y < r[1] + r[3]);

        // Art: the dot matrix, or a faint empty grid while there is none.
        let art = ART * s;
        let cell = art / GRID as f32;
        cv.round_rect(x0, y0, art, art, 14.0 * s, Rgba(1.0, 1.0, 1.0, 0.03 * a));
        let slot = self.art.lock().unwrap();
        match slot.as_ref().filter(|(k, _)| *k == self.key).and_then(|(_, img)| img.as_ref()) {
            Some(img) => fx::dot_image(cv, img, x0, y0, cell, self.colour.get(f.now), a * f.reveal),
            None => {
                for j in 0..GRID {
                    for i in 0..GRID {
                        let d = cell * 0.22;
                        let (cx, cy) = (x0 + (i as f32 + 0.5) * cell, y0 + (j as f32 + 0.5) * cell);
                        cv.round_rect(cx - d / 2.0, cy - d / 2.0, d, d, d / 2.0, Rgba::hex(0x2a2a2a).alpha(a));
                    }
                }
            }
        }
        drop(slot);
        self.hits.push((rel([x0, y0, art, art]), Hit::Art));

        // Text column
        let tx = x0 + art + 20.0 * s;
        let tw = f.x + f.w - PAD * s - tx;
        let clip = Some((0.0, tx + tw));
        let base = y0 + 10.0 * s;
        let (state, dot) = match &self.track {
            Some(t) if t.playing => ("NOW PLAYING", true),
            Some(_) => ("PAUSED", false),
            None => ("IDLE", false),
        };
        let mut sx = tx;
        if dot {
            cv.round_rect(sx, base - 6.5 * s, 6.0 * s, 6.0 * s, 3.0 * s, RED.alpha(a));
            sx += 12.0 * s;
        }
        fx::decode_text(cv, state, sx, base, 9.5 * s, Weight::Bold, GREY.alpha(a), f.decode_at(base), None, 0.0);
        let p = self.title.get(f.now).min(f.decode_at(base + 30.0 * s));
        let (title, artist) = match &self.track {
            Some(t) => (if t.title.is_empty() { "UNKNOWN TITLE".to_string() } else { t.title.clone() }, t.artist.clone()),
            None => ("NOTHING PLAYING".to_string(), "Play something in any MPRIS player".to_string()),
        };
        let tb = base + 32.0 * s;
        fx::decode_text(cv, &title, tx, tb, 15.0 * s, Weight::Bold, WHITE.alpha(a), p, clip, 24.0 * s);
        fx::decode_text(cv, &artist, tx, tb + 22.0 * s, 11.5 * s, Weight::Regular, GREY.alpha(a), p, clip, 24.0 * s);

        // Progress: a hairline track, the played part in white, a red head.
        let py = y0 + 98.0 * s;
        let prog = self.track.as_ref().and_then(Track::progress).unwrap_or(0.0).clamp(0.0, 1.0);
        let r = [tx, py - 8.0 * s, tw, 16.0 * s];
        let thick = if hov(r) { 3.0 } else { 2.0 } * s;
        cv.round_rect(tx, py - thick / 2.0, tw, thick, thick / 2.0, Rgba(1.0, 1.0, 1.0, 0.14 * a));
        cv.round_rect(tx, py - thick / 2.0, tw * prog, thick, thick / 2.0, WHITE.alpha(a));
        if self.track.is_some() {
            let hx = tx + tw * prog;
            cv.round_rect(hx - 4.0 * s, py - 4.0 * s, 8.0 * s, 8.0 * s, 4.0 * s, RED.alpha(a));
        }
        self.hits.push((rel(r), Hit::Seek));
        if let Some(t) = &self.track {
            let tb2 = py + 18.0 * s;
            cv.text(&fmt_time(t.position()), tx, tb2, 9.5 * s, Weight::Regular, GREY.alpha(a), None, 0.0);
            if t.length > 0.0 {
                let left = format!("-{}", fmt_time(t.length - t.position()));
                let lw = cv.fonts.width(Weight::Regular, 9.5 * s, &left);
                cv.text(&left, tx + tw - lw, tb2, 9.5 * s, Weight::Regular, GREY.alpha(a), None, 0.0);
            }
        }

        // Transport: prev, play/pause in a white disc, next.
        let by = y0 + art - 40.0 * s;
        let big = 40.0 * s;
        let mid = tx + tw / 2.0;
        let playing = self.track.as_ref().is_some_and(|t| t.playing);
        let buttons = [(Hit::Prev, Icon::Prev, mid - big * 1.6), (Hit::Toggle, if playing { Icon::Pause } else { Icon::Play }, mid), (Hit::Next, Icon::Next, mid + big * 1.6)];
        for (hit, icon, cx) in buttons {
            let r = [cx - big / 2.0, by, big, big];
            let main = hit == Hit::Toggle;
            if main {
                cv.round_rect(r[0], r[1], r[2], r[3], big / 2.0, WHITE.alpha(a));
            } else if hov(r) {
                cv.round_rect(r[0], r[1], r[2], r[3], big / 2.0, HOVER.alpha(a));
            }
            let isz = if main { 18.0 } else { 16.0 } * s;
            let c = if main { Rgba::hex(0x000000) } else if self.track.is_some() { WHITE } else { DIM };
            icons::draw(cv, icon, (cx - isz / 2.0).round(), (by + (big - isz) / 2.0).round(), isz, c.alpha(a));
            self.hits.push((rel(r), hit));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_urls_and_times() {
        assert_eq!(art_path("file:///tmp/My%20Album/cover.jpg").as_deref(), Some("/tmp/My Album/cover.jpg"));
        assert_eq!(art_path(""), None);
        assert_eq!(art_path("file://msl:/tmp/x"), None);
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!((fmt_time(65.4), fmt_time(3725.0), fmt_time(-3.0)), ("1:05".into(), "1:02:05".into(), "0:00".into()));
    }
}
