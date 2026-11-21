// A small text editor model for the widgets (notes body, search fields): a string, a cursor (byte index on a char
// boundary) and soft wrapping into visual lines for a monospace font.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBuf {
    pub text: String,
    pub cursor: usize,
}

impl TextBuf {
    pub fn new(text: &str) -> Self {
        Self { text: text.to_string(), cursor: text.len() }
    }

    pub fn insert(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    pub fn backspace(&mut self) {
        if let Some(c) = self.text[..self.cursor].chars().next_back() {
            self.cursor -= c.len_utf8();
            self.text.remove(self.cursor);
        }
    }

    /// Deletes the word before the cursor (Ctrl+Backspace).
    pub fn backspace_word(&mut self) {
        let head = &self.text[..self.cursor];
        let trimmed = head.trim_end_matches(|c: char| c.is_whitespace());
        let start = trimmed.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i + 1);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        if let Some(c) = self.text[..self.cursor].chars().next_back() {
            self.cursor -= c.len_utf8();
        }
    }

    pub fn right(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
    }

    /// Start / end of the visual line the cursor is on.
    pub fn home(&mut self, cols: usize) {
        let lines = wrap(&self.text, cols);
        self.cursor = lines[line_of(&lines, self.cursor)].0;
    }

    pub fn end(&mut self, cols: usize) {
        let lines = wrap(&self.text, cols);
        self.cursor = lines[line_of(&lines, self.cursor)].1;
    }

    /// Up (-1) or down (+1) one visual line, keeping the column where it can.
    pub fn vertical(&mut self, dir: i32, cols: usize) {
        let lines = wrap(&self.text, cols);
        let i = line_of(&lines, self.cursor);
        let col = self.text[lines[i].0..self.cursor].chars().count();
        let j = i as i32 + dir;
        if j < 0 {
            self.cursor = 0;
            return;
        }
        let Some(&(s, e)) = lines.get(j as usize) else {
            self.cursor = self.text.len();
            return;
        };
        self.cursor = self.text[s..e].char_indices().nth(col).map_or(e, |(k, _)| s + k);
    }

    /// The visual line and column of the cursor.
    pub fn position(&self, cols: usize) -> (usize, usize) {
        let lines = wrap(&self.text, cols);
        let i = line_of(&lines, self.cursor);
        (i, self.text[lines[i].0..self.cursor].chars().count())
    }

    /// Moves the cursor to a visual line and column (a click in the text).
    pub fn place(&mut self, line: usize, col: usize, cols: usize) {
        let lines = wrap(&self.text, cols);
        let (s, e) = lines[line.min(lines.len() - 1)];
        self.cursor = self.text[s..e].char_indices().nth(col).map_or(e, |(k, _)| s + k);
    }
}

/// Splits text into visual lines of at most `cols` characters: (start, end) byte ranges, without the newline.
/// Long lines break after the last space that fits, or mid-word when there is none. Always at least one line.
pub fn wrap(text: &str, cols: usize) -> Vec<(usize, usize)> {
    let cols = cols.max(1);
    let mut out = Vec::new();
    let mut start = 0;
    for para in text.split('\n') {
        let end = start + para.len();
        let mut s = start;
        loop {
            let rest = &text[s..end];
            if rest.chars().count() <= cols {
                out.push((s, end));
                break;
            }
            let cut = rest.char_indices().nth(cols).map_or(rest.len(), |(k, _)| k);
            // Break after the last space that fits (the space stays at the end of the line).
            let brk = rest[..cut].rfind(' ').map_or(cut, |k| k + 1);
            out.push((s, s + brk));
            s += brk;
        }
        start = end + 1;
    }
    out
}

/// The visual line holding byte `cursor`. A cursor at a soft-wrap point belongs to the next line.
fn line_of(lines: &[(usize, usize)], cursor: usize) -> usize {
    for (i, &(s, e)) in lines.iter().enumerate() {
        let soft = lines.get(i + 1).is_some_and(|n| n.0 == e);
        if cursor >= s && (cursor < e || (cursor == e && !soft)) {
            return i;
        }
    }
    lines.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_spaces_then_mid_word() {
        let t = "hello world foo\nabcdefghij";
        let lines: Vec<&str> = wrap(t, 6).iter().map(|&(s, e)| &t[s..e]).collect();
        assert_eq!(lines, ["hello ", "world ", "foo", "abcdef", "ghij"]);
        assert_eq!(wrap("", 10), [(0, 0)]);
        assert_eq!(wrap("a\n", 10).len(), 2);
    }

    #[test]
    fn edits_and_moves_by_chars() {
        let mut b = TextBuf::new("héllo");
        b.backspace();
        b.left();
        b.insert("L");
        assert_eq!((b.text.as_str(), b.cursor), ("hélLl", 5));
        b.delete();
        assert_eq!(b.text, "hélL");
        b.backspace_word();
        assert_eq!((b.text.as_str(), b.cursor), ("", 0));
        b.backspace();
        b.left();
        assert_eq!(b.cursor, 0);
    }

    #[test]
    fn moves_between_visual_lines() {
        let mut b = TextBuf::new("abc def\nxy");
        b.cursor = 1; // a|bc
        b.vertical(1, 4); // "def" on the second visual line
        assert_eq!(b.position(4), (1, 1));
        b.vertical(1, 4);
        assert_eq!(b.position(4), (2, 1)); // x|y
        b.vertical(1, 4);
        assert_eq!(b.cursor, b.text.len());
        b.home(4);
        assert_eq!(b.position(4), (2, 0));
        b.vertical(-5, 4);
        assert_eq!(b.cursor, 0);
        b.place(1, 9, 4);
        assert_eq!(&b.text[..b.cursor], "abc def");
    }
}
