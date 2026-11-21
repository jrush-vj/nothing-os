// Notes (`nothing-bar notes`, SUPER+N): a drawer under the bar's left end. Plain-text notes, one file each in
// ~/.local/share/nothing-bar/notes, saved half a second after typing stops and again on close.
// Tabs along the top (01 02 03 …), Ctrl+N new, Ctrl+D delete (press twice), Ctrl+Z bring the last one back,
// Ctrl+←/→ or Tab switch, Ctrl+Shift+C copies the note.
use crate::bar::{GREY, HOVER, RED, WHITE};
use crate::canvas::{Canvas, Weight};
use crate::shell::{self, Act, Frame, Key, Mods, Place, Ptr, Widget, DIM};
use crate::text::{wrap, TextBuf};
use smithay_client_toolkit::shell::wlr_layer::Anchor;
use std::path::PathBuf;

const W: f32 = 460.0;
const H: f32 = 420.0;
const PAD: f32 = 18.0;
const PX: f32 = 12.5;
const LINE: f32 = 20.0;

struct Note {
    file: PathBuf,
    buf: TextBuf,
    saved: bool,
}

pub struct Notes {
    dir: PathBuf,
    notes: Vec<Note>,
    cur: usize,
    deleted: Option<(PathBuf, String)>,
    /// Ctrl+D pressed once at this time; a second press within 2 s deletes.
    armed: Option<f32>,
    copied: Option<f32>,
    edited_at: f32,
    now: f32,
    scroll: usize,
    /// Columns that fit, from the last draw (for cursor movement).
    cols: usize,
    hits: Vec<([f32; 4], Hit)>,
    pointer: Option<(f32, f32)>,
    /// Text area origin and line height from the last draw (clicks in the text).
    text_at: (f32, f32, f32, f32),
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Tab(usize),
    New,
    Copy,
}

pub fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"));
    base.join("nothing-bar/notes")
}

impl Notes {
    pub fn new(dir: PathBuf) -> Self {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "txt")).collect())
            .unwrap_or_default();
        files.sort();
        let mut notes: Vec<Note> = files
            .into_iter()
            .map(|file| {
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                Note { file, buf: TextBuf::new(&text), saved: true }
            })
            .collect();
        if notes.is_empty() {
            notes.push(Note { file: dir.join("001.txt"), buf: TextBuf::default(), saved: true });
        }
        let cur = notes.len() - 1;
        Self { dir, notes, cur, deleted: None, armed: None, copied: None, edited_at: 0.0, now: 0.0, scroll: 0, cols: 50, hits: Vec::new(), pointer: None, text_at: (0.0, 0.0, 1.0, 1.0) }
    }

    fn next_file(&self) -> PathBuf {
        let n = self.notes.iter().filter_map(|n| n.file.file_stem()?.to_str()?.parse::<u32>().ok()).max().unwrap_or(0);
        self.dir.join(format!("{:03}.txt", n + 1))
    }

    /// Writes unsaved notes (atomically: temp file + rename). Empty notes are not written.
    fn save(&mut self) {
        let _ = std::fs::create_dir_all(&self.dir);
        for n in self.notes.iter_mut().filter(|n| !n.saved) {
            if n.buf.text.is_empty() && !n.file.exists() {
                n.saved = true;
                continue;
            }
            let tmp = n.file.with_extension("tmp");
            if std::fs::write(&tmp, &n.buf.text).and_then(|_| std::fs::rename(&tmp, &n.file)).is_ok() {
                n.saved = true;
            } else {
                eprintln!("nothing-notes: could not save {}", n.file.display());
            }
        }
    }

    fn edited(&mut self) {
        self.notes[self.cur].saved = false;
        self.edited_at = self.now;
        self.armed = None;
    }

    fn switch(&mut self, to: usize) {
        self.save();
        self.cur = to.min(self.notes.len() - 1);
        self.scroll = 0;
        self.armed = None;
    }

    fn new_note(&mut self) {
        self.save();
        let file = self.next_file();
        self.notes.push(Note { file, buf: TextBuf::default(), saved: true });
        self.cur = self.notes.len() - 1;
        self.scroll = 0;
    }

    fn delete(&mut self) {
        let n = self.notes.remove(self.cur);
        let _ = std::fs::remove_file(&n.file);
        self.deleted = Some((n.file, n.buf.text));
        if self.notes.is_empty() {
            let file = self.next_file();
            self.notes.push(Note { file, buf: TextBuf::default(), saved: true });
        }
        self.cur = self.cur.min(self.notes.len() - 1);
        self.armed = None;
    }

    fn undo_delete(&mut self) {
        if let Some((file, text)) = self.deleted.take() {
            self.notes.push(Note { file, buf: TextBuf::new(&text), saved: false });
            self.notes.sort_by(|a, b| a.file.cmp(&b.file));
            self.cur = self.notes.iter().position(|n| n.buf.text == text).unwrap_or(0);
            self.save();
        }
    }

    fn copy(&mut self) {
        let text = self.notes[self.cur].buf.text.clone();
        if let Ok(mut c) = std::process::Command::new("wl-copy").stdin(std::process::Stdio::piped()).spawn() {
            use std::io::Write;
            if let Some(mut i) = c.stdin.take() {
                let _ = i.write_all(text.as_bytes());
            }
            std::thread::spawn(move || c.wait());
            self.copied = Some(self.now);
        }
    }
}

impl Widget for Notes {
    fn place(&self) -> Place {
        Place { anchor: Anchor::TOP | Anchor::LEFT, gap: 8, size: (W, H) }
    }

    fn key(&mut self, k: &Key, m: Mods, now: f32) -> Act {
        self.now = now;
        let cols = self.cols;
        let n = self.notes.len();
        let b = &mut self.notes[self.cur].buf;
        match k {
            Key::Esc => return Act::Close,
            Key::Text(t) => {
                b.insert(t);
                self.edited();
            }
            Key::Enter => {
                b.insert("\n");
                self.edited();
            }
            Key::Back => {
                b.backspace();
                self.edited();
            }
            Key::BackWord => {
                b.backspace_word();
                self.edited();
            }
            Key::Delete => {
                b.delete();
                self.edited();
            }
            Key::Left if m.ctrl => self.switch((self.cur + n - 1) % n),
            Key::Right if m.ctrl => self.switch((self.cur + 1) % n),
            Key::Tab => self.switch(if m.shift { (self.cur + n - 1) % n } else { (self.cur + 1) % n }),
            Key::Left => b.left(),
            Key::Right => b.right(),
            Key::Up => b.vertical(-1, cols),
            Key::Down => b.vertical(1, cols),
            Key::PageUp => b.vertical(-10, cols),
            Key::PageDown => b.vertical(10, cols),
            Key::Home => b.home(cols),
            Key::End => b.end(cols),
            Key::Ctrl('n') => self.new_note(),
            Key::Ctrl('d') => {
                if self.armed.is_some_and(|t| now - t < 2.0) {
                    self.delete();
                } else {
                    self.armed = Some(now);
                }
            }
            Key::Ctrl('z') => self.undo_delete(),
            Key::Ctrl('c') if m.shift => self.copy(),
            Key::Ctrl('s') => self.save(),
            _ => {}
        }
        Act::Stay
    }

    fn pointer(&mut self, x: f32, y: f32, p: Ptr, now: f32) -> Act {
        self.now = now;
        self.pointer = (p != Ptr::Leave).then_some((x, y));
        if p != Ptr::Press(0x110) {
            return Act::Stay;
        }
        let hit = self.hits.iter().find(|(r, _)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]).map(|h| h.1);
        match hit {
            Some(Hit::Tab(i)) => self.switch(i),
            Some(Hit::New) => self.new_note(),
            Some(Hit::Copy) => self.copy(),
            None => {
                let (tx, ty, cw, lh) = self.text_at;
                if x >= tx && y >= ty {
                    let line = ((y - ty) / lh) as usize + self.scroll;
                    let col = ((x - tx) / cw).round() as usize;
                    let cols = self.cols;
                    self.notes[self.cur].buf.place(line, col, cols);
                }
            }
        }
        Act::Stay
    }

    fn tick(&mut self, now: f32) -> bool {
        self.now = now;
        if self.notes.iter().any(|n| !n.saved) && now - self.edited_at > 0.6 {
            self.save();
        }
        false
    }

    fn period(&self) -> Option<f32> {
        Some(0.5) // cursor blink and the autosave check
    }

    fn draw(&mut self, cv: &mut Canvas, f: &Frame) {
        let s = f.s;
        let (x0, y0) = (f.x + PAD * s, f.y + PAD * s);
        let iw = f.w - 2.0 * PAD * s;
        self.hits.clear();
        let hov = |r: [f32; 4]| self.pointer.is_some_and(|(x, y)| x + f.x >= r[0] && x + f.x < r[0] + r[2] && y + f.y >= r[1] && y + f.y < r[1] + r[3]);
        let rel = |r: [f32; 4]| [r[0] - f.x, r[1] - f.y, r[2], r[3]];

        // Header: NOTES, then tabs, then the save state / COPY on the right.
        let base = y0 + 12.0 * s;
        let mut pen = crate::fx::decode_text(cv, "NOTES", x0, base, 12.0 * s, Weight::Bold, WHITE.alpha(f.a), f.decode_at(base), None, 0.0);
        pen += 14.0 * s;
        let tab_w = 30.0 * s;
        let tabs_end = x0 + iw - 140.0 * s;
        // Show the tabs around the current note when there are more than fit.
        let fit = (((tabs_end - pen) / tab_w) as usize).max(1);
        let first = self.cur.saturating_sub(fit.saturating_sub(2)).min(self.notes.len().saturating_sub(fit));
        let mut tx = pen;
        for i in first..self.notes.len().min(first + fit) {
            let r = [tx, y0 - 2.0 * s, tab_w - 4.0 * s, 22.0 * s];
            let active = i == self.cur;
            if active {
                cv.round_rect(r[0], r[1], r[2], r[3], 8.0 * s, WHITE.alpha(f.a));
            } else if hov(r) {
                cv.round_rect(r[0], r[1], r[2], r[3], 8.0 * s, HOVER.alpha(f.a));
            }
            let label = format!("{:02}", i + 1);
            let lw = cv.fonts.width(Weight::Bold, 10.0 * s, &label);
            let c = if active { crate::canvas::Rgba::hex(0x000000) } else if self.notes[i].saved { GREY } else { WHITE };
            cv.text(&label, r[0] + (r[2] - lw) / 2.0, base, 10.0 * s, Weight::Bold, c.alpha(f.a), None, 0.0);
            self.hits.push((rel(r), Hit::Tab(i)));
            tx += tab_w;
        }
        let r = [tx, y0 - 2.0 * s, tab_w - 4.0 * s, 22.0 * s];
        if hov(r) {
            cv.round_rect(r[0], r[1], r[2], r[3], 8.0 * s, HOVER.alpha(f.a));
        }
        let pw = cv.fonts.width(Weight::Bold, 12.0 * s, "+");
        cv.text("+", r[0] + (r[2] - pw) / 2.0, base + 1.0 * s, 12.0 * s, Weight::Bold, GREY.alpha(f.a), None, 0.0);
        self.hits.push((rel(r), Hit::New));

        let copied = self.copied.is_some_and(|t| f.now - t < 1.2);
        let copy = if copied { "COPIED" } else { "COPY" };
        let cw = cv.fonts.width(Weight::Bold, 10.0 * s, copy) + 16.0 * s;
        let cr = [x0 + iw - cw, y0 - 2.0 * s, cw, 22.0 * s];
        cv.stroke_round_rect(cr[0] + 0.5, cr[1] + 0.5, cr[2] - 1.0, cr[3] - 1.0, 8.0 * s, 1.0, (if hov(cr) { WHITE } else { DIM }).alpha(f.a));
        cv.text(copy, cr[0] + 8.0 * s, base, 10.0 * s, Weight::Bold, (if copied { RED } else { WHITE }).alpha(f.a), None, 0.0);
        self.hits.push((rel(cr), Hit::Copy));
        let state = match (self.armed.is_some_and(|t| f.now - t < 2.0), self.notes[self.cur].saved) {
            (true, _) => ("CTRL+D AGAIN TO DELETE", RED),
            (false, true) => ("SAVED", GREY),
            (false, false) => ("EDITING", WHITE),
        };
        let sw = cv.fonts.width(Weight::Regular, 9.5 * s, state.0);
        cv.text(state.0, cr[0] - 10.0 * s - sw, base, 9.5 * s, Weight::Regular, state.1.alpha(f.a), None, 0.0);
        let line_y = y0 + 30.0 * s;
        cv.rect(x0, line_y, iw, 1.0, crate::canvas::Rgba(1.0, 1.0, 1.0, 0.08 * f.a));

        // Body
        let px = PX * s;
        let cwid = cv.fonts.advance(Weight::Regular, px, 'M');
        self.cols = ((iw / cwid) as usize).max(8);
        let lh = LINE * s;
        let ty = line_y + 12.0 * s;
        let rows = (((f.y + f.h - 40.0 * s) - ty) / lh) as usize;
        self.text_at = (x0 - f.x, ty - f.y, cwid, lh);
        let note = &self.notes[self.cur];
        let lines = wrap(&note.buf.text, self.cols);
        let (cl, cc) = note.buf.position(self.cols);
        if cl < self.scroll {
            self.scroll = cl;
        } else if cl >= self.scroll + rows {
            self.scroll = cl + 1 - rows;
        }
        if note.buf.text.is_empty() {
            let b = (ty + lh * 0.7).round();
            crate::fx::decode_text(cv, "START TYPING", x0 + cwid, b, px, Weight::Regular, DIM.alpha(f.a), f.decode_at(b), None, 0.0);
        }
        for (i, &(a, e)) in lines.iter().enumerate().skip(self.scroll).take(rows) {
            let b = (ty + (i - self.scroll) as f32 * lh + lh * 0.7).round();
            crate::fx::decode_text(cv, &note.buf.text[a..e], x0, b, px, Weight::Regular, WHITE.alpha(f.a), f.decode_at(b), None, 0.0);
        }
        // The cursor: a red bar, blinking once a second (steady while typing).
        let typing = f.now - self.edited_at < 0.8;
        if typing || (f.now * 2.0) as u32 % 2 == 0 {
            let cx = x0 + cc as f32 * cwid;
            let cy = ty + (cl - self.scroll) as f32 * lh + 3.0 * s;
            cv.rect(cx.round(), cy, (1.5 * s).max(1.0), lh - 6.0 * s, RED.alpha(f.a));
        }
        // Scroll position, as a short line on the right edge.
        if lines.len() > rows {
            let th = rows as f32 * lh;
            let bh = (th * rows as f32 / lines.len() as f32).max(12.0 * s);
            let by = ty + (th - bh) * self.scroll as f32 / (lines.len() - rows) as f32;
            cv.rect(f.x + f.w - 8.0 * s, by, 2.0 * s, bh, GREY.alpha(0.6 * f.a));
        }
        let fb = f.y + f.h - 16.0 * s;
        shell::hints(cv, &[("^N", "new"), ("^D", "delete"), ("TAB", "next"), ("ESC", "close")], x0, fb, s, f.a);
        let count = format!("{} / {}", self.cur + 1, self.notes.len());
        let cw2 = cv.fonts.width(Weight::Regular, 9.5 * s, &count);
        cv.text(&count, x0 + iw - cw2, fb, 9.5 * s, Weight::Regular, GREY.alpha(f.a), None, 0.0);
    }

    fn close(&mut self) {
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nothing-notes-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn typ(n: &mut Notes, s: &str) {
        for c in s.chars() {
            n.key(&Key::Text(c.to_string()), Mods::default(), 0.0);
        }
    }

    #[test]
    fn notes_save_switch_delete_and_come_back() {
        let d = tmp("cycle");
        let mut n = Notes::new(d.clone());
        typ(&mut n, "first");
        n.key(&Key::Ctrl('n'), Mods::default(), 0.0);
        typ(&mut n, "second");
        n.close();
        let again = Notes::new(d.clone());
        assert_eq!(again.notes.iter().map(|n| n.buf.text.as_str()).collect::<Vec<_>>(), ["first", "second"]);
        let mut n = again;
        let ctrl = Mods { ctrl: true, ..Mods::default() };
        n.key(&Key::Ctrl('d'), ctrl, 1.0);
        assert_eq!(n.notes.len(), 2); // armed only
        n.key(&Key::Ctrl('d'), ctrl, 1.5);
        assert_eq!(n.notes.len(), 1);
        assert!(!d.join("002.txt").exists());
        n.key(&Key::Ctrl('z'), ctrl, 2.0);
        assert!(d.join("002.txt").exists() && n.notes.len() == 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn empty_new_notes_leave_no_files() {
        let d = tmp("empty");
        let mut n = Notes::new(d.clone());
        n.key(&Key::Ctrl('n'), Mods::default(), 0.0);
        n.close();
        assert!(std::fs::read_dir(&d).map_or(0, |r| r.count()) == 0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
