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

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::test_support as atm_storage;
    use atm_storage::{
        BuiltInNudgeTemplateKind, IsoTimestamp, MessageKey, PromptTrigger, ReminderOutcome,
        TaskCloseOutcome, TaskEventMarker, TaskStateTag,
    };
    use std::str::FromStr;

    const ALL_KINDS: [(TaskEventKind, TaskTelemetryKind); 14] = [
        (TaskEventKind::Assigned, TaskTelemetryKind::Assigned),
        (TaskEventKind::Acked, TaskTelemetryKind::Acked),
        (TaskEventKind::Started, TaskTelemetryKind::Started),
        (TaskEventKind::Completed, TaskTelemetryKind::Completed),
        (TaskEventKind::Refused, TaskTelemetryKind::Refused),
        (TaskEventKind::Cancelled, TaskTelemetryKind::Cancelled),
        (TaskEventKind::Reassigned, TaskTelemetryKind::Reassigned),
        (TaskEventKind::Reopened, TaskTelemetryKind::Reopened),
        (TaskEventKind::Rejected, TaskTelemetryKind::Rejected),
        (TaskEventKind::Reminded, TaskTelemetryKind::Reminded),
        (TaskEventKind::LeadNotified, TaskTelemetryKind::LeadNotified),
        (TaskEventKind::Moved, TaskTelemetryKind::Moved),
        (TaskEventKind::Migrated, TaskTelemetryKind::Migrated),
        (
            TaskEventKind::RemindersReset,
            TaskTelemetryKind::RemindersReset,
        ),
    ];

    fn row(event: TaskEventKind, seq: u64) -> TaskEventRow {
        TaskEventRow {
            team: "conversion-team".parse().expect("team"),
            task_id: "CONV-1".parse().expect("task id"),
            assignee: "worker".parse().expect("agent"),
            seq,
            at: IsoTimestamp::from_str("2030-01-01T00:00:05Z").expect("timestamp"),
            event,
            from_state: Some(TaskState::Active),
            to_state: Some(TaskState::Complete(TaskCloseOutcome::Refused)),
            actor: TaskActor::Member("sender".parse().expect("agent")),
            message_id: Some("01J00000000000000000000001".parse().expect("message id")),
            outcome: Some(ReminderOutcome::Emitted),
            marker: Some(TaskEventMarker::Resend),
            detail: Some("free-form detail that must never be projected".to_owned()),
        }
    }

    /// Every durable kind, including the historical-only `Acked` and
    /// `Migrated` fixtures, maps to its telemetry kind with the row's own
    /// seq, timestamp and typed fields; detail has no record field.
    #[test]
    fn every_event_kind_converts_with_the_durable_fields() {
        for (event, kind) in ALL_KINDS {
            let source = row(event, 7);
            let record = record_from_event(&source);
            assert_eq!(record.kind, kind);
            assert_eq!(record.kind.as_str(), event.as_str());
            assert_eq!(record.team, source.team);
            assert_eq!(record.task_id, source.task_id);
            assert_eq!(record.assignee, source.assignee);
            assert_eq!(record.actor, source.actor);
            assert_eq!(record.seq, Some(7));
            assert_eq!(record.at, source.at);
            assert_eq!(record.from_state, Some(TaskStateTag::Active));
            assert_eq!(record.to_state, Some(TaskStateTag::Complete));
            assert_eq!(record.close_outcome, Some(TaskCloseOutcome::Refused));
            assert_eq!(record.message_id, source.message_id);
            assert_eq!(record.reminder_outcome, Some(ReminderOutcome::Emitted));
            assert_eq!(record.marker, Some(TaskEventMarker::Resend));
            assert_eq!(record.handoff, None);
            let json = serde_json::to_string(&record).expect("record json");
            assert!(!json.contains("free-form detail"));
        }
    }

    #[test]
    fn open_target_state_has_no_close_outcome() {
        let mut source = row(TaskEventKind::Started, 2);
        source.from_state = None;
        source.to_state = Some(TaskState::Active);
        let record = record_from_event(&source);
        assert_eq!(record.from_state, None);
        assert_eq!(record.to_state, Some(TaskStateTag::Active));
        assert_eq!(record.close_outcome, None);
    }

    fn handoff() -> PromptHandoff {
        let message_id: atm_core::schema::AtmMessageId =
            "01J00000000000000000000002".parse().expect("message id");
        PromptHandoff {
            team: "conversion-team".parse().expect("team"),
            agent: "worker".parse().expect("agent"),
            message_key: MessageKey::from(message_id),
            kind: BuiltInNudgeTemplateKind::TaskReminder,
            task_id: "CONV-1".parse().expect("task id"),
            attempt: 3,
            trigger: PromptTrigger::TaskPass,
            at: IsoTimestamp::from_str("2030-01-01T00:00:09Z").expect("timestamp"),
        }
    }

    #[test]
    fn handoff_converts_without_a_sequence_or_fabricated_timestamp() {
        let source = handoff();
        let record = record_from_handoff(&source);
        assert_eq!(record.kind, TaskTelemetryKind::PromptHandoff);
        assert_eq!(record.seq, None);
        assert_eq!(record.at, source.at);
        assert_eq!(record.actor, TaskActor::Daemon);
        assert_eq!(record.assignee, source.agent);
        assert_eq!(
            record.message_id,
            Some(source.message_key.as_atm_message_id().expect("message id"))
        );
        assert_eq!(
            record.handoff,
            Some(TaskHandoffFacts {
                attempt: 3,
                trigger: PromptTrigger::TaskPass,
                template_kind: BuiltInNudgeTemplateKind::TaskReminder,
            })
        );
    }

    async fn recorded(
        project: impl FnOnce(&TaskTelemetryRuntime),
        expected: u64,
    ) -> Vec<TaskTelemetryRecord> {
        let sink = atm_runtime_test_support::RecordingTaskTelemetrySink::new();
        let setup = atm_runtime_test_support::RecordingTaskTelemetrySink::setup(&sink);
        let runtime = TaskTelemetryRuntime::start(setup.config, setup.sink);
        project(&runtime);
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await;
        assert_eq!(runtime.diagnostics().snapshot().emitted, expected);
        sink.records()
    }

    #[tokio::test]
    async fn existing_handoff_emits_nothing_and_inserted_emits_once() {
        let records = recorded(
            |runtime| {
                project_handoff(runtime, &PromptHandoffWrite::Existing(handoff()));
                project_handoff(runtime, &PromptHandoffWrite::Inserted(handoff()));
            },
            1,
        )
        .await;
        assert_eq!(records, vec![record_from_handoff(&handoff())]);
    }

    #[tokio::test]
    async fn one_outcome_projects_in_durable_seq_order() {
        let rows = vec![
            row(TaskEventKind::Moved, 9),
            row(TaskEventKind::Assigned, 4),
            row(TaskEventKind::Rejected, 6),
        ];
        let records = recorded(|runtime| project_task_events(runtime, &rows), 3).await;
        let seqs: Vec<Option<u64>> = records.iter().map(|record| record.seq).collect();
        assert_eq!(seqs, vec![Some(4), Some(6), Some(9)]);
    }
}
