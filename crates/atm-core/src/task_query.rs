//! Transport-neutral task list and event query contracts.

use serde::{Deserialize, Serialize};

use crate::error::AtmError;
use crate::types::{AgentName, TaskId, TeamName};
use atm_storage::{TaskEventRow, TaskRow};

pub const DEFAULT_TASK_PAGE_LIMIT: usize = 200;
pub const MAX_TASK_PAGE_LIMIT: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskListQuery {
    pub team: TeamName,
    pub assignee: Option<AgentName>,
    pub page: TaskPage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEventQuery {
    pub team: TeamName,
    pub task_id: TaskId,
    pub assignee: Option<AgentName>,
    pub page: TaskPage,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskPage {
    Bounded { limit: usize },
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSelection<T> {
    pub rows: Vec<T>,
    pub omitted: usize,
}

impl TaskPage {
    pub fn bounded(limit: usize) -> Result<Self, AtmError> {
        if !(1..=MAX_TASK_PAGE_LIMIT).contains(&limit) {
            return Err(AtmError::validation(format!(
                "task limit must be between 1 and {MAX_TASK_PAGE_LIMIT}",
            )));
        }
        Ok(Self::Bounded { limit })
    }

    #[must_use]
    pub const fn default_bounded() -> Self {
        Self::Bounded {
            limit: DEFAULT_TASK_PAGE_LIMIT,
        }
    }
}

#[must_use]
pub fn select_task_rows(rows: Vec<TaskRow>, query: &TaskListQuery) -> Vec<TaskRow> {
    select_task_rows_page(rows, query).rows
}

#[must_use]
pub fn select_task_rows_page(
    mut rows: Vec<TaskRow>,
    query: &TaskListQuery,
) -> TaskSelection<TaskRow> {
    rows.retain(|row| {
        row.team == query.team
            && row.state.is_open()
            && query
                .assignee
                .as_ref()
                .is_none_or(|assignee| &row.assignee == assignee)
    });
    rows.sort_by(|left, right| {
        (&left.assignee, left.position, &left.task_id).cmp(&(
            &right.assignee,
            right.position,
            &right.task_id,
        ))
    });
    let omitted = apply_page(&mut rows, query.page, PageEdge::Start);
    TaskSelection { rows, omitted }
}

#[must_use]
pub fn select_task_events(rows: Vec<TaskEventRow>, query: &TaskEventQuery) -> Vec<TaskEventRow> {
    select_task_events_page(rows, query).rows
}

#[must_use]
pub fn select_task_events_page(
    mut rows: Vec<TaskEventRow>,
    query: &TaskEventQuery,
) -> TaskSelection<TaskEventRow> {
    rows.retain(|row| {
        row.team == query.team
            && row.task_id == query.task_id
            && query
                .assignee
                .as_ref()
                .is_none_or(|assignee| &row.assignee == assignee)
    });
    rows.sort_by_key(|row| row.seq);
    let omitted = apply_page(&mut rows, query.page, PageEdge::End);
    TaskSelection { rows, omitted }
}

#[derive(Clone, Copy)]
enum PageEdge {
    Start,
    End,
}

fn apply_page<T>(rows: &mut Vec<T>, page: TaskPage, edge: PageEdge) -> usize {
    if let TaskPage::Bounded { limit } = page {
        let omitted = rows.len().saturating_sub(limit);
        match edge {
            PageEdge::Start => rows.truncate(limit),
            PageEdge::End => {
                rows.drain(..omitted);
            }
        }
        omitted
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use atm_storage::{TaskEventKind, TaskEventRow, TaskRow};
    use serde_json::json;

    use super::{
        TaskEventQuery, TaskListQuery, TaskPage, select_task_events, select_task_events_page,
        select_task_rows, select_task_rows_page,
    };

    fn task(task_id: &str, assignee: &str, state: &str, position: Option<u32>) -> TaskRow {
        serde_json::from_value(json!({
            "team": "test-team",
            "task_id": task_id,
            "assignee": assignee,
            "assigner": "lead",
            "state": state,
            "close_outcome": (state == "complete").then_some("completed"),
            "position": position,
            "assignment_message_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "description": task_id,
            "assigned_at": "2026-01-02T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z",
            "reminder_count": 0,
            "lead_notified_count": 0
        }))
        .expect("task row")
    }

    fn event(seq: u64, event: &str) -> TaskEventRow {
        serde_json::from_value(json!({
            "team": "test-team",
            "task_id": "T1",
            "assignee": "alice",
            "seq": seq,
            "at": "2026-01-02T00:00:00Z",
            "event": event,
            "actor": "daemon"
        }))
        .expect("event row")
    }

    #[test]
    fn selector_default_is_callers_own_queue() {
        let query = TaskListQuery {
            team: "test-team".parse().expect("team"),
            assignee: Some("alice".parse().expect("agent")),
            page: TaskPage::All,
        };
        let selected = select_task_rows(
            vec![
                task("T1", "alice", "assigned", Some(1)),
                task("T2", "bob", "assigned", Some(1)),
                task("T3", "alice", "complete", None),
            ],
            &query,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].task_id.as_str(), "T1");
    }

    #[test]
    fn selector_orders_by_position_not_assigned_at() {
        let query = TaskListQuery {
            team: "test-team".parse().expect("team"),
            assignee: Some("alice".parse().expect("agent")),
            page: TaskPage::All,
        };
        let selected = select_task_rows(
            vec![
                task("T1", "alice", "assigned", Some(2)),
                task("T2", "alice", "assigned", Some(3)),
                task("T3", "alice", "assigned", Some(1)),
            ],
            &query,
        );
        assert_eq!(
            selected
                .iter()
                .map(|row| row.task_id.as_str())
                .collect::<Vec<_>>(),
            ["T3", "T1", "T2"]
        );
    }

    #[test]
    fn selector_events_are_seq_ordered() {
        let query = TaskEventQuery {
            team: "test-team".parse().expect("team"),
            task_id: "T1".parse().expect("task"),
            assignee: None,
            page: TaskPage::All,
        };
        let selected = select_task_events(
            vec![event(3, "started"), event(1, "assigned"), event(2, "moved")],
            &query,
        );
        assert_eq!(
            selected.iter().map(|row| row.seq).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(selected.iter().any(|row| row.event == TaskEventKind::Moved));
        assert!(
            selected
                .iter()
                .any(|row| row.event == TaskEventKind::Started)
        );
    }

    #[test]
    fn bounded_list_reports_rows_omitted_from_the_end() {
        let query = TaskListQuery {
            team: "test-team".parse().expect("team"),
            assignee: Some("alice".parse().expect("agent")),
            page: TaskPage::bounded(2).expect("limit"),
        };
        let selected = select_task_rows_page(
            vec![
                task("T3", "alice", "assigned", Some(3)),
                task("T1", "alice", "assigned", Some(1)),
                task("T2", "alice", "assigned", Some(2)),
            ],
            &query,
        );
        assert_eq!(selected.omitted, 1);
        assert_eq!(
            selected
                .rows
                .iter()
                .map(|row| row.task_id.as_str())
                .collect::<Vec<_>>(),
            ["T1", "T2"]
        );
    }

    #[test]
    fn bounded_events_keep_the_most_recent_rows_in_sequence_order() {
        let query = TaskEventQuery {
            team: "test-team".parse().expect("team"),
            task_id: "T1".parse().expect("task"),
            assignee: None,
            page: TaskPage::bounded(2).expect("limit"),
        };
        let selected = select_task_events_page(
            vec![
                event(2, "moved"),
                event(4, "started"),
                event(1, "assigned"),
                event(3, "moved"),
            ],
            &query,
        );
        assert_eq!(selected.omitted, 2);
        assert_eq!(
            selected.rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            [3, 4]
        );
    }
}
