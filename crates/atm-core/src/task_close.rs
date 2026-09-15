//! Read-lane preflight for the atomic deliver-then-close write.

use atm_storage::TaskRow;

use crate::types::{AgentName, TaskId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosePreflight {
    /// A row exists; the guarded writer transaction makes the final decision.
    Proceed { row: TaskRow },
    /// No row exists, so no report may be sent.
    Unknown,
}

pub fn preflight_close(rows: Vec<TaskRow>, task_id: &TaskId) -> ClosePreflight {
    match rows.into_iter().find(|row| &row.task_id == task_id) {
        Some(row) => ClosePreflight::Proceed { row },
        None => ClosePreflight::Unknown,
    }
}

#[must_use]
pub fn report_recipient(row: &TaskRow, caller: &AgentName) -> AgentName {
    if caller == &row.assignee {
        row.assigner.clone()
    } else {
        row.assignee.clone()
    }
}
