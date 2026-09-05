//! The per-project ticket store: `<repo-root>/.pm/pm.json5`.
//!
//! `pm` is aiming to be a local, in-repo ticket tracker ("Jira, direct-to-code").
//! This module is just the data model plus load/save — no gpui, no policy. The
//! file is read as JSON5 (comments, trailing commas, unquoted keys) and written
//! back as pretty-printed JSON, which is a valid JSON5 subset; hand-written
//! comments in the file are lost the first time `pm` saves it.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// File name under the `.pm/` directory.
const FILE: &str = "pm.json5";

/// Seconds since the Unix epoch, right now.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Lifecycle state of a ticket.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Open,
    InProgress,
    Blocked,
    Done,
    Wontfix,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Open => "Open",
            Status::InProgress => "In Progress",
            Status::Blocked => "Blocked",
            Status::Done => "Done",
            Status::Wontfix => "Won't Fix",
        }
    }

    pub fn color(self) -> u32 {
        match self {
            Status::Open => 0x6ca4dc,
            Status::InProgress => 0xe2c08d,
            Status::Blocked => 0xc74e39,
            Status::Done => 0x81b88b,
            Status::Wontfix => 0x808080,
        }
    }

    pub const ALL: [Status; 5] = [
        Status::Open,
        Status::InProgress,
        Status::Blocked,
        Status::Done,
        Status::Wontfix,
    ];

    /// Whether the ticket is finished (hidden from the list by default).
    pub fn is_closed(self) -> bool {
        matches!(self, Status::Done | Status::Wontfix)
    }
}

/// How urgent a ticket is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Urgent,
}

impl Priority {
    pub fn label(self) -> &'static str {
        match self {
            Priority::Low => "Low",
            Priority::Normal => "Normal",
            Priority::High => "High",
            Priority::Urgent => "Urgent",
        }
    }

    pub fn color(self) -> u32 {
        match self {
            Priority::Low => 0x808080,
            Priority::Normal => 0xd4d4d4,
            Priority::High => 0xe2c08d,
            Priority::Urgent => 0xc74e39,
        }
    }
}

/// A pointer into the code. Not created by the UI yet — the diff-gutter flow
/// comes later — but rendered read-only when present, and the shape the future
/// MCP server will read.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Anchor {
    /// Repo-root-relative path.
    pub file: String,
    /// 1-based inclusive line range.
    pub start_line: u32,
    pub end_line: u32,
    /// The commit (short id) this anchor was made against, or `"working"`.
    #[serde(default)]
    pub rev: String,
}

/// One comment on a ticket.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    #[serde(default)]
    pub author: String,
    /// Unix seconds.
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub body: String,
    /// Optional line anchor for a code-level comment.
    #[serde(default)]
    pub anchor: Option<Anchor>,
}

/// One recorded change to a ticket, forming its audit trail (PM-58). Ticket
/// creation and comment bodies already carry their own `created`/`author`, so
/// this only covers field edits plus a pointer to each comment, letting the UI
/// render one merged timeline without duplicating comment text here.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryEvent {
    TitleChanged { old: String, new: String },
    BodyChanged { old: String, new: String },
    StatusChanged { old: Status, new: Status },
    PriorityChanged { old: Priority, new: Priority },
    LabelsChanged { old: Vec<String>, new: Vec<String> },
    AssigneeChanged { old: Option<String>, new: Option<String> },
    /// A comment was added; look it up by id in `Ticket::comments` for its body.
    Commented { comment_id: u64 },
}

/// One entry in a ticket's history, oldest first.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix seconds.
    pub at: i64,
    #[serde(default)]
    pub author: String,
    #[serde(flatten)]
    pub event: HistoryEvent,
}

/// One ticket.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Ticket {
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    /// Who filed the ticket. Free-form and unverified (PM-15) — the GUI and the
    /// MCP server may both write any name here.
    #[serde(default)]
    pub author: String,
    /// Unix seconds.
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub updated: i64,
    #[serde(default)]
    pub anchors: Vec<Anchor>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// Audit trail of field edits and comments, oldest first (PM-58). Absent on
    /// tickets written before this field existed — an empty history there just
    /// means "no lineage recorded before now", not "nothing ever happened".
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

/// Project-level metadata.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Project {
    #[serde(default)]
    pub name: String,
    /// Optional key; ticket ids display as `"{key}-{id}"`, else `"#{id}"`.
    #[serde(default)]
    pub key: Option<String>,
}

/// The whole `.pm/pm.json5` file.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PmData {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub project: Project,
    #[serde(default = "one_u64")]
    pub next_id: u64,
    #[serde(default)]
    pub tickets: Vec<Ticket>,
}

fn one() -> u32 {
    1
}
fn one_u64() -> u64 {
    1
}

impl Default for PmData {
    fn default() -> Self {
        Self {
            version: 1,
            project: Project::default(),
            next_id: 1,
            tickets: Vec::new(),
        }
    }
}

/// Why a load didn't produce data. `Io` is almost always transient on Windows
/// (the watcher fired mid-write, or an antivirus scan briefly locked the file)
/// and callers should keep their last-good data and try again; `Parse` means the
/// file on disk is genuinely broken and the user needs to see it.
#[derive(Debug)]
pub enum LoadError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(s) | LoadError::Parse(s) => f.write_str(s),
        }
    }
}

/// Read `<root>/.pm/pm.json5`. A missing file is not an error — it yields the
/// default (empty) store. The read is retried briefly past a mid-write; bytes
/// are decoded lossily so one stray non-UTF-8 byte doesn't hide every ticket.
pub fn load(root: &Path) -> std::result::Result<PmData, LoadError> {
    load_in(&root.join(".pm"))
}

/// Like [`load`], but `dir` is the directory that directly contains `pm.json5`
/// (`<root>/.pm`, or an out-of-repo store under `~/.pm/` — PM-34).
pub fn load_in(dir: &Path) -> std::result::Result<PmData, LoadError> {
    let path = dir.join(FILE);
    let mut err = LoadError::Io(format!("reading {}", path.display()));
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(12));
        }
        match std::fs::read(&path) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                match json5::from_str::<PmData>(&text) {
                    Ok(d) => return Ok(d),
                    // Broken content the user needs to see — but it can also be
                    // a torn read from a non-atomic external writer, so retry.
                    Err(e) => err = LoadError::Parse(format!("parsing {}: {e}", path.display())),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PmData::default()),
            // PermissionDenied on Windows is a sharing violation (someone else
            // has the file open) — transient. Everything else here too.
            Err(e) => err = LoadError::Io(format!("reading {}: {e}", path.display())),
        }
    }
    Err(err)
}

impl PmData {
    /// Pretty-printed JSON text (a valid JSON5 subset).
    pub fn to_pretty(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }

    /// Write `<root>/.pm/pm.json5`, creating `.pm/` if needed. Writes a sibling
    /// temp file and renames it into place so a concurrent reader never sees a
    /// half-written file.
    pub fn save(&self, root: &Path) -> Result<()> {
        self.save_in(&root.join(".pm"))
    }

    /// Like [`save`](Self::save), but `dir` directly contains `pm.json5`
    /// (`<root>/.pm`, or an out-of-repo store under `~/.pm/` — PM-34).
    pub fn save_in(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join(FILE);
        let tmp = dir.join(format!(".{FILE}.{}.tmp", std::process::id()));
        std::fs::write(&tmp, self.to_pretty())
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).or_else(|_| {
            // Rare on Windows if a reader holds the target; fall back to a
            // direct write and drop the temp.
            let r = std::fs::write(&path, self.to_pretty());
            let _ = std::fs::remove_file(&tmp);
            r
        })
        .with_context(|| format!("replacing {}", path.display()))
    }

    pub fn ticket(&self, id: u64) -> Option<&Ticket> {
        self.tickets.iter().find(|t| t.id == id)
    }

    fn ticket_mut(&mut self, id: u64) -> Option<&mut Ticket> {
        self.tickets.iter_mut().find(|t| t.id == id)
    }

    /// Human id: `"PM-3"` when a project key is set, else `"#3"`.
    pub fn display_id(&self, t: &Ticket) -> String {
        match self.project.key.as_deref().filter(|k| !k.is_empty()) {
            Some(key) => format!("{key}-{}", t.id),
            None => format!("#{}", t.id),
        }
    }

    /// Set a ticket's status, touching `updated` and recording a
    /// [`HistoryEvent`]. Returns whether anything changed (unknown ticket /
    /// same status → `false`).
    pub fn set_status(
        &mut self,
        ticket_id: u64,
        status: Status,
        author: impl Into<String>,
        now: i64,
    ) -> bool {
        match self.ticket_mut(ticket_id) {
            Some(t) if t.status != status => {
                let old = std::mem::replace(&mut t.status, status);
                t.updated = now;
                t.history.push(HistoryEntry {
                    at: now,
                    author: author.into(),
                    event: HistoryEvent::StatusChanged { old, new: status },
                });
                true
            }
            _ => false,
        }
    }

    /// Create a ticket, returning its new id. Bumps `next_id`.
    pub fn create_ticket(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        author: impl Into<String>,
        now: i64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tickets.push(Ticket {
            id,
            title: title.into(),
            body: body.into(),
            status: Status::default(),
            priority: Priority::default(),
            labels: Vec::new(),
            assignee: None,
            author: author.into(),
            created: now,
            updated: now,
            anchors: Vec::new(),
            comments: Vec::new(),
            history: Vec::new(),
        });
        id
    }

    /// Set a ticket's title, touching `updated` and recording a [`HistoryEvent`].
    /// Returns whether anything changed.
    pub fn set_title(
        &mut self,
        ticket_id: u64,
        title: impl Into<String>,
        author: impl Into<String>,
        now: i64,
    ) -> bool {
        let title = title.into();
        match self.ticket_mut(ticket_id) {
            Some(t) if t.title != title => {
                let old = std::mem::replace(&mut t.title, title.clone());
                t.updated = now;
                t.history.push(HistoryEntry {
                    at: now,
                    author: author.into(),
                    event: HistoryEvent::TitleChanged { old, new: title },
                });
                true
            }
            _ => false,
        }
    }

    /// Set a ticket's body, touching `updated` and recording a [`HistoryEvent`].
    /// Returns whether anything changed.
    pub fn set_body(
        &mut self,
        ticket_id: u64,
        body: impl Into<String>,
        author: impl Into<String>,
        now: i64,
    ) -> bool {
        let body = body.into();
        match self.ticket_mut(ticket_id) {
            Some(t) if t.body != body => {
                let old = std::mem::replace(&mut t.body, body.clone());
                t.updated = now;
                t.history.push(HistoryEntry {
                    at: now,
                    author: author.into(),
                    event: HistoryEvent::BodyChanged { old, new: body },
                });
                true
            }
            _ => false,
        }
    }

    /// Set a ticket's priority, touching `updated` and recording a
    /// [`HistoryEvent`]. Returns whether anything changed.
    pub fn set_priority(
        &mut self,
        ticket_id: u64,
        priority: Priority,
        author: impl Into<String>,
        now: i64,
    ) -> bool {
        match self.ticket_mut(ticket_id) {
            Some(t) if t.priority != priority => {
                let old = std::mem::replace(&mut t.priority, priority);
                t.updated = now;
                t.history.push(HistoryEntry {
                    at: now,
                    author: author.into(),
                    event: HistoryEvent::PriorityChanged { old, new: priority },
                });
                true
            }
            _ => false,
        }
    }

    /// Set a ticket's assignee, touching `updated` and recording a
    /// [`HistoryEvent`]. Returns whether anything changed.
    pub fn set_assignee(
        &mut self,
        ticket_id: u64,
        assignee: Option<String>,
        author: impl Into<String>,
        now: i64,
    ) -> bool {
        match self.ticket_mut(ticket_id) {
            Some(t) if t.assignee != assignee => {
                let old = std::mem::replace(&mut t.assignee, assignee.clone());
                t.updated = now;
                t.history.push(HistoryEntry {
                    at: now,
                    author: author.into(),
                    event: HistoryEvent::AssigneeChanged { old, new: assignee },
                });
                true
            }
            _ => false,
        }
    }

    /// Replace a ticket's labels, touching `updated` and recording a
    /// [`HistoryEvent`]. Returns whether anything changed.
    pub fn set_labels(
        &mut self,
        ticket_id: u64,
        labels: Vec<String>,
        author: impl Into<String>,
        now: i64,
    ) -> bool {
        match self.ticket_mut(ticket_id) {
            Some(t) if t.labels != labels => {
                let old = std::mem::replace(&mut t.labels, labels.clone());
                t.updated = now;
                t.history.push(HistoryEntry {
                    at: now,
                    author: author.into(),
                    event: HistoryEvent::LabelsChanged { old, new: labels },
                });
                true
            }
            _ => false,
        }
    }

    /// Append a comment to a ticket, touch its `updated`, and record a
    /// [`HistoryEvent`]. Returns `false` if the ticket doesn't exist.
    pub fn add_comment(
        &mut self,
        ticket_id: u64,
        author: impl Into<String>,
        body: impl Into<String>,
        now: i64,
    ) -> bool {
        let Some(t) = self.ticket_mut(ticket_id) else {
            return false;
        };
        let author = author.into();
        let id = t.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        t.comments.push(Comment {
            id,
            author: author.clone(),
            created: now,
            body: body.into(),
            anchor: None,
        });
        t.updated = now;
        t.history.push(HistoryEntry {
            at: now,
            author,
            event: HistoryEvent::Commented { comment_id: id },
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "pm-json5-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn missing_file_is_default() {
        let d = tmp();
        let data = load(&d).unwrap();
        assert_eq!(data, PmData::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn round_trips() {
        let d = tmp();
        let mut data = PmData::default();
        data.project.key = Some("PM".into());
        let id = data.create_ticket("first", "body\nwith newline", "alice", 1000);
        assert_eq!(data.ticket(id).unwrap().author, "alice");
        assert!(data.add_comment(id, "me", "a comment", 1100));
        data.save(&d).unwrap();

        let back = load(&d).unwrap();
        assert_eq!(back, data);
        assert_eq!(back.display_id(back.ticket(id).unwrap()), "PM-1");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reads_hand_written_json5() {
        let d = tmp();
        std::fs::create_dir_all(d.join(".pm")).unwrap();
        std::fs::write(
            d.join(".pm").join(FILE),
            r#"{
                // a hand-written file
                version: 1,
                project: { name: "pm", key: "PM" },
                next_id: 2,
                tickets: [
                    { id: 1, title: "hi", status: "in_progress", priority: "high" },
                ],
            }"#,
        )
        .unwrap();
        let data = load(&d).unwrap();
        assert_eq!(data.next_id, 2);
        assert_eq!(data.tickets[0].status, Status::InProgress);
        assert_eq!(data.tickets[0].priority, Priority::High);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn create_ticket_is_monotonic() {
        let mut data = PmData::default();
        let a = data.create_ticket("a", "", "", 0);
        let b = data.create_ticket("b", "", "", 0);
        assert_eq!((a, b), (1, 2));
        assert_eq!(data.next_id, 3);
    }

    #[test]
    fn malformed_is_parse_error() {
        let d = tmp();
        std::fs::create_dir_all(d.join(".pm")).unwrap();
        std::fs::write(d.join(".pm").join(FILE), "{ not valid").unwrap();
        assert!(matches!(load(&d), Err(LoadError::Parse(_))));
        let _ = std::fs::remove_dir_all(&d);
    }
}
