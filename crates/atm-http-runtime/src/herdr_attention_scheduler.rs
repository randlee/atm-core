//! Identifier-only selection and durable reservation for one idle opportunity.
//!
//! This module deliberately stops before message reconstruction or prompt
//! emission. The queue and task ledger retain ownership of those lifecycles;
//! the scheduler owns only fair selection metadata.

use std::sync::Arc;

use atm_core::LocalServiceRuntime;
use atm_core::boundary::{
    AttentionCandidates, AttentionCursor, AttentionFinalizeOutcome, AttentionFinalizeRequest,
    AttentionItem, AttentionReservation, AttentionReservationRequest, AttentionReservationStatus,
    AttentionScheduleStore, EphemeralMessageCandidate, IdleOpportunity, LogicalTaskRow, MemberKey,
    NudgeClaim, PendingNudgeStore, PersistentTaskCandidate, ReadDeadline, TaskAssignmentAttempt,
    TaskLifecycleState, TaskMutationRequest, TaskOperation, TaskOperationId, select_attention_item,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::types::IsoTimestamp;

use super::herdr_queue_wake::{HERDR_REQUEST_DEADLINE, TASK_REMINDER_INTERVAL_MS, run_blocking};

pub(super) enum AttentionDispatchDisposition {
    Finalized(AttentionReservationStatus),
}

enum Claim {
    Queue {
        member: MemberKey,
        claim: NudgeClaim,
        guard: super::herdr_queue_wake::ReleasePendingOnDrop,
    },
    Task {
        member: MemberKey,
        row: LogicalTaskRow,
        assignment: TaskAssignmentAttempt,
    },
}

pub(super) async fn dispatch_reserved_attention(
    pump: &super::herdr_queue_wake::HerdrQueueWakePump,
    reservation: AttentionReservation,
    _now: IsoTimestamp,
    stats: &mut super::herdr_queue_wake::HerdrQueueWakeStats,
) -> Result<AttentionDispatchDisposition, AtmError> {
    let member = match &reservation.item {
        AttentionItem::EphemeralMessage { member, .. }
        | AttentionItem::PersistentTaskReminder { member, .. } => member,
    };
    let roster_is_current = pump
        .service_runtime
        .roster_ephemeral_state(member.team(), member.agent())
        .is_some_and(|state| {
            state.runtime.state == atm_core::protocol::RuntimeMemberState::Idle
                && state.runtime.revision == reservation.opportunity.roster_state_revision
        });
    if !roster_is_current {
        return Ok(AttentionDispatchDisposition::Finalized(
            AttentionReservationStatus::Stale,
        ));
    }
    match reservation.item {
        AttentionItem::EphemeralMessage { member, message_id } => {
            let pending_store = pump.service_runtime.pending_nudge_store()?;
            let claim_member = member.clone();
            let claim_store = Arc::clone(&pending_store);
            let Some(claim) =
                run_blocking(move || claim_store.claim_pending(&claim_member, &message_id)).await?
            else {
                return Ok(AttentionDispatchDisposition::Finalized(
                    AttentionReservationStatus::Stale,
                ));
            };
            let guard =
                pump.queue_claim_guard(Arc::clone(&pending_store), member.clone(), claim.clone());
            let claim = Claim::Queue {
                member,
                claim,
                guard,
            };
            let Claim::Queue {
                member,
                claim,
                mut guard,
            } = claim
            else {
                unreachable!("queue claim is constructed above")
            };
            let dispatch = match pump.rebuild_dispatch(&member, claim.msg).await {
                Ok(Some(dispatch)) => dispatch,
                Ok(None) | Err(_) => {
                    guard.release_without_input().await;
                    stats.released += 1;
                    return Ok(AttentionDispatchDisposition::Finalized(
                        AttentionReservationStatus::Stale,
                    ));
                }
            };
            let Some(emitter) = pump.selector.select_emitter(&dispatch) else {
                guard.release_without_input().await;
                stats.released += 1;
                return Ok(AttentionDispatchDisposition::Finalized(
                    AttentionReservationStatus::Stale,
                ));
            };
            #[cfg(test)]
            pump.notify_prompt_started_test_gate();
            match emitter
                .emit_received_message(
                    dispatch,
                    atm_core::api::RequestDeadline::after(HERDR_REQUEST_DEADLINE),
                )
                .await
            {
                Ok(_) => {
                    pump.complete_successful_claim(&member, &claim, &mut guard, stats)
                        .await;
                    Ok(AttentionDispatchDisposition::Finalized(
                        AttentionReservationStatus::Delivered,
                    ))
                }
                Err(error) => {
                    if error.code() == AtmErrorCode::HerdrUnavailable {
                        stats.breaker_open += 1;
                    }
                    match error.code() {
                        AtmErrorCode::HerdrPromptFailed => guard.requeue().await,
                        _ => guard.release_without_input().await,
                    }
                    stats.released += 1;
                    // Queued messages retain their own claim/requeue policy.
                    // Marking this scheduler reservation stale permits the
                    // queue owner to select the exact message again on a
                    // future idle observation instead of converting its
                    // delivery failure into a task-style retry chain.
                    Ok(AttentionDispatchDisposition::Finalized(
                        AttentionReservationStatus::Stale,
                    ))
                }
            }
        }
        AttentionItem::PersistentTaskReminder {
            member,
            task_id,
            attempt,
            assignment_message_id,
        } => {
            let reader = pump.service_runtime.async_task_ledger_reader()?;
            let deadline = ReadDeadline::new(HERDR_REQUEST_DEADLINE)?;
            let Some(row) = reader
                .top_runnable_task(member.team().clone(), member.agent().clone(), deadline)
                .await
                .map_err(|error| AtmError::daemon_unavailable(error.to_string()))?
                .filter(|row| {
                    row.task_id == task_id
                        && row.current_attempt == attempt
                        && row.assignment_message_id == assignment_message_id
                        && task_reminder_due(row, _now)
                })
            else {
                return Ok(AttentionDispatchDisposition::Finalized(
                    AttentionReservationStatus::Stale,
                ));
            };
            let deadline = ReadDeadline::new(HERDR_REQUEST_DEADLINE)?;
            let Some(assignment) = reader
                .list_task_assignment_attempts(member.team().clone(), task_id.clone(), deadline)
                .await
                .map_err(|error| AtmError::daemon_unavailable(error.to_string()))?
                .into_iter()
                .find(|assignment| {
                    assignment.attempt == attempt
                        && assignment.assignee == *member.agent()
                        && assignment.assignment_message_id == assignment_message_id
                })
            else {
                return Ok(AttentionDispatchDisposition::Finalized(
                    AttentionReservationStatus::Stale,
                ));
            };
            let claim = Claim::Task {
                member: member.clone(),
                row,
                assignment,
            };
            let Claim::Task {
                member,
                row,
                assignment,
            } = claim
            else {
                unreachable!("task claim is constructed above")
            };
            let runtime = pump.service_runtime.clone();
            let dispatch_member = member.clone();
            let dispatch_task_id = task_id.clone();
            let dispatch = run_blocking(move || {
                atm_core::nudge_dispatch::build_logical_task_reminder_dispatch(
                    &runtime,
                    &dispatch_member,
                    &dispatch_task_id,
                    &assignment,
                )
            })
            .await?;
            let Some(dispatch) = dispatch else {
                return Ok(AttentionDispatchDisposition::Finalized(
                    AttentionReservationStatus::PermanentlyFailed,
                ));
            };
            let Some(emitter) = pump.selector.select_emitter(&dispatch) else {
                return Ok(AttentionDispatchDisposition::Finalized(
                    AttentionReservationStatus::Stale,
                ));
            };
            if let Err(error) = emitter
                .emit_received_message(
                    dispatch,
                    atm_core::api::RequestDeadline::after(HERDR_REQUEST_DEADLINE),
                )
                .await
            {
                if error.code() == AtmErrorCode::HerdrUnavailable {
                    stats.breaker_open += 1;
                }
                stats.task_reminders_failed += 1;
                return Ok(AttentionDispatchDisposition::Finalized(
                    AttentionReservationStatus::PermanentlyFailed,
                ));
            }
            let mutation_store = pump.service_runtime.async_task_mutation_store()?;
            let daemon_actor = "atm-daemon"
                .parse()
                .map_err(|_| AtmError::validation("invalid daemon task actor"))?;
            if let Err(error) = mutation_store
                .apply(TaskMutationRequest {
                    operation_id: TaskOperationId::new(),
                    actor: MemberKey::new(member.team().clone(), daemon_actor),
                    task_id,
                    expected_revision: Some(row.revision),
                    operation: TaskOperation::RecordReminder { attempt, at: _now },
                })
                .await
            {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "task_reminder_record",
                    outcome = "failed",
                    error = %error,
                    member = %member,
                    "Herdr task reminder audit write failed after accepted emission"
                );
            }
            stats.prompted += 1;
            stats.task_reminders += 1;
            Ok(AttentionDispatchDisposition::Finalized(
                AttentionReservationStatus::Delivered,
            ))
        }
    }
}

pub(super) async fn finalize_attention(
    runtime: &LocalServiceRuntime,
    reservation: &AttentionReservation,
    status: AttentionReservationStatus,
) -> Result<AttentionReservation, AtmError> {
    let schedule_store = runtime.attention_schedule_store()?;
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
    run_blocking(move || schedule_store.finalize(request)).await
}

/// Reserves at most one fair attention item for a committed idle opportunity.
///
/// The caller claims the selected lane afterwards and finalizes this durable
/// reservation as delivered or stale. The result never carries message or
/// task text.
pub(super) async fn reserve_next_attention(
    runtime: &LocalServiceRuntime,
    opportunity: IdleOpportunity,
    now: IsoTimestamp,
) -> Result<Option<AttentionReservation>, AtmError> {
    let schedule_store = runtime.attention_schedule_store()?;
    let pending_store = runtime.pending_nudge_store()?;
    let member = opportunity.member.clone();
    let cursor = load_cursor(Arc::clone(&schedule_store), member.clone()).await?;
    let ephemeral = next_ephemeral(Arc::clone(&pending_store), member.clone()).await?;
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
    let reservation = run_blocking(move || {
        schedule_store.reserve(AttentionReservationRequest {
            opportunity,
            expected_cursor_revision: cursor.revision,
            item,
        })
    })
    .await?;
    Ok(Some(reservation))
}

async fn load_cursor(
    schedule_store: Arc<dyn AttentionScheduleStore + Send + Sync>,
    member: MemberKey,
) -> Result<AttentionCursor, AtmError> {
    run_blocking(move || schedule_store.load_cursor(&member)).await
}

async fn next_ephemeral(
    pending_store: Arc<dyn PendingNudgeStore + Send + Sync>,
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
        .map_err(|error| AtmError::daemon_unavailable(error.to_string()))
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

fn task_reminder_due(row: &LogicalTaskRow, now: IsoTimestamp) -> bool {
    row.last_reminded_at.is_none_or(|last_reminded_at| {
        now.into_inner()
            .signed_duration_since(last_reminded_at.into_inner())
            .num_milliseconds()
            >= i64::try_from(TASK_REMINDER_INTERVAL_MS).unwrap_or(i64::MAX)
    })
}

#[cfg(test)]
mod tests {
    use atm_core::boundary::{AssignmentAttempt, LogicalTaskRow, TaskLifecycleState, TaskPriority};
    use atm_core::schema::AtmMessageId;

    use super::{persistent_candidate, task_reminder_due};

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
