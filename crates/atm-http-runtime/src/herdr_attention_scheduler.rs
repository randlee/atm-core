//! Identifier-only selection and durable reservation for one idle opportunity.
//!
//! This module stops before message reconstruction or prompt emission. The
//! queue and task ledger retain ownership of their lifecycles; the scheduler
//! owns only fair selection metadata.

use atm_core::LocalServiceRuntime;
use atm_core::boundary::{
    AttentionCandidates, AttentionFinalizeOutcome, AttentionFinalizeRequest, AttentionReservation,
    AttentionReservationRequest, AttentionReservationStatus, EphemeralMessageCandidate,
    IdleOpportunity, LogicalTaskRow, MemberKey, PendingNudgeStore, PersistentTaskCandidate,
    ReadDeadline, TaskLifecycleState, select_attention_item,
};
use atm_core::error::AtmError;
use atm_core::types::IsoTimestamp;

use crate::herdr_queue_wake::{HERDR_REQUEST_DEADLINE, run_blocking, task_ledger_read_error};

pub(crate) const TASK_REMINDER_INTERVAL_MS: u64 = 60_000;

pub(super) async fn finalize_attention(
    runtime: &LocalServiceRuntime,
    reservation: &AttentionReservation,
    status: AttentionReservationStatus,
) -> Result<AttentionReservation, AtmError> {
    let schedule_store = runtime.async_attention_schedule_store()?;
    let request = AttentionFinalizeRequest {
        member: reservation.opportunity.member.clone(),
        opportunity_id: reservation.opportunity.id,
        outcome: match status {
            AttentionReservationStatus::Delivered => AttentionFinalizeOutcome::Delivered,
            AttentionReservationStatus::Stale => AttentionFinalizeOutcome::Stale,
            AttentionReservationStatus::PermanentlyFailed => {
                AttentionFinalizeOutcome::RetryableFailure
            }
            AttentionReservationStatus::Reserved => {
                return Err(AtmError::validation(
                    "cannot finalize an attention reservation as reserved",
                ));
            }
        },
    };
    let deadline = ReadDeadline::new(HERDR_REQUEST_DEADLINE)?;
    schedule_store.finalize(request, deadline).await
}

/// Reserves at most one fair attention item for a committed idle opportunity.
///
/// The runtime claims the selected lane afterwards and finalizes this durable
/// reservation as delivered or stale. The result never carries message or task
/// text.
pub(super) async fn reserve_next_attention(
    runtime: &LocalServiceRuntime,
    opportunity: IdleOpportunity,
    now: IsoTimestamp,
) -> Result<Option<AttentionReservation>, AtmError> {
    let schedule_store = runtime.async_attention_schedule_store()?;
    let pending_store = runtime.pending_nudge_store()?;
    let member = opportunity.member.clone();
    let deadline = ReadDeadline::new(HERDR_REQUEST_DEADLINE)?;
    let cursor = schedule_store
        .load_cursor(member.clone(), deadline)
        .await
        .map_err(AtmError::from)?;
    let ephemeral = next_ephemeral(pending_store, member.clone()).await?;
    let persistent_task = next_persistent_task(runtime, member.clone(), now).await?;
    let selection = select_attention_item(
        member,
        cursor.next_lane,
        AttentionCandidates {
            ephemeral,
            persistent_task,
        },
    );
    let Some(item) = selection.item else {
        return Ok(None);
    };
    let deadline = ReadDeadline::new(HERDR_REQUEST_DEADLINE)?;
    schedule_store
        .reserve(
            AttentionReservationRequest {
                opportunity,
                expected_cursor_revision: cursor.revision,
                item,
            },
            deadline,
        )
        .await
        .map(Some)
}

async fn next_ephemeral(
    pending_store: std::sync::Arc<dyn PendingNudgeStore + Send + Sync>,
    member: MemberKey,
) -> Result<Option<EphemeralMessageCandidate>, AtmError> {
    run_blocking(move || {
        pending_store.peek_next_pending(&member).map(|claim| {
            claim.map(|claim| EphemeralMessageCandidate {
                message_id: claim.msg,
            })
        })
    })
    .await
}

async fn next_persistent_task(
    runtime: &LocalServiceRuntime,
    member: MemberKey,
    now: IsoTimestamp,
) -> Result<Option<PersistentTaskCandidate>, AtmError> {
    let reader = runtime.async_task_ledger_reader()?;
    let deadline = ReadDeadline::new(HERDR_REQUEST_DEADLINE)?;
    reader
        .top_runnable_task(member.team().clone(), member.agent().clone(), deadline)
        .await
        .map_err(task_ledger_read_error)
        .map(|row| row.and_then(|row| persistent_candidate(row, now)))
}

fn persistent_candidate(row: LogicalTaskRow, now: IsoTimestamp) -> Option<PersistentTaskCandidate> {
    (matches!(
        row.state,
        TaskLifecycleState::Assigned | TaskLifecycleState::Active
    ) && task_reminder_due(&row, now))
    .then_some(PersistentTaskCandidate {
        task_id: row.task_id,
        attempt: row.current_attempt,
        assignment_message_id: row.assignment_message_id,
        priority: row.priority,
    })
}

pub(super) fn task_reminder_due(row: &LogicalTaskRow, now: IsoTimestamp) -> bool {
    row.last_reminded_at.is_none_or(|last_reminded_at| {
        now.into_inner()
            .signed_duration_since(last_reminded_at.into_inner())
            .num_milliseconds()
            >= i64::try_from(TASK_REMINDER_INTERVAL_MS).unwrap_or(i64::MAX)
    })
}

#[cfg(test)]
mod tests {
    use atm_core::boundary::{
        AssignmentAttempt, LogicalTaskRow, ReadLaneError, TaskLifecycleState, TaskPriority,
    };
    use atm_core::error::AtmErrorCode;
    use atm_core::schema::AtmMessageId;

    use super::{persistent_candidate, task_ledger_read_error, task_reminder_due};

    #[test]
    fn persistent_task_reader_errors_preserve_the_reader_lane_code() {
        for (error, expected) in [
            (
                ReadLaneError::Saturated {
                    reason: "test saturation",
                },
                AtmErrorCode::DaemonConnectionSaturated,
            ),
            (
                ReadLaneError::DeadlineExpired {
                    stage: "test deadline",
                },
                AtmErrorCode::MailboxLockTimeout,
            ),
            (
                ReadLaneError::Storage {
                    code: AtmErrorCode::MailboxReadFailed,
                    message: "test storage failure".to_owned(),
                    cause: Some("test cause".to_owned()),
                },
                AtmErrorCode::MailboxReadFailed,
            ),
        ] {
            assert_eq!(task_ledger_read_error(error).code(), expected);
        }
    }

    #[test]
    fn persistent_candidate_carries_only_revalidation_metadata() {
        let row = LogicalTaskRow {
            team: "team".parse().expect("team"),
            task_id: "task".parse().expect("task"),
            current_assignee: "agent".parse().expect("agent"),
            state: TaskLifecycleState::Assigned,
            priority: TaskPriority::High,
            original_assigned_at: "2026-09-10T00:00:00Z".parse().expect("time"),
            current_attempt: AssignmentAttempt::FIRST,
            assignment_message_id: AtmMessageId::new(),
            last_reminded_at: None,
            reminder_ordinal: 0,
            revision: 1,
            updated_at: "2026-09-10T00:00:00Z".parse().expect("time"),
        };

        let candidate = persistent_candidate(row, "2026-09-10T00:01:00Z".parse().expect("time"))
            .expect("runnable candidate");
        assert_eq!(candidate.priority, TaskPriority::High);
        assert_eq!(candidate.attempt, AssignmentAttempt::FIRST);
    }

    #[test]
    fn task_cadence_is_derived_from_the_current_attempt_audit_time() {
        let row = LogicalTaskRow {
            team: "team".parse().expect("team"),
            task_id: "task".parse().expect("task"),
            current_assignee: "agent".parse().expect("agent"),
            state: TaskLifecycleState::Active,
            priority: TaskPriority::Normal,
            original_assigned_at: "2026-09-10T00:00:00Z".parse().expect("time"),
            current_attempt: AssignmentAttempt::FIRST,
            assignment_message_id: AtmMessageId::new(),
            last_reminded_at: Some("2026-09-10T00:00:00Z".parse().expect("time")),
            reminder_ordinal: 1,
            revision: 2,
            updated_at: "2026-09-10T00:00:00Z".parse().expect("time"),
        };
        assert!(!task_reminder_due(
            &row,
            "2026-09-10T00:00:59Z".parse().expect("time")
        ));
        assert!(task_reminder_due(
            &row,
            "2026-09-10T00:01:00Z".parse().expect("time")
        ));
    }
}
