//! Buffer-coordinate caret model for the diff (Zed's `Point`, scaled down).

/// A caret position in *buffer* space: a line of one column's source file plus a
/// char-boundary byte offset into that line. Zed's `Point`, scaled down — the
/// diff's visual rows are a display layer on top of this (see [`Pm::col_display`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct BufferPos {
    /// Zero-based line index into the column's source (`old` for col 0, `new` for col 1).
    pub file_row: usize,
    pub byte: usize,
}

/// A caret + selection within one diff column (`col`: 0 = HEAD, 1 = working).
/// `anchor`/`head` are buffer positions; `goal_x` is the preferred x for vertical
/// movement (Zed's `SelectionGoal`).
#[derive(Clone, Copy)]
pub struct DiffCursor {
    pub col: usize,
    pub anchor: BufferPos,
    pub head: BufferPos,
    pub goal_x: Option<f32>,
}

impl DiffCursor {
    pub fn has_selection(&self) -> bool {
        self.anchor != self.head
    }
    pub fn ordered(&self) -> (BufferPos, BufferPos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

/// The two source files behind the open diff, one per column (`[old, new]`), each
/// split into line byte-ranges (newline excluded). This is the caret's "buffer":
/// selection and copy slice straight out of `src`, so lines that exist only on the
/// other column can't leak in.
#[derive(Default)]
pub struct DiffText {
    src: [String; 2],
    lines: [Vec<std::ops::Range<usize>>; 2],
}

impl DiffText {
    pub fn build(old: String, new: String) -> Self {
        let lines = [line_ranges(&old), line_ranges(&new)];
        Self { src: [old, new], lines }
    }

    pub fn line_count(&self, col: usize) -> usize {
        self.lines[col].len()
    }

    pub fn line(&self, col: usize, file_row: usize) -> &str {
        self.lines[col]
            .get(file_row)
            .map_or("", |r| &self.src[col][r.clone()])
    }

    /// 1-based character column of `pos` within its line (for a status readout).
    pub fn char_col(&self, col: usize, pos: BufferPos) -> usize {
        let line = self.line(col, pos.file_row);
        line[..pos.byte.min(line.len())].chars().count() + 1
    }

    /// Absolute byte offset in `src[col]` of `byte` within line `file_row`,
    /// clamped to the line's end.
    pub(crate) fn offset(&self, col: usize, file_row: usize, byte: usize) -> usize {
        match self.lines[col].get(file_row) {
            Some(r) => (r.start + byte).min(r.end),
            None => self.src[col].len(),
        }
    }

    /// Source text between two ordered buffer positions in the same column.
    pub fn slice(&self, col: usize, a: BufferPos, b: BufferPos) -> &str {
        let s = self.offset(col, a.file_row, a.byte);
        let e = self.offset(col, b.file_row, b.byte).max(s);
        &self.src[col][s..e]
    }

    /// Snap a buffer position onto real text: clamp the row into range and the
    /// byte onto a char boundary within that line. Zed's `clip_point`.
    pub fn clip(&self, col: usize, p: BufferPos) -> BufferPos {
        let last = self.line_count(col).saturating_sub(1);
        let file_row = p.file_row.min(last);
        let s = self.line(col, file_row);
        let mut byte = p.byte.min(s.len());
        while byte > 0 && !s.is_char_boundary(byte) {
            byte -= 1;
        }
        BufferPos { file_row, byte }
    }
}

/// Byte ranges of each line in `s` (trailing `\r`/`\n` excluded). Matches the line
/// count `diff::side_by_side` produces for the same text.
fn line_ranges(s: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = s.as_bytes();
    let mut v = Vec::new();
    let mut start = 0;
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            let end = if i > start && bytes[i - 1] == b'\r' { i - 1 } else { i };
            v.push(start..end);
            start = i + 1;
        }
    }
    if start < bytes.len() {
        let end = if bytes[bytes.len() - 1] == b'\r' { bytes.len() - 1 } else { bytes.len() };
        v.push(start..end.max(start));
    }
    v
}
