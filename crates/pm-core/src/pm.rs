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
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
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
    /// Unix seconds.
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub updated: i64,
    #[serde(default)]
    pub anchors: Vec<Anchor>,
    #[serde(default)]
    pub comments: Vec<Comment>,
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

/// Read `<root>/.pm/pm.json5`. A missing file is not an error — it yields the
/// default (empty) store. A malformed file *is* an error, so the UI can surface
/// it instead of silently starting blank.
pub fn load(root: &Path) -> Result<PmData> {
    let path = root.join(".pm").join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PmData::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    json5::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

impl PmData {
    /// Pretty-printed JSON text (what `save` writes; also used to dedupe the
    /// watcher event our own write triggers).
    pub fn to_pretty(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }

    /// Write `<root>/.pm/pm.json5`, creating `.pm/` if needed.
    pub fn save(&self, root: &Path) -> Result<()> {
        let dir = root.join(".pm");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join(FILE);
        std::fs::write(&path, self.to_pretty())
            .with_context(|| format!("writing {}", path.display()))
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

    /// Create a ticket, returning its new id. Bumps `next_id`.
    pub fn create_ticket(&mut self, title: impl Into<String>, body: impl Into<String>, now: i64) -> u64 {
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
            created: now,
            updated: now,
            anchors: Vec::new(),
            comments: Vec::new(),
        });
        id
    }

    /// Append a comment to a ticket and touch its `updated`. Returns `false` if
    /// the ticket doesn't exist.
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
        let id = t.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        t.comments.push(Comment {
            id,
            author: author.into(),
            created: now,
            body: body.into(),
            anchor: None,
        });
        t.updated = now;
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
        let id = data.create_ticket("first", "body\nwith newline", 1000);
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
        let a = data.create_ticket("a", "", 0);
        let b = data.create_ticket("b", "", 0);
        assert_eq!((a, b), (1, 2));
        assert_eq!(data.next_id, 3);
    }

    #[test]
    fn malformed_is_error() {
        let d = tmp();
        std::fs::create_dir_all(d.join(".pm")).unwrap();
        std::fs::write(d.join(".pm").join(FILE), "{ not valid").unwrap();
        assert!(load(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
