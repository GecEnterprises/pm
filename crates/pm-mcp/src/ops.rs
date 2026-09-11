//! The actual work behind each MCP tool — plain functions over `pm-core`, with
//! no `rmcp` types in sight so they can be unit-tested directly.
//!
//! Every mutating op uses pm-core's cross-process transaction API; a running
//! `pm` GUI notices the file change through its filesystem watch.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use pm_core::pm::{self, PmData, Priority, Status};
use pm_core::{resolve_author, Config, Repo};

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

fn parse_status(s: &str) -> Result<Status> {
    serde_json::from_value(Value::String(s.to_string())).with_context(|| {
        format!("unknown status {s:?} (open, in_progress, blocked, done, wontfix)")
    })
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
        "parent": data.parent_id(t.id),
        "children": data.child_progress(t.id),
        "blockers": data.blockers(t.id).iter().map(|t| t.id).collect::<Vec<_>>(),
        "dependencies_satisfied": data.dependencies_satisfied(t.id),
    })
}

fn ticket_full(data: &PmData, t: &pm_core::Ticket) -> Value {
    let mut v = serde_json::to_value(t).unwrap_or_else(|_| json!({}));
    if let Value::Object(ref mut m) = v {
        m.insert("display_id".into(), json!(data.display_id(t)));
        m.insert("parent".into(), json!(data.parent_id(t.id)));
        m.insert("child_progress".into(), json!(data.child_progress(t.id)));
        m.insert(
            "children".into(),
            json!(data.children(t.id).iter().map(|t| t.id).collect::<Vec<_>>()),
        );
        m.insert(
            "dependencies_satisfied".into(),
            json!(data.dependencies_satisfied(t.id)),
        );
        m.insert(
            "incoming_links".into(),
            json!(data
                .tickets
                .iter()
                .flat_map(|source| source
                    .links
                    .iter()
                    .filter(move |l| l.target == t.id)
                    .map(move |l| json!({"source":source.id,"kind":l.kind})))
                .collect::<Vec<_>>()),
        );
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
    let author = resolve_author(author, &Repo::open(root));
    let (data, ()) = pm::transact_in(&store_dir(root), |data| {
        if !data.add_comment(id, author.clone(), body, pm::now_unix()) {
            bail!("no ticket with id {id}");
        }
        Ok(((), true))
    })?;
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
    let author = resolve_author(author, &Repo::open(root));
    let now = pm::now_unix();
    let parsed_priority = priority.map(parse_priority).transpose()?;
    let (data, id) = pm::transact_in(&store_dir(root), |data| {
        let id = data.create_ticket(title, body.unwrap_or_default(), author.clone(), now);
        if let Some(p) = parsed_priority {
            data.set_priority(id, p, author.clone(), now);
        }
        if let Some(l) = labels {
            data.set_labels(id, l, author.clone(), now);
        }
        Ok((id, true))
    })?;
    let display = data
        .ticket(id)
        .map(|t| data.display_id(t))
        .unwrap_or_default();
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
    author: Option<&str>,
) -> Result<Value> {
    let author = resolve_author(author, &Repo::open(root));
    let now = pm::now_unix();
    let parsed_status = status.map(parse_status).transpose()?;
    let parsed_priority = priority.map(parse_priority).transpose()?;
    // `assignee`: a string sets it, JSON null clears it, absent leaves it.
    let assignee = assignee
        .map(|a| match a {
            Value::Null => Ok(None),
            Value::String(s) => Ok(Some(s)),
            other => bail!("assignee must be a string or null, got {other}"),
        })
        .transpose()?;
    let (_, changed) = pm::transact_in(&store_dir(root), |data| {
        if data.ticket(id).is_none() {
            bail!("no ticket with id {id}");
        }
        let mut changed = Vec::new();
        if let Some(v) = title {
            if data.set_title(id, v, author.clone(), now) {
                changed.push("title");
            }
        }
        if let Some(v) = body {
            if data.set_body(id, v, author.clone(), now) {
                changed.push("body");
            }
        }
        if let Some(v) = parsed_status {
            if data.set_status(id, v, author.clone(), now) {
                changed.push("status");
            }
        }
        if let Some(v) = parsed_priority {
            if data.set_priority(id, v, author.clone(), now) {
                changed.push("priority");
            }
        }
        if let Some(v) = labels {
            if data.set_labels(id, v, author.clone(), now) {
                changed.push("labels");
            }
        }
        if let Some(next) = assignee {
            if data.set_assignee(id, next, author.clone(), now) {
                changed.push("assignee");
            }
        }
        let dirty = !changed.is_empty();
        Ok((changed, dirty))
    })?;
    Ok(json!({ "ok": true, "id": id, "changed": changed }))
}

pub fn change_link(
    root: &Path,
    source: u64,
    target: u64,
    kind: &str,
    remove: bool,
    author: Option<&str>,
) -> Result<Value> {
    let kind = serde_json::from_value::<pm_core::relations::LinkKind>(json!(kind))
        .context("kind must be parent_of, blocks, relates, closes or duplicate_of")?;
    let author = resolve_author(author, &Repo::open(root));
    let (_, changed) = pm::transact_in(&store_dir(root), |data| {
        let changed = data.change_link(source, target, kind, remove, &author, pm::now_unix())?;
        Ok((changed, changed))
    })?;
    Ok(json!({"ok":true,"changed":changed}))
}

pub fn set_parent(
    root: &Path,
    child: u64,
    parent: Option<u64>,
    author: Option<&str>,
) -> Result<Value> {
    let author = resolve_author(author, &Repo::open(root));
    let (_, changed) = pm::transact_in(&store_dir(root), |data| {
        let changed = data.set_parent(child, parent, &author, pm::now_unix())?;
        Ok((changed, changed))
    })?;
    Ok(json!({"ok":true,"changed":changed,"child":child,"parent":parent}))
}

pub fn open_project(root: &Path) -> Result<Value> {
    let bin = locate_gui().ok_or_else(|| {
        anyhow!("could not find the `pm` GUI binary (set PM_BIN, or put `pm` on PATH)")
    })?;
    std::process::Command::new(&bin)
        .arg(root)
        .spawn()
        .with_context(|| format!("launching {}", bin.display()))?;
    Ok(
        json!({ "ok": true, "launched": bin.display().to_string(), "project": root.display().to_string() }),
    )
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
            // Prefer a sibling matching this server's own binary stem, so a
            // `pm-debug --mcp` launches the `pm-debug` GUI, not the release one
            // (PM-88). Fall back to `pm`.
            let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("pm");
            let mut names = vec![format!("{stem}.exe"), stem.to_string()];
            names.push("pm.exe".to_string());
            names.push("pm".to_string());
            for name in names {
                let cand = dir.join(&name);
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
        let c =
            create_ticket(&d, "hello", Some("body"), Some("alice"), Some("high"), None).unwrap();
        let id = c["id"].as_u64().unwrap();
        assert_eq!(c["display_id"], "T-1");

        add_comment(&d, id, "a note", Some("bob")).unwrap();
        let t = get_ticket(&d, id).unwrap();
        assert_eq!(t["author"], "alice");
        assert_eq!(t["priority"], "high");
        assert_eq!(t["comments"][0]["author"], "bob");

        edit_ticket(
            &d,
            id,
            None,
            None,
            Some("done"),
            None,
            None,
            None,
            Some("carol"),
        )
        .unwrap();
        let list = list_tickets(&d, Some("done"), None).unwrap();
        assert_eq!(list["count"], 1);
        let none = list_tickets(&d, Some("open"), None).unwrap();
        assert_eq!(none["count"], 0);

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn history_records_edits_and_comments() {
        let d = tmp_project();
        let id = create_ticket(&d, "hello", None, Some("alice"), None, None).unwrap()["id"]
            .as_u64()
            .unwrap();
        add_comment(&d, id, "a note", Some("bob")).unwrap();
        edit_ticket(
            &d,
            id,
            Some("bye"),
            None,
            Some("done"),
            None,
            None,
            None,
            Some("carol"),
        )
        .unwrap();

        let t = get_ticket(&d, id).unwrap();
        let history = t["history"].as_array().unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0]["kind"], "commented");
        assert_eq!(history[0]["author"], "bob");
        assert_eq!(history[1]["kind"], "title_changed");
        assert_eq!(history[1]["old"], "hello");
        assert_eq!(history[1]["new"], "bye");
        assert_eq!(history[1]["author"], "carol");
        assert_eq!(history[2]["kind"], "status_changed");
        assert_eq!(history[2]["old"], "open");
        assert_eq!(history[2]["new"], "done");

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
    fn relation_tools_round_trip_and_reparent_atomically() {
        let root = tmp_project();
        let parent = create_ticket(&root, "Phase", None, Some("test"), None, None).unwrap()["id"]
            .as_u64()
            .unwrap();
        let child = create_ticket(&root, "Slice", None, Some("test"), None, None).unwrap()["id"]
            .as_u64()
            .unwrap();
        let blocker = create_ticket(&root, "Prerequisite", None, Some("test"), None, None).unwrap()
            ["id"]
            .as_u64()
            .unwrap();
        assert_eq!(
            set_parent(&root, child, Some(parent), Some("test")).unwrap()["changed"],
            true
        );
        assert_eq!(
            change_link(&root, blocker, child, "blocks", false, Some("test")).unwrap()["changed"],
            true
        );
        let full = get_ticket(&root, child).unwrap();
        assert_eq!(full["parent"], parent);
        assert_eq!(full["incoming_links"][0]["source"], parent);
        assert_eq!(full["incoming_links"][1]["source"], blocker);
        assert_eq!(full["dependencies_satisfied"], false);
        assert!(set_parent(&root, child, Some(child), Some("test")).is_err());
        assert_eq!(get_ticket(&root, child).unwrap()["parent"], parent);
        let _ = std::fs::remove_dir_all(root);
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
