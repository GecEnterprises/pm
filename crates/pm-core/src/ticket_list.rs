//! Shared ticket-list ordering and filtering; user preferences never reorder the store.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{PmData, Priority, Status, Ticket};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketSort {
    #[default]
    Updated,
    Priority,
    Created,
}

impl TicketSort {
    pub const ALL: [Self; 3] = [Self::Updated, Self::Priority, Self::Created];

    pub fn label(self) -> &'static str {
        match self {
            Self::Updated => "Latest updated",
            Self::Priority => "Priority",
            Self::Created => "Newest created",
        }
    }

    pub fn sort(self, tickets: &mut Vec<&Ticket>) {
        fn priority(p: Priority) -> u8 {
            match p {
                Priority::Urgent => 0,
                Priority::High => 1,
                Priority::Normal => 2,
                Priority::Low => 3,
            }
        }
        tickets.sort_by(|a, b| {
            match self {
                Self::Updated => b.updated.cmp(&a.updated),
                Self::Priority => priority(a.priority)
                    .cmp(&priority(b.priority))
                    .then_with(|| b.updated.cmp(&a.updated)),
                Self::Created => b.created.cmp(&a.created),
            }
            .then_with(|| a.id.cmp(&b.id))
        });
    }
}

pub fn active_statuses() -> HashSet<Status> {
    [Status::Open, Status::InProgress, Status::Blocked]
        .into_iter()
        .collect()
}

/// Label filters require all selected labels, combined with statuses and the query.
pub fn shown_tickets<'a>(
    data: &'a PmData,
    statuses: &HashSet<Status>,
    labels: &HashSet<String>,
    query: &str,
    sort: TicketSort,
) -> Vec<&'a Ticket> {
    let query = query.trim().to_lowercase();
    let mut tickets = data
        .tickets
        .iter()
        .filter(|t| {
            statuses.contains(&t.status)
                && labels.iter().all(|label| t.labels.contains(label))
                && (query.is_empty()
                    || t.title.to_lowercase().contains(&query)
                    || data.display_id(t).to_lowercase().contains(&query))
        })
        .collect();
    sort.sort(&mut tickets);
    tickets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorting_and_combined_filters_leave_store_order_intact() {
        let mut data = PmData::default();
        let a = data.create_ticket("Search exploration", "", "test", 10);
        let b = data.create_ticket("Search implementation", "", "test", 20);
        let c = data.create_ticket("Other work", "", "test", 30);
        data.set_labels(a, vec!["search".into(), "exploration".into()], "test", 40);
        data.set_labels(b, vec!["search".into()], "test", 40);
        data.set_priority(b, Priority::Urgent, "test", 40);
        let ids = |sort, labels: HashSet<String>, query: &str| {
            shown_tickets(&data, &active_statuses(), &labels, query, sort)
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(TicketSort::Updated, HashSet::new(), ""), vec![a, b, c]);
        assert_eq!(ids(TicketSort::Priority, HashSet::new(), ""), vec![b, a, c]);
        assert_eq!(ids(TicketSort::Created, HashSet::new(), ""), vec![c, b, a]);
        assert_eq!(
            ids(
                TicketSort::Updated,
                ["search".into(), "exploration".into()].into(),
                "SEARCH"
            ),
            vec![a]
        );
        assert!(ids(TicketSort::Updated, ["missing".into()].into(), "").is_empty());
        assert_eq!(
            data.tickets.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![a, b, c]
        );
        data.set_status(a, Status::Done, "test", 50);
        assert_eq!(
            shown_tickets(
                &data,
                &active_statuses(),
                &HashSet::new(),
                "exploration",
                TicketSort::Updated
            )
            .len(),
            0
        );
    }
}
