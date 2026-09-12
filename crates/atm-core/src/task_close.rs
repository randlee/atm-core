//! Read-lane preflight for the atomic deliver-then-close write.

use atm_storage::{AsyncTaskLedgerReader, ReadDeadline, TaskRow};

use crate::error::AtmError;
use crate::types::{AgentName, TaskId, TeamName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosePreflight {
    /// A row exists; the guarded writer transaction makes the final decision.
    Proceed { row: TaskRow },
    /// No row exists, so no report may be sent.
    Unknown,
}

pub async fn preflight_close(
    reader: &dyn AsyncTaskLedgerReader,
    team: TeamName,
    task_id: &TaskId,
    deadline: ReadDeadline,
) -> Result<ClosePreflight, AtmError> {
    let row = reader
        .list_tasks(team, None, deadline)
        .await
        .map_err(AtmError::from)?
        .into_iter()
        .find(|row| &row.task_id == task_id);
    Ok(match row {
        Some(row) => ClosePreflight::Proceed { row },
        None => ClosePreflight::Unknown,
    })
}

#[must_use]
pub fn report_recipient(row: &TaskRow, caller: &AgentName) -> AgentName {
    if caller == &row.assignee {
        row.assigner.clone()
    } else {
        row.assignee.clone()
    }
}
