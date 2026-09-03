//! Line-level diffing turned into rows that line up left (HEAD) and right
//! (working tree) for a side-by-side view.

use similar::{ChangeTag, TextDiff};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Unchanged line, present on both sides.
    Equal,
    /// Line only on the right (added).
    Add,
    /// Line only on the left (removed).
    Remove,
    /// A left line replaced by a right line.
    Modify,
}

pub struct DiffRow {
    pub left_no: Option<usize>,
    pub right_no: Option<usize>,
    pub kind: RowKind,
}

/// Align `old` and `new` line-by-line into side-by-side rows.
pub fn side_by_side(old: &str, new: &str) -> Vec<DiffRow> {
    let diff = TextDiff::from_lines(old, new);

    let mut rows = Vec::new();
    let mut left_no = 0usize;
    let mut right_no = 0usize;
    let mut dels: Vec<String> = Vec::new();
    let mut inss: Vec<String> = Vec::new();

    for change in diff.iter_all_changes() {
        let line = change.value().trim_end_matches('\n').to_string();
        match change.tag() {
            ChangeTag::Delete => dels.push(line),
            ChangeTag::Insert => inss.push(line),
            ChangeTag::Equal => {
                flush(&mut rows, &mut left_no, &mut right_no, &mut dels, &mut inss);
                left_no += 1;
                right_no += 1;
                rows.push(DiffRow {
                    left_no: Some(left_no),
                    right_no: Some(right_no),
                    kind: RowKind::Equal,
                });
            }
        }
    }
    flush(&mut rows, &mut left_no, &mut right_no, &mut dels, &mut inss);
    rows
}

/// Emit rows for a run of deletions/insertions, pairing them up as modifications
/// where both sides have a line and as pure add/remove for the leftover tail.
fn flush(
    rows: &mut Vec<DiffRow>,
    left_no: &mut usize,
    right_no: &mut usize,
    dels: &mut Vec<String>,
    inss: &mut Vec<String>,
) {
    let n = dels.len().max(inss.len());
    for i in 0..n {
        let has_left = i < dels.len();
        let has_right = i < inss.len();
        let row = match (has_left, has_right) {
            (true, true) => {
                *left_no += 1;
                *right_no += 1;
                DiffRow {
                    left_no: Some(*left_no),
                    right_no: Some(*right_no),
                    kind: RowKind::Modify,
                }
            }
            (true, false) => {
                *left_no += 1;
                DiffRow {
                    left_no: Some(*left_no),
                    right_no: None,
                    kind: RowKind::Remove,
                }
            }
            (false, true) => {
                *right_no += 1;
                DiffRow {
                    left_no: None,
                    right_no: Some(*right_no),
                    kind: RowKind::Add,
                }
            }
            (false, false) => unreachable!(),
        };
        rows.push(row);
    }
    dels.clear();
    inss.clear();
}
