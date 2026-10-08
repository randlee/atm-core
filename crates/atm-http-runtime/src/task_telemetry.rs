//! Projects newly committed task-ledger rows onto the bounded task telemetry
//! runtime.
//!
//! Projection runs only after the storage write returned, outside any writer
//! lock, and uses the runtime's non-blocking `try_emit`; it never changes the
//! caller's result. Records carry typed task metadata only: event detail,
//! message bodies and template variables are never projected, and timestamps
//! come from the durable row.

use atm_core::boundary::{
    PromptHandoff, PromptHandoffWrite, TaskActor, TaskEventKind, TaskEventRow, TaskState,
};
use atm_core::{TaskHandoffFacts, TaskTelemetryKind, TaskTelemetryRecord};
use atm_runtime::TaskTelemetryRuntime;

/// Emits one record per committed row, ordered by durable `seq` within this
/// one outcome. No ordering across concurrent requests is promised.
pub(crate) fn project_task_events(runtime: &TaskTelemetryRuntime, rows: &[TaskEventRow]) {
    let mut ordered: Vec<&TaskEventRow> = rows.iter().collect();
    ordered.sort_by_key(|row| row.seq);
    for row in ordered {
        runtime.try_emit(record_from_event(row));
    }
}

/// Emits a newly inserted handoff; an existing handoff is a duplicate and
/// emits nothing.
pub(crate) fn project_handoff(runtime: &TaskTelemetryRuntime, write: &PromptHandoffWrite) {
    if let PromptHandoffWrite::Inserted(handoff) = write {
        runtime.try_emit(record_from_handoff(handoff));
    }
}

pub(crate) fn record_from_event(row: &TaskEventRow) -> TaskTelemetryRecord {
    TaskTelemetryRecord {
        kind: event_kind(row.event),
        team: row.team.clone(),
        task_id: row.task_id.clone(),
        assignee: row.assignee.clone(),
        actor: row.actor.clone(),
        seq: Some(row.seq),
        at: row.at,
        from_state: row.from_state.map(TaskState::tag),
        to_state: row.to_state.map(TaskState::tag),
        close_outcome: row.to_state.and_then(TaskState::close_outcome),
        message_id: row.message_id,
        reminder_outcome: row.outcome,
        marker: row.marker,
        handoff: None,
    }
}

pub(crate) fn record_from_handoff(handoff: &PromptHandoff) -> TaskTelemetryRecord {
    TaskTelemetryRecord {
        kind: TaskTelemetryKind::PromptHandoff,
        team: handoff.team.clone(),
        task_id: handoff.task_id.clone(),
        assignee: handoff.agent.clone(),
        actor: TaskActor::Daemon,
        seq: None,
        at: handoff.at,
        from_state: None,
        to_state: None,
        close_outcome: None,
        message_id: handoff.message_key.as_atm_message_id().ok(),
        reminder_outcome: None,
        marker: None,
        handoff: Some(TaskHandoffFacts {
            attempt: handoff.attempt,
            trigger: handoff.trigger,
            template_kind: handoff.kind,
        }),
    }
}

const fn event_kind(kind: TaskEventKind) -> TaskTelemetryKind {
    match kind {
        TaskEventKind::Assigned => TaskTelemetryKind::Assigned,
        TaskEventKind::Acked => TaskTelemetryKind::Acked,
        TaskEventKind::Started => TaskTelemetryKind::Started,
        TaskEventKind::Completed => TaskTelemetryKind::Completed,
        TaskEventKind::Refused => TaskTelemetryKind::Refused,
        TaskEventKind::Cancelled => TaskTelemetryKind::Cancelled,
        TaskEventKind::Reassigned => TaskTelemetryKind::Reassigned,
        TaskEventKind::Reopened => TaskTelemetryKind::Reopened,
        TaskEventKind::Rejected => TaskTelemetryKind::Rejected,
        TaskEventKind::Reminded => TaskTelemetryKind::Reminded,
        TaskEventKind::LeadNotified => TaskTelemetryKind::LeadNotified,
        TaskEventKind::Moved => TaskTelemetryKind::Moved,
        TaskEventKind::Migrated => TaskTelemetryKind::Migrated,
        TaskEventKind::RemindersReset => TaskTelemetryKind::RemindersReset,
    }
}
