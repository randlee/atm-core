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
pub fn select_task_rows(mut rows: Vec<TaskRow>, query: &TaskListQuery) -> Vec<TaskRow> {
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
    apply_page(&mut rows, query.page);
    rows
}

#[must_use]
pub fn select_task_events(
    mut rows: Vec<TaskEventRow>,
    query: &TaskEventQuery,
) -> Vec<TaskEventRow> {
    rows.retain(|row| {
        row.team == query.team
            && row.task_id == query.task_id
            && query
                .assignee
                .as_ref()
                .is_none_or(|assignee| &row.assignee == assignee)
    });
    rows.sort_by_key(|row| row.seq);
    apply_page(&mut rows, query.page);
    rows
}

fn apply_page<T>(rows: &mut Vec<T>, page: TaskPage) {
    if let TaskPage::Bounded { limit } = page {
        rows.truncate(limit);
    }
}
