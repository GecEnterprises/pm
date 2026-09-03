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
    pub left: Option<String>,
    pub right_no: Option<usize>,
    pub right: Option<String>,
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
        let value = change.value().trim_end_matches('\n').to_string();
        match change.tag() {
            ChangeTag::Delete => dels.push(value),
            ChangeTag::Insert => inss.push(value),
            ChangeTag::Equal => {
                flush(&mut rows, &mut left_no, &mut right_no, &mut dels, &mut inss);
                left_no += 1;
                right_no += 1;
                rows.push(DiffRow {
                    left_no: Some(left_no),
                    left: Some(value.clone()),
                    right_no: Some(right_no),
                    right: Some(value),
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
        let left = dels.get(i).cloned();
        let right = inss.get(i).cloned();
        let (ln, rn, kind) = match (&left, &right) {
            (Some(_), Some(_)) => {
                *left_no += 1;
                *right_no += 1;
                (Some(*left_no), Some(*right_no), RowKind::Modify)
            }
            (Some(_), None) => {
                *left_no += 1;
                (Some(*left_no), None, RowKind::Remove)
            }
            (None, Some(_)) => {
                *right_no += 1;
                (None, Some(*right_no), RowKind::Add)
            }
            (None, None) => unreachable!(),
        };
        rows.push(DiffRow {
            left_no: ln,
            left,
            right_no: rn,
            right,
            kind,
        });
    }
    dels.clear();
    inss.clear();
}
