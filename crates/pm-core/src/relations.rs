//! Canonical ticket relations, validation and derived views (PM-50).
use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::ticket_list::TicketSort;
use crate::{HistoryEntry, HistoryEvent, PmData, Status, Ticket};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    ParentOf,
    Blocks,
    Relates,
    Closes,
    DuplicateOf,
}

impl LinkKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ParentOf => "Parent of",
            Self::Blocks => "Blocks",
            Self::Relates => "Related to",
            Self::Closes => "Closes",
            Self::DuplicateOf => "Duplicate of",
        }
    }
    pub fn inverse_label(self) -> &'static str {
        match self {
            Self::ParentOf => "Parent",
            Self::Blocks => "Blocked by",
            Self::Relates => "Related to",
            Self::Closes => "Closed by",
            Self::DuplicateOf => "Duplicates",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub kind: LinkKind,
    pub target: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ChildProgress {
    pub total: usize,
    pub done: usize,
    pub cancelled: usize,
}

impl ChildProgress {
    pub fn label(self) -> String {
        let mut text = format!("{} / {} children done", self.done, self.total);
        if self.cancelled > 0 {
            text.push_str(&format!(" · {} won't fix", self.cancelled));
        }
        text
    }
}

impl PmData {
    pub fn parent_id(&self, child: u64) -> Option<u64> {
        self.tickets
            .iter()
            .find(|t| {
                t.links
                    .iter()
                    .any(|l| l.kind == LinkKind::ParentOf && l.target == child)
            })
            .map(|t| t.id)
    }

    pub fn children(&self, parent: u64) -> Vec<&Ticket> {
        let Some(t) = self.ticket(parent) else {
            return Vec::new();
        };
        let ids: HashSet<_> = t
            .links
            .iter()
            .filter(|l| l.kind == LinkKind::ParentOf)
            .map(|l| l.target)
            .collect();
        self.tickets
            .iter()
            .filter(|t| ids.contains(&t.id))
            .collect()
    }

    pub fn child_progress(&self, parent: u64) -> ChildProgress {
        let children = self.children(parent);
        ChildProgress {
            total: children.len(),
            done: children.iter().filter(|t| t.status == Status::Done).count(),
            cancelled: children
                .iter()
                .filter(|t| t.status == Status::Wontfix)
                .count(),
        }
    }

    pub fn blockers(&self, id: u64) -> Vec<&Ticket> {
        self.tickets
            .iter()
            .filter(|t| {
                t.links
                    .iter()
                    .any(|l| l.kind == LinkKind::Blocks && l.target == id)
            })
            .collect()
    }

    /// Wontfix is not successful delivery; remove an obsolete dependency explicitly.
    pub fn dependencies_satisfied(&self, id: u64) -> bool {
        self.blockers(id).iter().all(|t| t.status == Status::Done)
    }

    fn reaches(&self, start: u64, goal: u64, kind: LinkKind) -> bool {
        let mut stack = vec![start];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if id == goal {
                return true;
            }
            if !seen.insert(id) {
                continue;
            }
            if let Some(t) = self.ticket(id) {
                stack.extend(t.links.iter().filter(|l| l.kind == kind).map(|l| l.target));
            }
        }
        false
    }

    /// Add/remove one edge against the current graph. Inverses are never stored.
    pub fn change_link(
        &mut self,
        mut source: u64,
        mut target: u64,
        kind: LinkKind,
        remove: bool,
        author: &str,
        now: i64,
    ) -> Result<bool> {
        if source == target {
            bail!("A ticket cannot link to itself");
        }
        if kind == LinkKind::Relates && source > target {
            std::mem::swap(&mut source, &mut target);
        }
        let Some(ticket) = self.ticket(source) else {
            bail!("Source ticket {source} does not exist");
        };
        let link = Link { kind, target };
        let exists = ticket.links.contains(&link);
        if remove && !exists {
            return Ok(false);
        }
        if !remove {
            if self.ticket(target).is_none() {
                bail!("Target ticket {target} does not exist");
            }
            if exists {
                return Ok(false);
            }
            if kind == LinkKind::ParentOf && self.parent_id(target).is_some() {
                bail!("Ticket {target} already has a parent; change its parent instead");
            }
            if matches!(
                kind,
                LinkKind::ParentOf | LinkKind::Blocks | LinkKind::DuplicateOf
            ) && self.reaches(target, source, kind)
            {
                bail!("This relation would create a cycle");
            }
        }
        let ticket = self.tickets.iter_mut().find(|t| t.id == source).unwrap();
        if remove {
            ticket.links.retain(|l| l != &link);
        } else {
            ticket.links.push(link);
        }
        self.version = self.version.max(2);
        // Both endpoints have changed from the user's perspective, even though
        // the canonical edge is stored on only one ticket.
        for ticket in self
            .tickets
            .iter_mut()
            .filter(|t| t.id == source || t.id == target)
        {
            ticket.updated = now;
            ticket.history.push(HistoryEntry {
                at: now,
                author: author.into(),
                event: HistoryEvent::RelationChanged {
                    source,
                    target,
                    relation: kind,
                    removed: remove,
                },
            });
        }
        Ok(true)
    }

    /// Reparent atomically: rejected changes leave the old parent/history intact.
    pub fn set_parent(
        &mut self,
        child: u64,
        parent: Option<u64>,
        author: &str,
        now: i64,
    ) -> Result<bool> {
        if self.ticket(child).is_none() {
            bail!("Ticket {child} does not exist");
        }
        if self.parent_id(child) == parent {
            return Ok(false);
        }
        let mut next = self.clone();
        let old_parents: Vec<_> = next
            .tickets
            .iter()
            .filter(|t| {
                t.links
                    .iter()
                    .any(|l| l.kind == LinkKind::ParentOf && l.target == child)
            })
            .map(|t| t.id)
            .collect();
        for old in old_parents {
            next.change_link(old, child, LinkKind::ParentOf, true, author, now)?;
        }
        if let Some(parent) = parent {
            next.change_link(parent, child, LinkKind::ParentOf, false, author, now)?;
        }
        *self = next;
        Ok(true)
    }
}

pub struct HierarchyRow<'a> {
    pub ticket: &'a Ticket,
    pub depth: usize,
    pub context: bool,
    pub has_children: bool,
}

/// Include ancestors as context, sort siblings, and keep filtered descendants
/// reachable. An iterative walk also tolerates malformed hand-written cycles.
pub fn hierarchy_rows<'a>(
    data: &'a PmData,
    matches: &[&'a Ticket],
    sort: TicketSort,
    collapsed: &HashSet<u64>,
    reveal_matches: bool,
) -> Vec<HierarchyRow<'a>> {
    let matched: HashSet<_> = matches.iter().map(|t| t.id).collect();
    let mut included = matched.clone();
    for &id in &matched {
        let mut current = id;
        let mut seen = HashSet::new();
        while let Some(parent) = data.parent_id(current) {
            if !seen.insert(parent) {
                break;
            }
            included.insert(parent);
            current = parent;
        }
    }
    let mut siblings: HashMap<Option<u64>, Vec<&Ticket>> = HashMap::new();
    for t in data.tickets.iter().filter(|t| included.contains(&t.id)) {
        let parent = data.parent_id(t.id).filter(|p| included.contains(p));
        siblings.entry(parent).or_default().push(t);
    }
    for group in siblings.values_mut() {
        sort.sort(group);
    }
    let mut stack: Vec<_> = siblings
        .get(&None)
        .into_iter()
        .flatten()
        .rev()
        .map(|t| (*t, 0))
        .collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    // Orphaned/cyclic components have no root; render each once rather than disappear.
    let mut fallback: Vec<_> = data
        .tickets
        .iter()
        .filter(|t| included.contains(&t.id))
        .collect();
    sort.sort(&mut fallback);
    let mut fallback = fallback.into_iter();
    loop {
        let (ticket, depth) = match stack.pop() {
            Some(row) => row,
            None => match fallback.find(|t| !seen.contains(&t.id)) {
                Some(t) => (t, 0),
                None => break,
            },
        };
        if !seen.insert(ticket.id) {
            continue;
        }
        let children = siblings.get(&Some(ticket.id));
        out.push(HierarchyRow {
            ticket,
            depth,
            context: !matched.contains(&ticket.id),
            has_children: children.is_some_and(|c| !c.is_empty()),
        });
        if reveal_matches || !collapsed.contains(&ticket.id) {
            stack.extend(
                children
                    .into_iter()
                    .flatten()
                    .rev()
                    .map(|t| (*t, depth + 1)),
            );
        } else {
            // Suppressed descendants must not resurface as fallback roots.
            let mut hidden: Vec<_> = children.into_iter().flatten().map(|t| t.id).collect();
            while let Some(id) = hidden.pop() {
                if seen.insert(id) {
                    hidden.extend(siblings.get(&Some(id)).into_iter().flatten().map(|t| t.id));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (PmData, u64, u64, u64, u64) {
        let mut data = PmData::default();
        let parent = data.create_ticket("Phase", "", "test", 1);
        let done = data.create_ticket("Landed slice", "", "test", 2);
        let open = data.create_ticket("Remaining slice", "", "test", 3);
        let cancelled = data.create_ticket("Rejected slice", "", "test", 4);
        data.set_status(done, Status::Done, "test", 5);
        data.set_status(cancelled, Status::Wontfix, "test", 5);
        data.change_link(parent, done, LinkKind::ParentOf, false, "test", 6)
            .unwrap();
        data.change_link(parent, open, LinkKind::ParentOf, false, "test", 6)
            .unwrap();
        data.change_link(parent, cancelled, LinkKind::ParentOf, false, "test", 6)
            .unwrap();
        (data, parent, done, open, cancelled)
    }

    #[test]
    fn parenting_rollup_and_hierarchy_are_derived_from_canonical_edges() {
        let (data, parent, done, open, cancelled) = project();
        assert_eq!(data.parent_id(open), Some(parent));
        assert_eq!(data.child_progress(parent).total, 3);
        assert_eq!(data.child_progress(parent).done, 1);
        assert_eq!(data.child_progress(parent).cancelled, 1);
        let matches = vec![
            data.ticket(done).unwrap(),
            data.ticket(open).unwrap(),
            data.ticket(cancelled).unwrap(),
        ];
        let rows = hierarchy_rows(&data, &matches, TicketSort::Created, &HashSet::new(), true);
        assert_eq!(
            rows.iter()
                .map(|r| (r.ticket.id, r.depth, r.context))
                .collect::<Vec<_>>(),
            vec![
                (parent, 0, true),
                (cancelled, 1, false),
                (open, 1, false),
                (done, 1, false)
            ]
        );
    }

    #[test]
    fn invalid_reparent_and_cycles_leave_the_graph_unchanged() {
        let (mut data, parent, _, open, _) = project();
        let before = data.clone();
        assert!(data.set_parent(parent, Some(open), "test", 7).is_err());
        assert_eq!(data, before);
        let other = data.create_ticket("Other parent", "", "test", 8);
        assert!(data
            .change_link(other, open, LinkKind::ParentOf, false, "test", 9)
            .is_err());
        assert_eq!(data.parent_id(open), Some(parent));
        assert!(data
            .change_link(parent, 999, LinkKind::Blocks, false, "test", 9)
            .is_err());
    }

    #[test]
    fn dependency_resolution_requires_done_and_never_changes_status() {
        let (mut data, _, done, open, cancelled) = project();
        data.change_link(done, open, LinkKind::Blocks, false, "test", 10)
            .unwrap();
        assert!(data.dependencies_satisfied(open));
        assert_eq!(data.ticket(open).unwrap().status, Status::Open);
        data.change_link(cancelled, open, LinkKind::Blocks, false, "test", 11)
            .unwrap();
        assert!(!data.dependencies_satisfied(open));
        assert_eq!(data.ticket(open).unwrap().status, Status::Open);
    }

    #[test]
    fn relations_promote_a_v1_store_to_v2_and_round_trip() {
        let (data, parent, _, open, _) = project();
        assert_eq!(data.version, 2);
        let raw = data.to_pretty();
        assert!(raw.contains("\"version\": 2"));
        let decoded: PmData = json5::from_str(&raw).unwrap();
        assert_eq!(decoded.parent_id(open), Some(parent));
        assert!(decoded
            .ticket(parent)
            .unwrap()
            .history
            .iter()
            .any(|h| matches!(
                h.event,
                HistoryEvent::RelationChanged {
                    relation: LinkKind::ParentOf,
                    ..
                }
            )));
    }
}
