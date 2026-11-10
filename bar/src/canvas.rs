// Drawing: a tiny-skia pixmap plus JetBrains Mono text. Glyph coverage is rasterised once per (char, size,
// sub-pixel offset) and cached, so a frame of the bar is mostly memcpy-sized work.
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use std::collections::HashMap;
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Weight {
    Regular,
    Bold,
}

/// A colour with straight (not premultiplied) alpha, 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub f32, pub f32, pub f32, pub f32);

impl Rgba {
    pub const fn hex(v: u32) -> Self {
        Self(((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, 1.0)
    }
    pub fn alpha(self, a: f32) -> Self {
        Self(self.0, self.1, self.2, self.3 * a)
    }
    pub fn mix(self, o: Self, t: f32) -> Self {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Self(l(self.0, o.0), l(self.1, o.1), l(self.2, o.2), l(self.3, o.3))
    }
    pub fn skia(self) -> Color {
        Color::from_rgba(self.0, self.1, self.2, self.3.clamp(0.0, 1.0)).unwrap_or(Color::TRANSPARENT)
    }
}

struct Glyph {
    left: i32,
    top: i32,
    w: usize,
    h: usize,
    coverage: Vec<u8>,
}

pub struct Fonts {
    regular: FontVec,
    bold: FontVec,
    cache: HashMap<(char, Weight, u32, u8), Option<Glyph>>,
}

/// Finds a font file through fontconfig, so it follows whatever is installed.
fn font_file(pattern: &str) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new("fc-match").args(["-f", "%{file}", pattern]).output().map_err(|e| format!("fc-match: {e}"))?;
    let path = String::from_utf8_lossy(&out.stdout).into_owned();
    std::fs::read(&path).map_err(|e| format!("{pattern} -> {path}: {e}"))
}

impl Fonts {
    pub fn load(family: &str) -> Result<Self, String> {
        let load = |style: &str| -> Result<FontVec, String> {
            FontVec::try_from_vec(font_file(&format!("{family}:style={style}"))?).map_err(|e| e.to_string())
        };
        Ok(Self { regular: load("Regular")?, bold: load("Bold")?, cache: HashMap::new() })
    }

    fn font(&self, w: Weight) -> &FontVec {
        match w {
            Weight::Regular => &self.regular,
            Weight::Bold => &self.bold,
        }
    }

    pub fn advance(&self, w: Weight, px: f32, c: char) -> f32 {
        let f = self.font(w).as_scaled(PxScale::from(px));
        f.h_advance(f.glyph_id(c))
    }

    pub fn width(&self, w: Weight, px: f32, s: &str) -> f32 {
        s.chars().map(|c| self.advance(w, px, c)).sum()
    }

    /// Ascent of capitals/digits, used to centre text vertically.
    pub fn cap_height(&self, w: Weight, px: f32) -> f32 {
        let f = self.font(w).as_scaled(PxScale::from(px));
        f.ascent() * 0.72
    }

    fn glyph(&mut self, w: Weight, px: f32, c: char, frac: u8) -> Option<&Glyph> {
        let key = (c, w, (px * 4.0).round() as u32, frac);
        if !self.cache.contains_key(&key) {
            let font = self.font(w);
            let id = font.glyph_id(c);
            let g = id.with_scale_and_position(PxScale::from(key.2 as f32 / 4.0), ab_glyph::point(frac as f32 / 4.0, 0.0));
            let rendered = font.outline_glyph(g).map(|og| {
                let b = og.px_bounds();
                let (w, h) = (b.width() as usize, b.height() as usize);
                let mut coverage = vec![0u8; w * h];
                og.draw(|x, y, v| {
                    if let Some(p) = coverage.get_mut(y as usize * w + x as usize) {
                        *p = (v.clamp(0.0, 1.0) * 255.0) as u8;
                    }
                });
                Glyph { left: b.min.x as i32, top: b.min.y as i32, w, h, coverage }
            });
            self.cache.insert(key, rendered);
        }
        self.cache[&key].as_ref()
    }
}

pub struct Canvas {
    pub pix: Pixmap,
    pub fonts: Fonts,
}

impl Canvas {
    pub fn new(w: u32, h: u32, fonts: Fonts) -> Self {
        Self { pix: Pixmap::new(w.max(1), h.max(1)).unwrap(), fonts }
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if (self.pix.width(), self.pix.height()) != (w, h) {
            self.pix = Pixmap::new(w.max(1), h.max(1)).unwrap();
        }
    }

    pub fn clear(&mut self, c: Rgba) {
        self.pix.fill(c.skia());
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        if let Some(r) = Rect::from_xywh(x, y, w, h) {
            let mut p = Paint::default();
            p.set_color(c.skia());
            p.anti_alias = true;
            self.pix.fill_rect(r, &p, Transform::identity(), None);
        }
    }

    pub fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
        if let Some(path) = round_rect_path(x, y, w, h, r) {
            let mut p = Paint::default();
            p.set_color(c.skia());
            p.anti_alias = true;
            self.pix.fill_path(&path, &p, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
    }

    pub fn stroke_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: Rgba) {
        if let Some(path) = round_rect_path(x, y, w, h, r) {
            let mut p = Paint::default();
            p.set_color(c.skia());
            p.anti_alias = true;
            let stroke = tiny_skia::Stroke { width, ..tiny_skia::Stroke::default() };
            self.pix.stroke_path(&path, &p, &stroke, Transform::identity(), None);
        }
    }

    /// A horizontal 1 px line whose alpha fades out towards both ends (the hairline under the bar).
    pub fn hairline(&mut self, y: f32, x0: f32, x1: f32, c: Rgba) {
        use tiny_skia::{GradientStop, LinearGradient, Point, SpreadMode};
        let stops = vec![
            GradientStop::new(0.0, c.alpha(0.0).skia()),
            GradientStop::new(0.5, c.skia()),
            GradientStop::new(1.0, c.alpha(0.0).skia()),
        ];
        if let (Some(shader), Some(r)) = (
            LinearGradient::new(Point::from_xy(x0, y), Point::from_xy(x1, y), stops, SpreadMode::Pad, Transform::identity()),
            Rect::from_xywh(x0, y, x1 - x0, 1.0),
        ) {
            let p = Paint { shader, ..Paint::default() };
            self.pix.fill_rect(r, &p, Transform::identity(), None);
        }
    }

    /// Draws `s` with its baseline at `y`, starting at `x`; returns the x after the last glyph. Pixels outside
    /// `clip` (x0, x1) are skipped, and `fade` fades the last `fade` px before x1 (for long titles).
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, s: &str, x: f32, y: f32, px: f32, w: Weight, c: Rgba, clip: Option<(f32, f32)>, fade: f32) -> f32 {
        let mut pen = x;
        let (cx0, cx1) = clip.unwrap_or((f32::MIN, f32::MAX));
        for ch in s.chars() {
            let adv = self.fonts.advance(w, px, ch);
            let ix = pen.floor();
            let frac = ((pen - ix) * 4.0).round() as u8 % 4;
            if pen < cx1
                && let Some(g) = self.fonts.glyph(w, px, ch, frac)
            {
                blend(&mut self.pix, g, ix as i32, y.round() as i32, c, cx0, cx1, fade);
            }
            pen += adv;
        }
        pen
    }
}

pub fn round_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = tiny_skia::PathBuilder::new();
    let k = 0.5523 * r; // cubic approximation of a quarter circle
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Source-over of a coverage mask in colour `c` onto premultiplied RGBA pixels.
#[allow(clippy::too_many_arguments)]
fn blend(pix: &mut Pixmap, g: &Glyph, ox: i32, baseline: i32, c: Rgba, cx0: f32, cx1: f32, fade: f32) {
    let (pw, ph) = (pix.width() as i32, pix.height() as i32);
    let data = pix.data_mut();
    for gy in 0..g.h as i32 {
        let py = baseline + g.top + gy;
        if py < 0 || py >= ph {
            continue;
        }
        for gx in 0..g.w as i32 {
            let px = ox + g.left + gx;
            if px < 0 || px >= pw || (px as f32) < cx0 || (px as f32) >= cx1 {
                continue;
            }
            let cov = g.coverage[(gy * g.w as i32 + gx) as usize];
            if cov == 0 {
                continue;
            }
            let edge = if fade > 0.0 { ((cx1 - px as f32) / fade).clamp(0.0, 1.0) } else { 1.0 };
            let a = cov as f32 / 255.0 * c.3 * edge;
            let i = ((py * pw + px) * 4) as usize;
            let inv = 1.0 - a;
            for (k, v) in [c.0, c.1, c.2].into_iter().enumerate() {
                data[i + k] = (v * a * 255.0 + data[i + k] as f32 * inv).round() as u8;
            }
            data[i + 3] = (a * 255.0 + data[i + 3] as f32 * inv).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_mix() {
        let red = Rgba::hex(0xd71921);
        assert!((red.0 - 215.0 / 255.0).abs() < 1e-6 && red.3 == 1.0);
        let m = Rgba::hex(0x000000).mix(Rgba::hex(0xffffff), 0.5);
        assert!((m.0 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn round_rect_is_closed_and_bounded() {
        let p = round_rect_path(2.0, 3.0, 40.0, 20.0, 22.0).unwrap();
        let b = p.bounds();
        assert!((b.left() - 2.0).abs() < 0.01 && (b.bottom() - 23.0).abs() < 0.01);
    }
}
