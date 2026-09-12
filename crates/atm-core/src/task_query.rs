//! Transport-neutral task list and event query contracts.

use serde::{Deserialize, Serialize};

use crate::error::AtmError;
use crate::types::{AgentName, TaskId, TeamName};

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
