// Shared motion and drawing for every surface: the dot-matrix screen that opens cards, labels that decode left to
// right, and images drawn as a dot matrix (album art, clipboard images, wallpaper thumbnails).
use crate::anim::{decode, dot_cover};
use crate::bar::RED;
use crate::canvas::{Canvas, Rgba, Weight};

/// The dot-matrix screen over the rect (x, y, w, h) at reveal progress `p` (0 = covered, 1 = clear). Cells shrink to
/// dots and vanish along a sweep starting at `from` (fractions of the rect: (1, 0) = top right, (0.5, 0.5) = centre).
/// With `r` > 0, cells whose centre is outside the rect's rounded corners are left out.
#[allow(clippy::too_many_arguments)]
pub fn dot_screen(cv: &mut Canvas, x: f32, y: f32, w: f32, h: f32, cell: f32, r: f32, p: f32, from: (f32, f32), ink: Rgba) {
    if p >= 1.0 {
        return;
    }
    let (cols, rows) = ((w / cell).ceil() as usize, (h / cell).ceil() as usize);
    let (fx, fy) = (from.0 * w, from.1 * h);
    let span = fx.max(w - fx) + fy.max(h - fy);
    for j in 0..rows {
        for i in 0..cols {
            let (cx, cy) = ((i as f32 + 0.5) * cell, (j as f32 + 0.5) * cell);
            if cy > h {
                continue;
            }
            if r > 0.0 {
                let (dx, dy) = ((r - cx).max(cx - (w - r)).max(0.0), (r - cy).max(cy - (h - r)).max(0.0));
                if dx * dx + dy * dy > r * r {
                    continue;
                }
            }
            let phase = ((cx - fx).abs() + (cy - fy).abs()) / span.max(1.0);
            let size = cell * dot_cover(p, phase);
            if size > 0.5 {
                cv.rect(x + cx - size / 2.0, y + cy - size / 2.0, size, size, ink);
            }
        }
    }
}

/// Text decoding at progress `p`: written prefix, the red character being written, dim dots for the rest.
#[allow(clippy::too_many_arguments)]
pub fn decode_text(cv: &mut Canvas, t: &str, x: f32, base: f32, px: f32, w: Weight, c: Rgba, p: f32, clip: Option<(f32, f32)>, fade: f32) -> f32 {
    if p >= 1.0 {
        return cv.text(t, x, base, px, w, c, clip, fade);
    }
    let (head, cur, rest) = decode(t, p);
    let pen = cv.text(&head, x, base, px, w, c, clip, fade);
    let pen = cv.text(&cur, pen, base, px, w, RED.alpha(c.3), clip, fade);
    cv.text(&rest, pen, base, px, w, c.alpha(0.35), clip, fade)
}

/// An RGBA image (straight alpha, 8 bits per channel).
#[derive(Clone, Debug, PartialEq)]
pub struct Img {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Img {
    pub fn rgb(&self, x: u32, y: u32) -> (f32, f32, f32) {
        let i = ((y * self.w + x) * 4) as usize;
        (self.px[i] as f32 / 255.0, self.px[i + 1] as f32 / 255.0, self.px[i + 2] as f32 / 255.0)
    }
}

/// Dot radius (as a fraction of the cell) for a pixel's brightness: never quite gone, never touching.
pub fn dot_radius(luma: f32) -> f32 {
    0.5 * (0.18 + 0.72 * luma.clamp(0.0, 1.0))
}

/// Draws `img` as a dot matrix, one dot per pixel in a `cell`-px grid: bright pixels make big dots. `colour` 0 is
/// white dots, 1 the pixel's own colour.
#[allow(clippy::too_many_arguments)]
pub fn dot_image(cv: &mut Canvas, img: &Img, x: f32, y: f32, cell: f32, colour: f32, alpha: f32) {
    for j in 0..img.h {
        for i in 0..img.w {
            let (r, g, b) = img.rgb(i, j);
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let d = cell * 2.0 * dot_radius(luma);
            let mono = Rgba(1.0, 1.0, 1.0, 0.35 + 0.65 * luma);
            let c = mono.mix(Rgba(r, g, b, 1.0), colour).alpha(alpha);
            let (cx, cy) = (x + (i as f32 + 0.5) * cell, y + (j as f32 + 0.5) * cell);
            cv.round_rect(cx - d / 2.0, cy - d / 2.0, d, d, d / 2.0, c);
        }
    }
}

/// The image format from its first bytes. Only plain raster formats are ever handed to ImageMagick: art URLs and
/// clipboard contents are untrusted, and ImageMagick would otherwise sniff SVG/MVG/MSL or obey a `coder:` prefix.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.starts_with(b"\xff\xd8\xff") {
        Some("jpeg")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("gif")
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Decodes a PNG/JPEG/GIF/WebP file (absolute path) or bytes into a `w`×`h` RGBA thumbnail (centre-cropped to
/// fill). Anything else is refused.
pub fn thumb(input: Thumb, w: u32, h: u32) -> Option<Img> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    let src = match &input {
        Thumb::Path(p) => {
            if !p.starts_with('/') {
                return None;
            }
            let mut head = [0u8; 16];
            let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut head)).ok()?;
            format!("{}:{p}[0]", sniff(&head[..n])?)
        }
        Thumb::Bytes(b) => format!("{}:-[0]", sniff(b)?),
    };
    let size = format!("{w}x{h}");
    let mut child = Command::new("magick")
        .args([src.as_str(), "-resize", &format!("{size}^"), "-gravity", "center", "-extent", &size, "-depth", "8", "rgba:-"])
        .stdin(if matches!(input, Thumb::Bytes(_)) { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let writer = match input {
        Thumb::Bytes(b) => {
            let mut stdin = child.stdin.take()?;
            Some(std::thread::spawn(move || {
                let _ = stdin.write_all(&b);
            }))
        }
        Thumb::Path(_) => None,
    };
    let mut px = Vec::new();
    child.stdout.take()?.read_to_end(&mut px).ok()?;
    let _ = child.wait();
    if let Some(t) = writer {
        let _ = t.join();
    }
    (px.len() == (w * h * 4) as usize).then_some(Img { w, h, px })
}

pub enum Thumb {
    Path(String),
    Bytes(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_grow_with_brightness_and_stay_apart() {
        assert!(dot_radius(0.0) > 0.0);
        assert!(dot_radius(1.0) < 0.5);
        assert!(dot_radius(0.8) > dot_radius(0.2));
    }

    #[test]
    fn only_raster_formats_are_decoded() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff(b"<svg xmlns="), None);
        assert_eq!(sniff(b"push graphic-context"), None); // MVG
        assert!(thumb(Thumb::Bytes(b"P6\n1 1\n255\n\xff\x00\x00".to_vec()), 1, 1).is_none()); // PPM: not allowed
        assert!(thumb(Thumb::Path("msl:/tmp/x".into()), 1, 1).is_none());
    }

    #[test]
    fn thumbnails_decode_through_imagemagick() {
        // A 2x1 PNG: red then blue, scaled to 4x2.
        let mut pm = tiny_skia::Pixmap::new(2, 1).unwrap();
        pm.data_mut().copy_from_slice(&[255, 0, 0, 255, 0, 0, 255, 255]);
        let png = pm.encode_png().unwrap();
        let Some(img) = thumb(Thumb::Bytes(png), 4, 2) else {
            eprintln!("magick not installed: skipped");
            return;
        };
        assert_eq!((img.w, img.h, img.px.len()), (4, 2, 32));
        assert!(img.rgb(0, 0).0 > 0.9 && img.rgb(3, 1).2 > 0.9);
    }
}
