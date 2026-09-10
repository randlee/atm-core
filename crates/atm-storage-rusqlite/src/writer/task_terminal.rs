//! Canonical completion, reassignment, and supersession task transitions.

use atm_storage::{AtmError, TaskMutationRequest, TaskOperation, TaskOutcome};

use super::super::stmt_cache::WriterStatementCache;
use super::{
    TransitionResult, TransitionSpec, close_with_handoff, reassign_and_clear_markers, supersede_v2,
};
use crate::shared_db::{SharedDbTarget, SqliteConnection};

pub(super) fn apply(
    request: &TaskMutationRequest,
    connection: &SqliteConnection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
    now: &str,
) -> Result<TransitionResult, AtmError> {
    match &request.operation {
        TaskOperation::Close { outcome, handoff } => close_with_handoff(
            connection,
            cache,
            target,
            request,
            handoff,
            TransitionSpec::closed(outcome.clone(), "closed"),
            now,
        ),
        TaskOperation::LegacyCloseSucceeded { completion_notice } => close_with_handoff(
            connection,
            cache,
            target,
            request,
            completion_notice,
            TransitionSpec::closed(TaskOutcome::Succeeded, "legacy_close_succeeded"),
            now,
        ),
        TaskOperation::Reassign(assignment) | TaskOperation::Reopen(assignment) => {
            let reopen = matches!(request.operation, TaskOperation::Reopen(_));
            reassign_and_clear_markers(connection, cache, target, request, assignment, now, reopen)
        }
        TaskOperation::Supersede {
            handoff,
            successor_task_id,
            successor,
        } => supersede_v2(
            connection,
            cache,
            target,
            request,
            handoff,
            successor_task_id,
            successor,
            now,
        ),
        TaskOperation::Assign(_)
        | TaskOperation::Start
        | TaskOperation::Block { .. }
        | TaskOperation::Unblock { .. }
        | TaskOperation::RecordReminder { .. } => {
            unreachable!("caller routes only completion and reassignment operations")
        }
    }
}
