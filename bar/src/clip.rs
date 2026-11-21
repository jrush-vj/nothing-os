// Clipboard history (`nothing-bar clip`, SUPER+V) over cliphist: type to filter, ↑/↓ to pick, Enter copies, Del
// deletes, Ctrl+P pins (pinned entries stay on top; ids in ~/.local/share/nothing-bar/pins). The selected entry is
// previewed on the right: text in full, images as a dot matrix (decoded by ImageMagick in the background).
use crate::bar::{GREY, HOVER, RED, WHITE};
use crate::canvas::{Canvas, Rgba, Weight};
use crate::fx::{self, Img, Thumb};
use crate::shell::{self, Act, Frame, Key, Mods, Place, Ptr, Widget, DIM};
use crate::text::TextBuf;
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const W: f32 = 640.0;
const H: f32 = 440.0;
const PAD: f32 = 18.0;
const ROW: f32 = 28.0;
const LIST_W: f32 = 360.0;
const THUMB: u32 = 40;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The whole `cliphist list` line (what decode/delete read on stdin).
    pub line: String,
    pub id: u64,
    pub preview: String,
    pub image: Option<String>,
}

/// One `cliphist list` line -> an entry. Images look like "[[ binary data 31 KiB png 640x480 ]]".
pub fn parse_line(line: &str) -> Option<Entry> {
    let (id, preview) = line.split_once('\t')?;
    let id = id.trim().parse().ok()?;
    let image = preview.strip_prefix("[[ binary data ").and_then(|r| r.strip_suffix(" ]]")).map(|r| {
        // "31 KiB png 640x480" -> "PNG · 640×480 · 31 KIB"
        let f: Vec<&str> = r.split_whitespace().collect();
        match f.as_slice() {
            [n, unit, kind, dims] => format!("{} · {} · {n} {}", kind.to_uppercase(), dims.replace('x', "×"), unit.to_uppercase()),
            _ => r.to_uppercase(),
        }
    });
    Some(Entry { line: line.to_string(), id, preview: preview.to_string(), image })
}

/// Pinned first (in pin order), then the rest newest first; only entries matching `q` (case-insensitive).
pub fn visible(entries: &[Entry], pins: &[u64], q: &str) -> Vec<usize> {
    let q = q.to_lowercase();
    let hit = |e: &Entry| q.is_empty() || e.preview.to_lowercase().contains(&q) || e.image.as_ref().is_some_and(|i| i.to_lowercase().contains(&q));
    let mut out: Vec<usize> = pins.iter().filter_map(|p| entries.iter().position(|e| e.id == *p)).filter(|&i| hit(&entries[i])).collect();
    out.extend((0..entries.len()).filter(|i| !pins.contains(&entries[*i].id) && hit(&entries[*i])));
    out
}

enum Loaded {
    Text(String),
    Image(Option<Img>),
}

pub struct Clip {
    entries: Vec<Entry>,
    pins: Vec<u64>,
    query: TextBuf,
    sel: usize,
    top: usize,
    loaded: Arc<Mutex<HashMap<u64, Loaded>>>,
    asked: Vec<u64>,
    hits: Vec<([f32; 4], usize)>,
    pointer: Option<(f32, f32)>,
    rows: usize,
}

fn pins_file() -> std::path::PathBuf {
    crate::notes::dir().with_file_name("pins")
}

fn run_stdin(args: &[&str], input: &str) -> Option<Vec<u8>> {
    let mut c = Command::new(args[0]).args(&args[1..]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    c.stdin.take()?.write_all(input.as_bytes()).ok()?;
    Some(c.wait_with_output().ok()?.stdout)
}

impl Clip {
    pub fn new() -> Self {
        let list = Command::new("cliphist").arg("list").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        Self::with(list.lines().filter_map(parse_line).collect())
    }

    pub fn with(entries: Vec<Entry>) -> Self {
        let pins: Vec<u64> = std::fs::read_to_string(pins_file()).unwrap_or_default().lines().filter_map(|l| l.trim().parse().ok()).collect();
        Self { entries, pins, query: TextBuf::default(), sel: 0, top: 0, loaded: Arc::default(), asked: Vec::new(), hits: Vec::new(), pointer: None, rows: 10 }
    }

    fn shown(&self) -> Vec<usize> {
        visible(&self.entries, &self.pins, &self.query.text)
    }

    fn selected(&self) -> Option<&Entry> {
        self.shown().get(self.sel).map(|&i| &self.entries[i])
    }

    fn save_pins(&self) {
        let _ = std::fs::create_dir_all(pins_file().parent().unwrap_or(std::path::Path::new("/tmp")));
        let text: String = self.pins.iter().map(|p| format!("{p}\n")).collect();
        let _ = std::fs::write(pins_file(), text);
    }

    /// Copies the selected entry back to the clipboard: `cliphist decode | wl-copy`, without a shell.
    fn copy(&self) -> bool {
        let Some(e) = self.selected() else { return false };
        let Ok(mut dec) = Command::new("cliphist").arg("decode").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
            return false;
        };
        if let Some(mut i) = dec.stdin.take() {
            let _ = i.write_all(e.line.as_bytes());
        }
        let Some(out) = dec.stdout.take() else { return false };
        let ok = Command::new("wl-copy").stdin(Stdio::from(out)).spawn().map(|mut c| c.wait()).is_ok();
        let _ = dec.wait();
        ok
    }

    fn delete(&mut self) {
        let Some(e) = self.selected().cloned() else { return };
        let _ = run_stdin(&["cliphist", "delete"], &e.line);
        self.entries.retain(|x| x.id != e.id);
        self.pins.retain(|p| *p != e.id);
        self.save_pins();
        self.sel = self.sel.min(self.shown().len().saturating_sub(1));
    }

    fn toggle_pin(&mut self) {
        let Some(id) = self.selected().map(|e| e.id) else { return };
        if let Some(i) = self.pins.iter().position(|p| *p == id) {
            self.pins.remove(i);
        } else {
            self.pins.insert(0, id);
        }
        self.save_pins();
        self.sel = self.shown().iter().position(|&i| self.entries[i].id == id).unwrap_or(0);
    }

    /// Starts decoding the selected entry for the preview (once per entry).
    fn request_preview(&mut self) {
        let Some(e) = self.selected().cloned() else { return };
        if self.asked.contains(&e.id) {
            return;
        }
        self.asked.push(e.id);
        let loaded = self.loaded.clone();
        std::thread::spawn(move || {
            let bytes = run_stdin(&["cliphist", "decode"], &e.line).unwrap_or_default();
            let v = if e.image.is_some() {
                Loaded::Image(fx::thumb(Thumb::Bytes(bytes), THUMB, THUMB))
            } else {
                let mut t = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).into_owned();
                t.retain(|c| c != '\r');
                Loaded::Text(t)
            };
            loaded.lock().unwrap().insert(e.id, v);
        });
    }

    fn step(&mut self, d: i32) {
        let n = self.shown().len() as i32;
        if n > 0 {
            self.sel = (self.sel as i32 + d).clamp(0, n - 1) as usize;
        }
    }
}

impl Widget for Clip {
    fn place(&self) -> Place {
        Place { anchor: Anchor::empty(), gap: 0, size: (W, H) }
    }

    fn key(&mut self, k: &Key, _m: Mods, _now: f32) -> Act {
        match k {
            Key::Esc => return Act::Close,
            Key::Enter => return if self.copy() { Act::Close } else { Act::Stay },
            Key::Up => self.step(-1),
            Key::Down | Key::Tab => self.step(1),
            Key::PageUp => self.step(-(self.rows as i32)),
            Key::PageDown => self.step(self.rows as i32),
            Key::Delete => self.delete(),
            Key::Ctrl('p') => self.toggle_pin(),
            Key::Ctrl('u') => {
                self.query = TextBuf::default();
                self.sel = 0;
            }
            Key::Text(t) => {
                self.query.insert(t);
                self.sel = 0;
            }
            Key::Back => {
                self.query.backspace();
                self.sel = 0;
            }
            Key::BackWord => {
                self.query.backspace_word();
                self.sel = 0;
            }
            _ => {}
        }
        Act::Stay
    }

    fn pointer(&mut self, x: f32, y: f32, p: Ptr, _now: f32) -> Act {
        self.pointer = (p != Ptr::Leave).then_some((x, y));
        let hit = self.hits.iter().find(|(r, _)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]).map(|h| h.1);
        match (p, hit) {
            (Ptr::Press(0x110), Some(i)) if i == self.sel => return if self.copy() { Act::Close } else { Act::Stay },
            (Ptr::Press(0x110), Some(i)) => self.sel = i,
            (Ptr::Scroll(a), _) => self.step(if a > 0.0 { 1 } else { -1 }),
            _ => {}
        }
        Act::Stay
    }

    fn period(&self) -> Option<f32> {
        // Poll for background previews while one is pending.
        let pending = self.selected().is_some_and(|e| !self.loaded.lock().unwrap().contains_key(&e.id));
        Some(if pending { 0.05 } else { 0.5 })
    }

    fn draw(&mut self, cv: &mut Canvas, f: &Frame) {
        self.request_preview();
        let s = f.s;
        let (x0, y0) = (f.x + PAD * s, f.y + PAD * s);
        let iw = f.w - 2.0 * PAD * s;
        let a = f.a;
        // Header: title, the search field, the count.
        let base = y0 + 12.0 * s;
        let pen = fx::decode_text(cv, "CLIPBOARD", x0, base, 12.0 * s, Weight::Bold, WHITE.alpha(a), f.decode_at(base), None, 0.0);
        let qx = pen + 18.0 * s;
        let shown = self.shown();
        if self.query.text.is_empty() {
            cv.text("TYPE TO SEARCH", qx + 4.0 * s, base, 11.0 * s, Weight::Regular, DIM.alpha(a), None, 0.0);
        } else {
            cv.text(&self.query.text, qx, base, 11.0 * s, Weight::Regular, WHITE.alpha(a), None, 0.0);
        }
        let cxp = qx + cv.fonts.width(Weight::Regular, 11.0 * s, &self.query.text);
        if (f.now * 2.0) as u32 % 2 == 0 || !self.query.text.is_empty() {
            cv.rect(cxp.round() + 1.0, base - 11.0 * s, (1.5 * s).max(1.0), 14.0 * s, RED.alpha(a));
        }
        let count = format!("{} ITEMS", shown.len());
        let cw = cv.fonts.width(Weight::Regular, 9.5 * s, &count);
        cv.text(&count, x0 + iw - cw, base, 9.5 * s, Weight::Regular, GREY.alpha(a), None, 0.0);
        let line_y = y0 + 30.0 * s;
        cv.rect(x0, line_y, iw, 1.0, Rgba(1.0, 1.0, 1.0, 0.08 * a));

        // The list
        let ly = line_y + 10.0 * s;
        let lw = LIST_W * s;
        let rh = ROW * s;
        let bottom = f.y + f.h - 36.0 * s;
        self.rows = (((bottom - ly) / rh) as usize).max(1);
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + self.rows {
            self.top = self.sel + 1 - self.rows;
        }
        self.hits.clear();
        if shown.is_empty() {
            let msg = if self.entries.is_empty() { "NOTHING COPIED YET" } else { "NO MATCHES" };
            let b = ly + rh * 0.65;
            fx::decode_text(cv, msg, x0 + 8.0 * s, b, 11.0 * s, Weight::Regular, GREY.alpha(a), f.decode_at(b), None, 0.0);
        }
        for (row, &i) in shown.iter().enumerate().skip(self.top).take(self.rows) {
            let e = &self.entries[i];
            let y = ly + (row - self.top) as f32 * rh;
            let r = [x0 - 6.0 * s, y, lw, rh];
            let hovered = self.pointer.is_some_and(|(px, py)| px + f.x >= r[0] && px + f.x < r[0] + r[2] && py + f.y >= r[1] && py + f.y < r[1] + r[3]);
            if row == self.sel {
                cv.round_rect(r[0], r[1] + 2.0 * s, r[2], r[3] - 4.0 * s, 9.0 * s, HOVER.alpha(a));
                cv.rect(r[0] + 1.0 * s, r[1] + 8.0 * s, 2.0 * s, r[3] - 16.0 * s, RED.alpha(a));
            } else if hovered {
                cv.round_rect(r[0], r[1] + 2.0 * s, r[2], r[3] - 4.0 * s, 9.0 * s, HOVER.alpha(0.6 * a));
            }
            let b = (y + (rh + cv.fonts.cap_height(Weight::Regular, 11.0 * s)) / 2.0).round();
            let mut tx = x0 + 6.0 * s;
            if self.pins.contains(&e.id) {
                cv.round_rect(tx, b - 6.0 * s, 5.0 * s, 5.0 * s, 2.5 * s, RED.alpha(a));
            }
            tx += 12.0 * s;
            let (label, c) = match &e.image {
                Some(info) => (format!("IMAGE · {info}"), GREY),
                None => (e.preview.split_whitespace().collect::<Vec<_>>().join(" "), if row == self.sel { WHITE } else { Rgba::hex(0xcccccc) }),
            };
            fx::decode_text(cv, &label, tx, b, 11.0 * s, Weight::Regular, c.alpha(a), f.decode_at(b), Some((0.0, r[0] + r[2] - 8.0 * s)), 22.0 * s);
            self.hits.push(([r[0] - f.x, r[1] - f.y, r[2], r[3]], row));
        }
        // Scroll position
        if shown.len() > self.rows {
            let th = self.rows as f32 * rh;
            let bh = (th * self.rows as f32 / shown.len() as f32).max(10.0 * s);
            let by = ly + (th - bh) * self.top as f32 / (shown.len() - self.rows) as f32;
            cv.rect(x0 + lw - 2.0 * s, by, 2.0 * s, bh, GREY.alpha(0.5 * a));
        }

        // The preview
        let px0 = x0 + lw + 16.0 * s;
        let pw = x0 + iw - px0;
        let ph = bottom - ly;
        cv.round_rect(px0, ly, pw, ph, 14.0 * s, Rgba(1.0, 1.0, 1.0, 0.04 * a));
        if let Some(e) = self.selected() {
            let loaded = self.loaded.lock().unwrap();
            match loaded.get(&e.id) {
                Some(Loaded::Image(Some(img))) => {
                    let cell = ((pw - 20.0 * s) / img.w as f32).min((ph - 40.0 * s) / img.h as f32).floor();
                    let (ix, iy) = (px0 + (pw - cell * img.w as f32) / 2.0, ly + 12.0 * s);
                    fx::dot_image(cv, img, ix, iy, cell, 0.0, a * f.reveal);
                    if let Some(info) = &e.image {
                        let b = ly + ph - 12.0 * s;
                        cv.text(info, px0 + 10.0 * s, b, 9.5 * s, Weight::Regular, GREY.alpha(a), Some((0.0, px0 + pw - 6.0 * s)), 16.0 * s);
                    }
                }
                Some(Loaded::Image(None)) => {
                    cv.text("NO PREVIEW", px0 + 12.0 * s, ly + 24.0 * s, 10.0 * s, Weight::Regular, DIM.alpha(a), None, 0.0);
                }
                Some(Loaded::Text(t)) => {
                    let pxs = 10.5 * s;
                    let cols = ((pw - 24.0 * s) / cv.fonts.advance(Weight::Regular, pxs, 'M')) as usize;
                    let lh = 16.0 * s;
                    let max = ((ph - 20.0 * s) / lh) as usize;
                    let lines = crate::text::wrap(t, cols);
                    for (k, &(st, en)) in lines.iter().take(max).enumerate() {
                        let b = (ly + 20.0 * s + k as f32 * lh).round();
                        cv.text(&t[st..en], px0 + 12.0 * s, b, pxs, Weight::Regular, WHITE.alpha(0.9 * a), None, 0.0);
                    }
                    if lines.len() > max {
                        cv.text("…", px0 + pw - 20.0 * s, ly + ph - 8.0 * s, pxs, Weight::Regular, GREY.alpha(a), None, 0.0);
                    }
                }
                None => {
                    // Loading: a row of dots filling in.
                    let n = ((f.now * 8.0) as usize) % 4;
                    for k in 0..3 {
                        let c = if k < n { WHITE } else { DIM };
                        cv.round_rect(px0 + 14.0 * s + k as f32 * 10.0 * s, ly + 18.0 * s, 4.0 * s, 4.0 * s, 2.0 * s, c.alpha(a));
                    }
                }
            }
        }
        let fb = f.y + f.h - 16.0 * s;
        shell::hints(cv, &[("ENTER", "copy"), ("DEL", "delete"), ("^P", "pin"), ("ESC", "close")], x0, fb, s, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: u64, p: &str) -> Entry {
        parse_line(&format!("{id}\t{p}")).unwrap()
    }

    #[test]
    fn parses_text_and_image_lines() {
        let t = e(12, "hello world");
        assert_eq!((t.id, t.preview.as_str(), t.image), (12, "hello world", None));
        let i = e(7, "[[ binary data 31 KiB png 640x480 ]]");
        assert_eq!(i.image.as_deref(), Some("PNG · 640×480 · 31 KIB"));
        assert!(parse_line("garbage").is_none());
    }

    #[test]
    fn pinned_first_then_newest_and_filtered() {
        let list = vec![e(5, "newest"), e(4, "Alpha beta"), e(3, "gamma"), e(2, "[[ binary data 1 KiB png 2x2 ]]")];
        assert_eq!(visible(&list, &[3], ""), [2, 0, 1, 3]);
        assert_eq!(visible(&list, &[3], "ALPHA"), [1]);
        assert_eq!(visible(&list, &[], "png"), [3]);
        assert_eq!(visible(&list, &[99], ""), [0, 1, 2, 3]); // a pin whose entry is gone
    }
}
