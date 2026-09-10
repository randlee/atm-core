use atm_core::api::RequestDeadline;
use atm_core::boundary::{AssignmentAttempt, LogicalTaskRow, ReadDeadline, TaskAssignmentAttempt};
use atm_core::error::AtmError;
use atm_core::schema::AtmMessageId;
use atm_core::types::{IsoTimestamp, TaskId};

use crate::herdr_attention_scheduler;
use crate::herdr_queue_wake::{HerdrQueueWakePump, task_ledger_read_error};

pub(super) async fn load_current_task_reminder(
    pump: &HerdrQueueWakePump,
    member: &atm_core::boundary::MemberKey,
    task_id: &TaskId,
    attempt: AssignmentAttempt,
    assignment_message_id: AtmMessageId,
    now: IsoTimestamp,
    absolute_deadline: RequestDeadline,
) -> Result<Option<(LogicalTaskRow, TaskAssignmentAttempt)>, AtmError> {
    let reader = pump.service_runtime.async_task_ledger_reader()?;
    let deadline = ReadDeadline::new(absolute_deadline.remaining().ok_or_else(|| {
        AtmError::validation("attention dispatch deadline expired before task revalidation")
    })?)?;
    let Some(row) = reader
        .top_runnable_task(member.team().clone(), member.agent().clone(), deadline)
        .await
        .map_err(task_ledger_read_error)?
        .filter(|row| {
            row.task_id == *task_id
                && row.current_attempt == attempt
                && row.assignment_message_id == assignment_message_id
                && herdr_attention_scheduler::task_reminder_due(row, now)
        })
    else {
        return Ok(None);
    };
    let deadline = ReadDeadline::new(absolute_deadline.remaining().ok_or_else(|| {
        AtmError::validation("attention dispatch deadline expired before assignment revalidation")
    })?)?;
    let assignment = reader
        .list_task_assignment_attempts(member.team().clone(), task_id.clone(), deadline)
        .await
        .map_err(task_ledger_read_error)?
        .into_iter()
        .find(|assignment| {
            assignment.attempt == attempt
                && assignment.assignee == *member.agent()
                && assignment.assignment_message_id == assignment_message_id
        });
    Ok(assignment.map(|assignment| (row, assignment)))
}
