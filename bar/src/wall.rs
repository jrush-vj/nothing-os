// Wallpaper picker (`nothing-bar wall`, SUPER+W): generated Nothing patterns plus any images in
// ~/Pictures/Wallpapers. ←/→ to choose, Enter to apply. Applying turns the picker into a full-screen dot matrix:
// dots close in over the desktop, swaybg is swapped underneath, and the dots open again from the centre.
// The choice is kept in ~/.config/nothing-bar/wallpaper: line 1 what was picked ("preset:signal" or the image),
// line 2 the file swaybg shows (autostart.lua reads it at login).
use crate::bar::{GREY, RED, WHITE};
use crate::canvas::{Canvas, Rgba, Weight};
use crate::fx::{self, Img, Thumb};
use crate::shell::{self, Act, Frame, Key, Mods, Place, Ptr, Widget};
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

const W: f32 = 760.0;
const H: f32 = 262.0;
const PAD: f32 = 18.0;
const CW: f32 = 168.0;
const CH: f32 = 105.0;
const GAP: f32 = 14.0;
/// Transition timing (s): close in, hold while swaybg draws, open out.
const CLOSE: f32 = 0.4;
const HOLD: f32 = 0.35;
const OPEN: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Preset {
    Grid,
    Fine,
    Signal,
    Glyph,
    Horizon,
    Void,
}

const PRESETS: [(Preset, &str); 6] =
    [(Preset::Grid, "DOT GRID"), (Preset::Fine, "FINE GRID"), (Preset::Signal, "SIGNAL"), (Preset::Glyph, "GLYPH"), (Preset::Horizon, "HORIZON"), (Preset::Void, "VOID")];

#[derive(Clone, Debug, PartialEq)]
enum Source {
    Preset(Preset),
    Image(PathBuf),
}

struct Item {
    name: String,
    src: Source,
}

fn gray(v: u32) -> Rgba {
    Rgba::hex(v * 0x010101)
}

fn dot(pix: &mut Pixmap, x: f32, y: f32, r: f32, c: Rgba) {
    if let Some(path) = PathBuilder::from_circle(x, y, r.max(0.3)) {
        let mut p = Paint::default();
        p.set_color(c.skia());
        p.anti_alias = true;
        pix.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    }
}

/// Renders a pattern at w×h. `k` scales the pattern (1 = a 1080p screen; thumbnails use less).
pub fn render(p: Preset, w: u32, h: u32, k: f32) -> Option<Pixmap> {
    let mut pix = Pixmap::new(w.max(1), h.max(1))?;
    pix.fill(Rgba::hex(0x000000).skia());
    let (wf, hf) = (w as f32, h as f32);
    let grid = |pix: &mut Pixmap, pitch: f32, f: &dyn Fn(f32, f32) -> (f32, Rgba)| {
        let pitch = pitch * k;
        let (nx, ny) = ((wf / pitch).ceil() as i32 + 1, (hf / pitch).ceil() as i32 + 1);
        // Centre the grid so it sits evenly on any screen.
        let (ox, oy) = ((wf - (nx - 1) as f32 * pitch) / 2.0, (hf - (ny - 1) as f32 * pitch) / 2.0);
        for j in 0..ny {
            for i in 0..nx {
                let (x, y) = (ox + i as f32 * pitch, oy + j as f32 * pitch);
                let (r, c) = f(x, y);
                dot(pix, x, y, r * k, c);
            }
        }
    };
    match p {
        Preset::Grid => grid(&mut pix, 28.0, &|_, _| (1.6, gray(0x2a))),
        Preset::Fine => grid(&mut pix, 14.0, &|_, _| (1.0, gray(0x26))),
        Preset::Signal => {
            let (sx, sy, reach) = (wf * 0.76, hf * 0.68, hf * 0.42);
            grid(&mut pix, 28.0, &|x, y| {
                let t = (1.0 - ((x - sx).hypot(y - sy) / reach)).clamp(0.0, 1.0);
                (1.6 + 2.6 * t * t, gray(0x2a).mix(Rgba::hex(0xd71921), t * t))
            })
        }
        Preset::Glyph => {
            let (cx, cy) = (wf / 2.0, hf / 2.0);
            let step = 44.0 * k;
            let rings = ((wf.hypot(hf) / 2.0) / step) as i32 + 1;
            for ring in 1..=rings {
                let rad = ring as f32 * step;
                let n = ((std::f32::consts::TAU * rad) / (14.0 * k)).max(6.0) as i32;
                let fade = (1.0 - ring as f32 / rings as f32).powf(0.6);
                for i in 0..n {
                    let ang = i as f32 / n as f32 * std::f32::consts::TAU;
                    let (x, y) = (cx + rad * ang.cos(), cy + rad * ang.sin());
                    // One arc of the third ring is lit: the Glyph signal.
                    let lit = ring == 3 && (-0.9..-0.15).contains(&(ang - std::f32::consts::PI * 1.5));
                    let c = if lit { Rgba::hex(0xd71921) } else { gray(0x20).mix(gray(0x3a), fade) };
                    dot(&mut pix, x, y, if lit { 2.4 } else { 1.4 } * k, c);
                }
            }
        }
        Preset::Horizon => grid(&mut pix, 20.0, &|_, y| {
            let t = (y / hf).clamp(0.0, 1.0);
            (0.5 + 3.6 * t * t * t, gray(0x22).mix(gray(0x4a), t))
        }),
        Preset::Void => {}
    }
    Some(pix)
}

fn config_file() -> PathBuf {
    crate::config::path().with_file_name("wallpaper")
}

fn img_dirs() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    vec![home.join("Pictures/Wallpapers"), home.join("Pictures/wallpapers")]
}

fn to_pixmap(img: &Img) -> Option<Pixmap> {
    let mut px = img.px.clone();
    for p in px.chunks_exact_mut(4) {
        let a = p[3] as u16;
        for c in &mut p[..3] {
            *c = (*c as u16 * a / 255) as u8;
        }
    }
    Pixmap::from_vec(px, tiny_skia::IntSize::from_wh(img.w, img.h)?)
}

pub struct Wall {
    items: Vec<Item>,
    sel: usize,
    first: usize,
    current: Option<String>,
    /// Thumbnails at the current card size in buffer px: (size, pixmaps).
    thumbs: Arc<Mutex<(u32, Vec<Option<Pixmap>>)>>,
    asked: u32,
    /// Full-screen transition: when it started, and whether the wallpaper has been swapped yet.
    trans: Option<(f32, bool)>,
    hits: Vec<([f32; 4], usize)>,
    pointer: Option<(f32, f32)>,
    done: bool,
}

impl Wall {
    pub fn new() -> Self {
        let mut items: Vec<Item> = PRESETS.iter().map(|(p, n)| Item { name: n.to_string(), src: Source::Preset(*p) }).collect();
        let mut files: Vec<PathBuf> = img_dirs()
            .iter()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flatten()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|x| x.to_str()).is_some_and(|x| matches!(x.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")))
            .collect();
        files.sort();
        files.dedup();
        for f in files {
            let name = f.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
            items.push(Item { name, src: Source::Image(f) });
        }
        let current = std::fs::read_to_string(config_file()).ok().and_then(|s| s.lines().next().map(|l| l.trim().to_string()));
        let sel = current.as_ref().and_then(|c| items.iter().position(|i| Self::id(&i.src) == *c)).unwrap_or(0);
        Self { items, sel, first: 0, current, thumbs: Arc::default(), asked: 0, trans: None, hits: Vec::new(), pointer: None, done: false }
    }

    /// What the config file stores for a source: "preset:signal" or the image path.
    fn id(src: &Source) -> String {
        match src {
            Source::Preset(p) => format!("preset:{}", format!("{p:?}").to_lowercase()),
            Source::Image(p) => p.to_string_lossy().into_owned(),
        }
    }

    /// Renders every thumbnail at w×h px in the background (presets first, then images through ImageMagick).
    fn load_thumbs(&mut self, w: u32, h: u32) {
        if self.asked == w {
            return;
        }
        self.asked = w;
        let srcs: Vec<Source> = self.items.iter().map(|i| i.src.clone()).collect();
        let slot = self.thumbs.clone();
        *slot.lock().unwrap() = (w, vec![None; srcs.len()]);
        std::thread::spawn(move || {
            for (i, src) in srcs.into_iter().enumerate() {
                let pm = match src {
                    // Thumbnails exaggerate the pattern (bigger dots, wider pitch) so it reads at card size.
                    Source::Preset(p) => render(p, w, h, h as f32 / 1080.0 * 3.4),
                    Source::Image(path) => fx::thumb(Thumb::Path(path.to_string_lossy().into_owned()), w, h).and_then(|img| to_pixmap(&img)),
                };
                let mut g = slot.lock().unwrap();
                if g.0 == w {
                    g.1[i] = pm;
                }
            }
        });
    }

    /// Writes the full-size wallpaper (presets are rendered at the screen size), swaps swaybg, saves the choice.
    fn apply(&mut self, sw: u32, sh: u32) {
        let src = self.items[self.sel].src.clone();
        let file = match &src {
            Source::Image(p) => Some(p.clone()),
            Source::Preset(p) => {
                let dir = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/nothing");
                let _ = std::fs::create_dir_all(&dir);
                let out = dir.join(format!("wallpaper-{}.png", format!("{p:?}").to_lowercase()));
                render(*p, sw, sh, sh as f32 / 1080.0).and_then(|pm| pm.save_png(&out).ok()).map(|_| out)
            }
        };
        let Some(file) = file else {
            eprintln!("nothing-wall: could not prepare the wallpaper");
            return;
        };
        let _ = std::process::Command::new("pkill").args(["-x", "swaybg"]).status();
        crate::cc::run(&["swaybg", "-m", "fill", "-c", "#000000", "-i", &file.to_string_lossy()]);
        if let Some(d) = config_file().parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(config_file(), format!("{}\n{}\n", Self::id(&src), file.to_string_lossy()));
        self.current = Some(Self::id(&src));
    }

    fn choose(&mut self, now: f32, still: bool) -> Act {
        if still {
            self.trans = Some((now, false));
            self.done = true;
        } else {
            self.trans = Some((now, false));
        }
        Act::Fullscreen
    }
}

impl Widget for Wall {
    fn place(&self) -> Place {
        Place { anchor: Anchor::empty(), gap: 0, size: (W, H) }
    }

    fn key(&mut self, k: &Key, _m: Mods, now: f32) -> Act {
        if self.trans.is_some() {
            return Act::Stay;
        }
        let n = self.items.len();
        match k {
            Key::Esc => return Act::Close,
            Key::Left | Key::Up => self.sel = (self.sel + n - 1) % n,
            Key::Right | Key::Down | Key::Tab => self.sel = (self.sel + 1) % n,
            Key::Home => self.sel = 0,
            Key::End => self.sel = n - 1,
            Key::Enter => return self.choose(now, shell::turbo()),
            _ => {}
        }
        Act::Stay
    }

    fn pointer(&mut self, x: f32, y: f32, p: Ptr, now: f32) -> Act {
        if self.trans.is_some() {
            return Act::Stay;
        }
        self.pointer = (p != Ptr::Leave).then_some((x, y));
        let hit = self.hits.iter().find(|(r, _)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]).map(|h| h.1);
        match (p, hit) {
            (Ptr::Press(0x110), Some(i)) if i == self.sel => return self.choose(now, shell::turbo()),
            (Ptr::Press(0x110), Some(i)) => self.sel = i,
            (Ptr::Scroll(a), _) => {
                let n = self.items.len();
                self.sel = if a > 0.0 { (self.sel + 1) % n } else { (self.sel + n - 1) % n };
            }
            _ => {}
        }
        Act::Stay
    }

    fn tick(&mut self, now: f32) -> bool {
        match self.trans {
            Some((t0, _)) => now - t0 < CLOSE + HOLD + OPEN,
            None => false,
        }
    }

    fn period(&self) -> Option<f32> {
        let loading = self.thumbs.lock().unwrap().1.iter().any(Option::is_none);
        loading.then_some(0.1)
    }

    fn finished(&self) -> bool {
        self.done
    }

    fn draw(&mut self, cv: &mut Canvas, f: &Frame) {
        if let Some((t0, swapped)) = self.trans {
            // Full screen: close in (cells grow, edges first), hold black while swaybg swaps, open from the centre.
            let t = f.now - t0;
            let (w, h) = (f.w, f.h);
            let cell = 24.0 * f.s;
            let ink = Rgba::hex(0x000000);
            if self.done && !swapped {
                self.apply(w as u32, h as u32); // turbo: no transition
                self.trans = Some((t0, true));
                return;
            }
            if t < CLOSE {
                fx::dot_screen(cv, 0.0, 0.0, w, h, cell, 0.0, 1.0 - t / CLOSE, (0.5, 0.5), ink);
            } else if t < CLOSE + HOLD {
                cv.rect(0.0, 0.0, w, h, ink);
                if !swapped {
                    self.apply(w as u32, h as u32);
                    self.trans = Some((t0, true));
                }
                // A single red dot in the middle while the screen is dark.
                let d = 8.0 * f.s;
                cv.round_rect(w / 2.0 - d / 2.0, h / 2.0 - d / 2.0, d, d, d / 2.0, RED);
            } else if t < CLOSE + HOLD + OPEN {
                fx::dot_screen(cv, 0.0, 0.0, w, h, cell, 0.0, (t - CLOSE - HOLD) / OPEN, (0.5, 0.5), ink);
            } else {
                self.done = true;
            }
            return;
        }
        let s = f.s;
        let a = f.a;
        let (x0, y0) = (f.x + PAD * s, f.y + PAD * s);
        let iw = f.w - 2.0 * PAD * s;
        let base = y0 + 12.0 * s;
        fx::decode_text(cv, "WALLPAPER", x0, base, 12.0 * s, Weight::Bold, WHITE.alpha(a), f.decode_at(base), None, 0.0);
        let n = self.items.len();
        let total = format!(" / {n:02}");
        let tw = cv.fonts.width(Weight::Bold, 11.0 * s, &total);
        let num = format!("{:02}", self.sel + 1);
        let nw = cv.fonts.width(Weight::Bold, 11.0 * s, &num);
        cv.text(&num, x0 + iw - tw - nw, base, 11.0 * s, Weight::Bold, RED.alpha(a), None, 0.0);
        cv.text(&total, x0 + iw - tw, base, 11.0 * s, Weight::Bold, GREY.alpha(a), None, 0.0);

        let (cw, ch, gap) = (CW * s, CH * s, GAP * s);
        self.load_thumbs(cw as u32, ch as u32);
        let fit = ((iw + gap) / (cw + gap)) as usize;
        if self.sel < self.first {
            self.first = self.sel;
        } else if self.sel >= self.first + fit {
            self.first = self.sel + 1 - fit;
        }
        let cy = y0 + 36.0 * s;
        self.hits.clear();
        let thumbs = self.thumbs.lock().unwrap();
        for (k, i) in (self.first..n.min(self.first + fit)).enumerate() {
            let cx = x0 + k as f32 * (cw + gap);
            let r = 12.0 * s;
            let selected = i == self.sel;
            let lift = if selected { 4.0 * s } else { 0.0 };
            let y = cy - lift;
            match thumbs.1.get(i).and_then(|t| t.as_ref()) {
                Some(pm) => {
                    if let (Some(path), Some(pat)) = (
                        crate::canvas::round_rect_path(cx, y, cw, ch, r),
                        Some(tiny_skia::Pattern::new(pm.as_ref(), tiny_skia::SpreadMode::Pad, tiny_skia::FilterQuality::Bilinear, a * f.reveal, Transform::from_translate(cx, y))),
                    ) {
                        let p = Paint { shader: pat, anti_alias: true, ..Paint::default() };
                        cv.pix.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
                    }
                }
                None => cv.round_rect(cx, y, cw, ch, r, Rgba(1.0, 1.0, 1.0, 0.04 * a)),
            }
            let hovered = self.pointer.is_some_and(|(px, py)| px + f.x >= cx && px + f.x < cx + cw && py + f.y >= y && py + f.y < y + ch);
            let edge = if selected { WHITE } else if hovered { GREY } else { Rgba(1.0, 1.0, 1.0, 0.12) };
            cv.stroke_round_rect(cx + 0.5, y + 0.5, cw - 1.0, ch - 1.0, r, if selected { 2.0 * s } else { 1.0 }, edge.alpha(a));
            let item = &self.items[i];
            let lb = y + ch + 20.0 * s;
            let c = if selected { WHITE } else { GREY };
            fx::decode_text(cv, &item.name, cx + 2.0 * s, lb, 10.0 * s, Weight::Bold, c.alpha(a), f.decode_at(lb), Some((0.0, cx + cw - 50.0 * s)), 16.0 * s);
            if self.current.as_deref() == Some(Self::id(&item.src).as_str()) {
                let tag = "CURRENT";
                let tgw = cv.fonts.width(Weight::Regular, 8.5 * s, tag);
                cv.round_rect(cx + cw - tgw - 6.0 * s, lb - 6.5 * s, 5.0 * s, 5.0 * s, 2.5 * s, RED.alpha(a));
                cv.text(tag, cx + cw - tgw + 2.0 * s, lb, 8.5 * s, Weight::Regular, GREY.alpha(a), None, 0.0);
            }
            self.hits.push(([cx - f.x, y - f.y, cw, ch + 28.0 * s], i));
        }
        drop(thumbs);
        let fb = f.y + f.h - 16.0 * s;
        shell::hints(cv, &[("←/→", "choose"), ("ENTER", "apply"), ("ESC", "close")], x0, fb, s, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_render_dots_on_black() {
        for (p, _) in PRESETS {
            let pm = render(p, 320, 180, 0.5).unwrap();
            let lit = pm.data().chunks_exact(4).filter(|px| px[0] > 0 || px[1] > 0).count();
            assert_eq!(lit == 0, p == Preset::Void, "{p:?}");
        }
        // Signal has red in it; the plain grid does not.
        let red = |p| render(p, 320, 180, 0.5).unwrap().data().chunks_exact(4).any(|px| px[0] > 150 && px[1] < 60);
        assert!(red(Preset::Signal) && !red(Preset::Grid));
    }

    #[test]
    fn ids_round_trip_through_the_config_file() {
        assert_eq!(Wall::id(&Source::Preset(Preset::Signal)), "preset:signal");
        assert_eq!(Wall::id(&Source::Image(PathBuf::from("/w/a.png"))), "/w/a.png");
    }
}
