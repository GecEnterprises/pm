//! The actual work behind each MCP tool — plain functions over `pm-core`, with
//! no `rmcp` types in sight so they can be unit-tested directly.
//!
//! Every mutating op is load → mutate → atomic save (`PmData::save`); a running
//! `pm` GUI notices the file change through its filesystem watch.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use pm_core::pm::{self, PmData, Priority, Status};
use pm_core::{resolve_author, Config, Repo};

/// Serializes every read-modify-write of a `pm.json5` in this process. `rmcp`
/// runs tool calls concurrently, so two writes in one client batch would
/// otherwise load the same base and the last save would clobber the first.
/// (Cross-process races are still handled by `pm::load`'s torn-read retry +
/// `PmData::save`'s atomic rename.)
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// Resolve a project root from an optional path argument (falling back to
/// `default`). If the chosen directory has no `.pm/pm.json5`, walk up until one
/// is found; otherwise use the directory as-is (a fresh project).
pub fn resolve_root(arg: Option<&str>, default: &Path) -> PathBuf {
    let start = arg
        .map(PathBuf::from)
        .unwrap_or_else(|| default.to_path_buf());
    let start = std::fs::canonicalize(&start).unwrap_or(start);
    if store_path(&start).is_file() {
        return start;
    }
    let mut dir = start.as_path();
    while let Some(parent) = dir.parent() {
        if store_path(parent).is_file() {
            return parent.to_path_buf();
        }
        dir = parent;
    }
    start
}

/// The directory holding this project's `pm.json5` — the in-repo `.pm/` by
/// default, or an out-of-repo store from the `~/.pm/config.json` registry
/// (PM-34).
fn store_dir(root: &Path) -> PathBuf {
    Config::load().resolve_store_dir(root)
}

fn store_path(root: &Path) -> PathBuf {
    store_dir(root).join("pm.json5")
}

fn load(root: &Path) -> Result<PmData> {
    pm::load_in(&store_dir(root)).map_err(|e| anyhow!("{e}"))
}

fn save(data: &PmData, root: &Path) -> Result<()> {
    data.save_in(&store_dir(root))
}

fn parse_status(s: &str) -> Result<Status> {
    serde_json::from_value(Value::String(s.to_string()))
        .with_context(|| format!("unknown status {s:?} (open, in_progress, blocked, done, wontfix)"))
}

fn parse_priority(s: &str) -> Result<Priority> {
    serde_json::from_value(Value::String(s.to_string()))
        .with_context(|| format!("unknown priority {s:?} (low, normal, high, urgent)"))
}

fn ticket_summary(data: &PmData, t: &pm_core::Ticket) -> Value {
    json!({
        "id": t.id,
        "display_id": data.display_id(t),
        "title": t.title,
        "status": t.status,
        "priority": t.priority,
        "author": t.author,
        "labels": t.labels,
        "assignee": t.assignee,
        "updated": t.updated,
        "comments": t.comments.len(),
    })
}

fn ticket_full(data: &PmData, t: &pm_core::Ticket) -> Value {
    let mut v = serde_json::to_value(t).unwrap_or_else(|_| json!({}));
    if let Value::Object(ref mut m) = v {
        m.insert("display_id".into(), json!(data.display_id(t)));
    }
    v
}

pub fn list_tickets(root: &Path, status: Option<&str>, label: Option<&str>) -> Result<Value> {
    let data = load(root)?;
    let want_status = status.map(parse_status).transpose()?;
    let out: Vec<Value> = data
        .tickets
        .iter()
        .filter(|t| want_status.is_none_or(|s| t.status == s))
        .filter(|t| label.is_none_or(|l| t.labels.iter().any(|x| x == l)))
        .map(|t| ticket_summary(&data, t))
        .collect();
    Ok(json!({ "project": data.project.name, "count": out.len(), "tickets": out }))
}

pub fn get_ticket(root: &Path, id: u64) -> Result<Value> {
    let data = load(root)?;
    let t = data
        .ticket(id)
        .ok_or_else(|| anyhow!("no ticket with id {id}"))?;
    Ok(ticket_full(&data, t))
}

pub fn add_comment(root: &Path, id: u64, body: &str, author: Option<&str>) -> Result<Value> {
    if body.trim().is_empty() {
        bail!("comment body is empty");
    }
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut data = load(root)?;
    let author = resolve_author(author, &Repo::open(root));
    if !data.add_comment(id, author.clone(), body, pm::now_unix()) {
        bail!("no ticket with id {id}");
    }
    save(&data, root)?;
    let comment_id = data
        .ticket(id)
        .and_then(|t| t.comments.last())
        .map(|c| c.id)
        .unwrap_or(0);
    Ok(json!({ "ok": true, "ticket": id, "comment_id": comment_id, "author": author }))
}

#[allow(clippy::too_many_arguments)]
pub fn create_ticket(
    root: &Path,
    title: &str,
    body: Option<&str>,
    author: Option<&str>,
    priority: Option<&str>,
    labels: Option<Vec<String>>,
) -> Result<Value> {
    if title.trim().is_empty() {
        bail!("ticket title is empty");
    }
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut data = load(root)?;
    let author = resolve_author(author, &Repo::open(root));
    let now = pm::now_unix();
    let id = data.create_ticket(title, body.unwrap_or_default(), author.clone(), now);
    if let Some(p) = priority {
        data.set_priority(id, parse_priority(p)?, now);
    }
    if let Some(l) = labels {
        data.set_labels(id, l, now);
    }
    save(&data, root)?;
    let display = data.ticket(id).map(|t| data.display_id(t)).unwrap_or_default();
    Ok(json!({ "ok": true, "id": id, "display_id": display, "author": author }))
}

#[allow(clippy::too_many_arguments)]
pub fn edit_ticket(
    root: &Path,
    id: u64,
    title: Option<&str>,
    body: Option<&str>,
    status: Option<&str>,
    priority: Option<&str>,
    labels: Option<Vec<String>>,
    assignee: Option<Value>,
) -> Result<Value> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut data = load(root)?;
    if data.ticket(id).is_none() {
        bail!("no ticket with id {id}");
    }
    let now = pm::now_unix();
    let mut changed = Vec::new();
    if let Some(v) = title {
        if data.set_title(id, v, now) {
            changed.push("title");
        }
    }
    if let Some(v) = body {
        if data.set_body(id, v, now) {
            changed.push("body");
        }
    }
    if let Some(v) = status {
        if data.set_status(id, parse_status(v)?, now) {
            changed.push("status");
        }
    }
    if let Some(v) = priority {
        if data.set_priority(id, parse_priority(v)?, now) {
            changed.push("priority");
        }
    }
    if let Some(v) = labels {
        if data.set_labels(id, v, now) {
            changed.push("labels");
        }
    }
    // `assignee`: a string sets it, JSON null clears it, absent leaves it.
    if let Some(a) = assignee {
        let next = match a {
            Value::Null => None,
            Value::String(s) => Some(s),
            other => bail!("assignee must be a string or null, got {other}"),
        };
        if data.set_assignee(id, next, now) {
            changed.push("assignee");
        }
    }
    if !changed.is_empty() {
        save(&data, root)?;
    }
    Ok(json!({ "ok": true, "id": id, "changed": changed }))
}

pub fn open_project(root: &Path) -> Result<Value> {
    let bin = locate_gui().ok_or_else(|| {
        anyhow!("could not find the `pm` GUI binary (set PM_BIN, or put `pm` on PATH)")
    })?;
    std::process::Command::new(&bin)
        .arg(root)
        .spawn()
        .with_context(|| format!("launching {}", bin.display()))?;
    Ok(json!({ "ok": true, "launched": bin.display().to_string(), "project": root.display().to_string() }))
}

fn locate_gui() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PM_BIN") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["pm.exe", "pm"] {
                let cand = dir.join(name);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    // Last resort: let the OS resolve it on PATH.
    Some(PathBuf::from("pm"))
}

pub fn list_projects(root: &Path, depth: usize) -> Result<Value> {
    let mut found = Vec::new();
    scan(root, depth, &mut found);
    // Fold in projects whose store lives outside any repo (PM-34).
    for s in Config::load().stores {
        if s.dir.join("pm.json5").is_file() && !found.contains(&s.root) {
            found.push(s.root);
        }
    }
    let projects: Vec<Value> = found
        .into_iter()
        .filter_map(|dir| {
            let data = pm::load_in(&store_dir(&dir)).ok()?;
            Some(json!({
                "path": dir.display().to_string(),
                "name": data.project.name,
                "key": data.project.key,
                "tickets": data.tickets.len(),
            }))
        })
        .collect();
    Ok(json!({ "count": projects.len(), "projects": projects }))
}

fn scan(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if store_path(dir).is_file() {
        out.push(dir.to_path_buf());
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == ".git" || name == "target" || name == "node_modules" || name == ".pm" {
                continue;
            }
            scan(&p, depth - 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_project() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "pm-mcp-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".pm")).unwrap();
        std::fs::write(
            d.join(".pm").join("pm.json5"),
            r#"{ "version": 1, "project": { "name": "t", "key": "T" }, "next_id": 1, "tickets": [] }"#,
        )
        .unwrap();
        d
    }

    #[test]
    fn create_comment_edit_roundtrip() {
        let d = tmp_project();
        let c = create_ticket(&d, "hello", Some("body"), Some("alice"), Some("high"), None).unwrap();
        let id = c["id"].as_u64().unwrap();
        assert_eq!(c["display_id"], "T-1");

        add_comment(&d, id, "a note", Some("bob")).unwrap();
        let t = get_ticket(&d, id).unwrap();
        assert_eq!(t["author"], "alice");
        assert_eq!(t["priority"], "high");
        assert_eq!(t["comments"][0]["author"], "bob");

        edit_ticket(&d, id, None, None, Some("done"), None, None, None).unwrap();
        let list = list_tickets(&d, Some("done"), None).unwrap();
        assert_eq!(list["count"], 1);
        let none = list_tickets(&d, Some("open"), None).unwrap();
        assert_eq!(none["count"], 0);

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn concurrent_writes_all_land() {
        let d = tmp_project();
        let id = create_ticket(&d, "race", None, None, None, None).unwrap()["id"]
            .as_u64()
            .unwrap();
        std::thread::scope(|s| {
            for i in 0..12 {
                let d = &d;
                s.spawn(move || {
                    add_comment(d, id, &format!("c{i}"), Some("t")).unwrap();
                });
            }
        });
        let t = get_ticket(&d, id).unwrap();
        assert_eq!(t["comments"].as_array().unwrap().len(), 12);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resolve_root_walks_up() {
        let d = tmp_project();
        let sub = d.join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            std::fs::canonicalize(resolve_root(Some(sub.to_str().unwrap()), &d)).unwrap(),
            std::fs::canonicalize(&d).unwrap()
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
