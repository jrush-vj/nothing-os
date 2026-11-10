// Line icons: the same 24x24 SVG paths as the Bar Lab previews, parsed once and stroked with tiny-skia.
// Supports the path commands those icons use: M L H V C A Z, absolute and relative, with implicit repeats.
use crate::canvas::{Canvas, Rgba};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Stroke, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Wifi,
    WifiOff,
    Eth,
    Vol,
    Mute,
    Sun,
    Bolt,
    Gpu,
    Bell,
    Play,
    Pause,
    Prev,
    Next,
    Bt,
    Mic,
    MicOff,
}

enum Part {
    Stroke(&'static str),
    Fill(&'static str),
    Circle(f32, f32, f32, bool),
    Rect(f32, f32, f32, f32, f32),
}

fn parts(i: Icon) -> &'static [Part] {
    use Part::*;
    match i {
        Icon::Wifi => &[Stroke("M2 8.5a15 15 0 0 1 20 0M5.5 12.5a10 10 0 0 1 13 0M9 16.3a5 5 0 0 1 6 0"), Circle(12.0, 19.5, 1.2, true)],
        Icon::WifiOff => &[Stroke("M9 16.3a5 5 0 0 1 6 0M3 3l18 18"), Circle(12.0, 19.5, 1.2, true)],
        Icon::Eth => &[Rect(4.0, 9.0, 16.0, 10.0, 1.5), Stroke("M8 9V6h8v3M8 13v2M12 13v2M16 13v2")],
        Icon::Vol => &[Stroke("M4 9h4l5-4v14l-5-4H4z"), Stroke("M16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12")],
        Icon::Mute => &[Stroke("M4 9h4l5-4v14l-5-4H4z"), Stroke("M16 9l6 6M22 9l-6 6")],
        Icon::Sun => &[Circle(12.0, 12.0, 4.0, false), Stroke("M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5L19 19M5 19l1.5-1.5M17.5 6.5L19 5")],
        Icon::Bolt => &[Fill("M13 2L4 14h7l-1 8 9-12h-7z")],
        Icon::Gpu => &[Rect(3.0, 7.0, 18.0, 10.0, 1.5), Circle(9.0, 12.0, 2.5, false), Stroke("M14 10h4M14 14h4")],
        Icon::Bell => &[Stroke("M6 16V11a6 6 0 0 1 12 0v5l2 2H4z"), Stroke("M10 20.5h4")],
        Icon::Play => &[Fill("M7 5l12 7-12 7z")],
        Icon::Pause => &[Rect(6.0, 5.0, 4.0, 14.0, 0.0), Rect(14.0, 5.0, 4.0, 14.0, 0.0)],
        Icon::Prev => &[Fill("M19 5L9 12l10 7zM5 5h3v14H5z")],
        Icon::Next => &[Fill("M5 5l10 7-10 7zM16 5h3v14h-3z")],
        Icon::Bt => &[Stroke("M7 7l10 10-5 4V3l5 4L7 17")],
        Icon::Mic => &[Rect(9.0, 3.0, 6.0, 11.0, 3.0), Stroke("M5 11a7 7 0 0 0 14 0M12 18v3")],
        Icon::MicOff => &[Rect(9.0, 3.0, 6.0, 11.0, 3.0), Stroke("M5 11a7 7 0 0 0 14 0M12 18v3M3 3l18 18")],
    }
}

/// Draws `icon` in a `size`×`size` box with its top-left at (x, y).
pub fn draw(cv: &mut Canvas, icon: Icon, x: f32, y: f32, size: f32, c: Rgba) {
    let k = size / 24.0;
    let t = Transform::from_row(k, 0.0, 0.0, k, x, y);
    let mut paint = Paint::default();
    paint.set_color(c.skia());
    paint.anti_alias = true;
    let stroke = Stroke { width: 2.4, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
    for p in parts(icon) {
        match p {
            Part::Stroke(d) => {
                if let Some(path) = parse(d) {
                    cv.pix.stroke_path(&path, &paint, &stroke, t, None);
                }
            }
            Part::Fill(d) => {
                if let Some(path) = parse(d) {
                    cv.pix.fill_path(&path, &paint, FillRule::Winding, t, None);
                }
            }
            Part::Circle(cx, cy, r, filled) => {
                if let Some(path) = PathBuilder::from_circle(*cx, *cy, *r) {
                    if *filled {
                        cv.pix.fill_path(&path, &paint, FillRule::Winding, t, None);
                    } else {
                        cv.pix.stroke_path(&path, &paint, &stroke, t, None);
                    }
                }
            }
            Part::Rect(rx, ry, rw, rh, r) => {
                if let Some(path) = crate::canvas::round_rect_path(*rx, *ry, *rw, *rh, *r) {
                    // The pause bars are solid; the other rects are outlines.
                    if matches!(icon, Icon::Pause) {
                        cv.pix.fill_path(&path, &paint, FillRule::Winding, t, None);
                    } else {
                        cv.pix.stroke_path(&path, &paint, &stroke, t, None);
                    }
                }
            }
        }
    }
}

fn tokens(d: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c.is_ascii_digit() || c == '-' || c == '.' {
            let start = i;
            i += 1;
            let mut dot = c == '.';
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() || (ch == '.' && !dot) {
                    dot |= ch == '.';
                    i += 1;
                } else {
                    break;
                }
            }
            if let Ok(v) = d[start..i].parse() {
                out.push(Tok::Num(v));
            }
        } else {
            i += 1;
        }
    }
    out
}

#[derive(Debug, PartialEq)]
enum Tok {
    Cmd(char),
    Num(f32),
}

/// Parses SVG path data into a tiny-skia path.
pub fn parse(d: &str) -> Option<Path> {
    let toks = tokens(d);
    let mut pb = PathBuilder::new();
    let (mut cx, mut cy, mut sx, mut sy) = (0f32, 0f32, 0f32, 0f32);
    let mut i = 0;
    let mut cmd = 'M';
    let num = |i: &mut usize| -> Option<f32> {
        match toks.get(*i) {
            Some(Tok::Num(v)) => {
                *i += 1;
                Some(*v)
            }
            _ => None,
        }
    };
    while i < toks.len() {
        if let Tok::Cmd(c) = toks[i] {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                pb.close();
                (cx, cy) = (sx, sy);
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (x, y) = (num(&mut i)? + ox, num(&mut i)? + oy);
                pb.move_to(x, y);
                (cx, cy, sx, sy) = (x, y, x, y);
                cmd = if rel { 'l' } else { 'L' }; // further pairs are line-tos
            }
            'L' => {
                let (x, y) = (num(&mut i)? + ox, num(&mut i)? + oy);
                pb.line_to(x, y);
                (cx, cy) = (x, y);
            }
            'H' => {
                cx = num(&mut i)? + ox;
                pb.line_to(cx, cy);
            }
            'V' => {
                cy = num(&mut i)? + oy;
                pb.line_to(cx, cy);
            }
            'C' => {
                let v: Vec<f32> = (0..6).map(|_| num(&mut i)).collect::<Option<_>>()?;
                pb.cubic_to(v[0] + ox, v[1] + oy, v[2] + ox, v[3] + oy, v[4] + ox, v[5] + oy);
                (cx, cy) = (v[4] + ox, v[5] + oy);
            }
            'A' => {
                let v: Vec<f32> = (0..7).map(|_| num(&mut i)).collect::<Option<_>>()?;
                let (x, y) = (v[5] + ox, v[6] + oy);
                arc(&mut pb, (cx, cy), v[0], v[1], v[2], v[3] != 0.0, v[4] != 0.0, (x, y));
                (cx, cy) = (x, y);
            }
            _ => return None,
        }
    }
    pb.finish()
}

/// SVG elliptical arc (endpoint form) as cubic Béziers, one per quarter turn at most.
#[allow(clippy::too_many_arguments)]
fn arc(pb: &mut PathBuilder, p0: (f32, f32), rx: f32, ry: f32, rot_deg: f32, large: bool, sweep: bool, p1: (f32, f32)) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 || p0 == p1 {
        pb.line_to(p1.0, p1.1);
        return;
    }
    let phi = rot_deg.to_radians();
    let (sin, cos) = phi.sin_cos();
    let (dx, dy) = ((p0.0 - p1.0) / 2.0, (p0.1 - p1.1) / 2.0);
    let (x1, y1) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        (rx, ry) = (rx * lambda.sqrt(), ry * lambda.sqrt());
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = (num / den).max(0.0).sqrt();
    if large == sweep {
        co = -co;
    }
    let (cxp, cyp) = (co * rx * y1 / ry, -co * ry * x1 / rx);
    let (cx, cy) = (cos * cxp - sin * cyp + (p0.0 + p1.0) / 2.0, sin * cxp + cos * cyp + (p0.1 + p1.1) / 2.0);
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let a = (ux * vx + uy * vy) / ((ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt());
        let s = if ux * vy - uy * vx < 0.0 { -1.0 } else { 1.0 };
        s * a.clamp(-1.0, 1.0).acos()
    };
    let t1 = ang(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut dt = ang((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    let n = (dt.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = dt / n as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let pt = |t: f32| {
        let (s, c) = t.sin_cos();
        (cx + rx * c * cos - ry * s * sin, cy + rx * c * sin + ry * s * cos)
    };
    let dpt = |t: f32| {
        let (s, c) = t.sin_cos();
        (-rx * s * cos - ry * c * sin, -rx * s * sin + ry * c * cos)
    };
    let mut t = t1;
    for _ in 0..n {
        let (a, da) = (pt(t), dpt(t));
        let (b, db) = (pt(t + step), dpt(t + step));
        pb.cubic_to(a.0 + k * da.0, a.1 + k * da.1, b.0 - k * db.0, b.1 - k * db.1, b.0, b.1);
        t += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_compact_numbers() {
        assert_eq!(tokens("M5 5l1.5-1.5.5"), vec![Tok::Cmd('M'), Tok::Num(5.0), Tok::Num(5.0), Tok::Cmd('l'), Tok::Num(1.5), Tok::Num(-1.5), Tok::Num(0.5)]);
    }

    #[test]
    fn implicit_repeats_and_relative_moves() {
        // "l6 6 6-6": two relative line-tos -> ends at (18, 9)
        let p = parse("M6 9l6 6 6-6").unwrap();
        let b = p.bounds();
        assert_eq!((b.left(), b.right(), b.top(), b.bottom()), (6.0, 18.0, 9.0, 15.0));
    }

    #[test]
    fn half_circle_arc_bulges_the_right_way() {
        // From (2, 8.5) to (22, 8.5) with r=15, sweep=1: an arc bulging upwards (smaller y).
        let p = parse("M2 8.5a15 15 0 0 1 20 0").unwrap();
        let b = p.bounds();
        assert!((b.left() - 2.0).abs() < 0.01 && (b.right() - 22.0).abs() < 0.01);
        assert!(b.top() < 8.5 - 3.0 && (b.bottom() - 8.5).abs() < 0.01, "{b:?}");
    }

    #[test]
    fn every_icon_parses() {
        for icon in [Icon::Wifi, Icon::WifiOff, Icon::Eth, Icon::Vol, Icon::Mute, Icon::Sun, Icon::Bolt, Icon::Gpu, Icon::Bell, Icon::Play, Icon::Pause, Icon::Prev, Icon::Next, Icon::Bt, Icon::Mic, Icon::MicOff] {
            for p in parts(icon) {
                if let Part::Stroke(d) | Part::Fill(d) = p {
                    assert!(parse(d).is_some(), "{icon:?}: {d}");
                }
            }
        }
    }
}
