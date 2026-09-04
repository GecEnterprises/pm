//! The Sentinel: a recursive filesystem watcher over the repo working tree.
//! It accumulates changed paths and, once things have been quiet for a beat,
//! hands them to `Pm` so the changes list, file tree, and open diff refresh
//! without a manual rescan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Default)]
struct Pending {
    paths: HashSet<PathBuf>,
    last_event: Option<Instant>,
}

pub struct Sentinel {
    // Dropping the watcher stops the OS notifications, so keep it alive.
    _watcher: RecommendedWatcher,
    pending: Arc<Mutex<Pending>>,
}

impl Sentinel {
    /// Watch `root` recursively. `extra` is an out-of-repo ticket-store directory
    /// (`~/.pm/projects/<slug>/` — PM-34); when set it's watched too so external
    /// edits to `pm.json5` still reach the GUI.
    pub fn start(root: PathBuf, extra: Option<PathBuf>) -> Result<Self> {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let sink = pending.clone();
        let filter_root = root.clone();
        let filter_extra = extra.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            let mut guard = sink.lock().unwrap();
            let mut hit = false;
            for path in event.paths {
                if relevant(&path, &filter_root)
                    || filter_extra.as_ref().is_some_and(|e| path.starts_with(e))
                {
                    guard.paths.insert(path);
                    hit = true;
                }
            }
            if hit {
                guard.last_event = Some(Instant::now());
            }
        })?;
        watcher.watch(&root, RecursiveMode::Recursive)?;
        if let Some(extra) = &extra {
            // The store dir may not exist yet (Home store chosen, no ticket
            // written). Create it so the watch takes; a failure here is not
            // fatal — the GUI still reads/writes the file directly.
            let _ = std::fs::create_dir_all(extra);
            let _ = watcher.watch(extra, RecursiveMode::NonRecursive);
        }

        Ok(Self {
            _watcher: watcher,
            pending,
        })
    }

    /// The accumulated changed paths, once nothing has changed for `quiet`.
    /// `None` while events are still settling or nothing is pending.
    pub fn poll(&self, quiet: Duration) -> Option<Vec<PathBuf>> {
        let mut guard = self.pending.lock().unwrap();
        if guard.last_event?.elapsed() < quiet {
            return None;
        }
        guard.last_event = None;
        let paths: Vec<PathBuf> = guard.paths.drain().collect();
        (!paths.is_empty()).then_some(paths)
    }
}

/// Filter out noisy paths — build output and the VCS internals we don't react to.
fn relevant(path: &Path, root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let mut comps = rel.components().map(|c| c.as_os_str().to_str().unwrap_or_default());
    match comps.next() {
        Some("target") | Some("node_modules") | Some(".jj") => false,
        Some(".git") => matches!(
            comps.next(),
            Some("index") | Some("HEAD") | Some("ORIG_HEAD") | Some("MERGE_HEAD") | Some("refs")
        ),
        _ => true,
    }
}
