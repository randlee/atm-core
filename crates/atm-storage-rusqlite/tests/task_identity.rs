use std::collections::BTreeMap;

use atm_storage::contract::{Message, MessageKey};
use atm_storage::schema::{AtmMessageId, MessageEnvelope};
use atm_storage::{
    AgentName, AtmErrorCode, IsoTimestamp, MemberKey, MoveTarget, QueuePosition, ReminderOutcome,
    TaskCloseOutcome, TaskEventKind, TaskId, TaskOp, TaskState, TeamName,
};
use atm_storage_rusqlite::SqliteStorageBackend;
use chrono::Utc;
use rusqlite::{Connection, params};
use serde_json::Map;
use tempfile::TempDir;

struct Harness {
    _dir: TempDir,
    path: std::path::PathBuf,
    backend: SqliteStorageBackend,
    team: TeamName,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.sqlite");
        let backend = SqliteStorageBackend::new(&path).expect("backend");
        Self {
            _dir: dir,
            path,
            backend,
            team: "ba2".parse().expect("team"),
        }
    }

    fn save(&self, message: &Message) -> Result<(), atm_storage::AtmError> {
        self.backend.message_store().save_message(message)
    }

    fn assign(
        &self,
        task: &str,
        assignee: &str,
        assigner: &str,
        placement: Option<MoveTarget>,
    ) -> Message {
        let task_id: TaskId = task.parse().expect("task");
        let mut message = self.message(assignee, assigner, &format!("assign {task}"));
        message.envelope.task_id = Some(task_id);
        message.envelope.placement = placement;
        message.envelope.requires_ack = true;
        message.envelope.pending_ack_at = Some(message.envelope.timestamp);
        self.save(&message).expect("assign");
        message
    }

    fn start(&self, task: &str, assignee: &str) -> Result<(), atm_storage::AtmError> {
        let task_id: TaskId = task.parse().expect("task");
        let member = MemberKey::new(self.team.clone(), assignee.parse().expect("assignee"));
        self.backend.task_store().record_reminder(
            &member,
            &task_id,
            IsoTimestamp::now(),
            ReminderOutcome::Emitted,
        )?;
        let mut message = self.message(assignee, "atm-daemon", "start");
        message.envelope.task_id = Some(task_id);
        message.envelope.task_op = Some(TaskOp::Start);
        self.save(&message)
    }

    fn close(
        &self,
        task: &str,
        recipient: &str,
        actor: &str,
        outcome: TaskCloseOutcome,
    ) -> Result<Message, atm_storage::AtmError> {
        let mut message = self.message(recipient, actor, "close report");
        message.envelope.task_id = Some(task.parse().expect("task"));
        message.envelope.task_op = Some(TaskOp::Close {
            outcome,
            reason: Some("reason".to_owned()),
        });
        self.save(&message)?;
        Ok(message)
    }

    fn message(&self, recipient: &str, from: &str, text: &str) -> Message {
        let message_id = AtmMessageId::new();
        Message {
            team: self.team.clone(),
            agent: recipient.parse::<AgentName>().expect("recipient"),
            message_key: MessageKey::from(message_id),
            envelope: MessageEnvelope {
                from: from.parse().expect("sender"),
                source_chat_id: None,
                text: text.to_owned(),
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
                read: false,
                source_team: Some(self.team.clone()),
                destination_chat_id: None,
                summary: None,
                message_id: Some(message_id),
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                placement: None,
                task_op: None,
                task_complete: None,
                extra: Map::new(),
            },
        }
    }

    fn row(&self, task: &str) -> atm_storage::TaskRow {
        let task_id: TaskId = task.parse().expect("task");
        self.backend
            .task_store()
            .load_task(&self.team, &task_id)
            .expect("load")
            .expect("row")
    }

    fn events(&self, task: &str) -> Vec<atm_storage::TaskEventRow> {
        self.backend
            .task_store()
            .list_task_events(&self.team, &task.parse().expect("task"), None)
            .expect("events")
    }

    fn positions(&self, assignee: &str) -> BTreeMap<String, u32> {
        self.backend
            .task_store()
            .open_tasks(&MemberKey::new(
                self.team.clone(),
                assignee.parse().expect("assignee"),
            ))
            .expect("open tasks")
            .into_iter()
            .map(|row| {
                (
                    row.task_id.to_string(),
                    row.position.expect("position").get(),
                )
            })
            .collect()
    }
}

#[test]
fn writer_assign_existing_open_id_reassigns_in_place() {
    let h = Harness::new();
    let old = h.assign("T1", "alice", "lead", None);
    h.assign("T2", "alice", "lead", None);
    h.assign("T3", "bob", "lead", None);
    h.assign("T1", "bob", "new-lead", Some(MoveTarget::Head));
    let row = h.row("T1");
    assert_eq!(row.assignee.as_str(), "bob");
    assert_eq!(h.positions("alice")["T2"], 1);
    assert_eq!(h.positions("bob")["T1"], 1);
    assert_eq!(
        h.events("T1").last().unwrap().event,
        TaskEventKind::Reassigned
    );
    assert!(
        h.backend
            .message_store()
            .load_message(&old.message_key)
            .unwrap()
            .unwrap()
            .envelope
            .acknowledged_at
            .is_some()
    );
}

#[test]
fn writer_assign_closed_id_reopens_same_row() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.close("T1", "lead", "alice", TaskCloseOutcome::Refused)
        .unwrap();
    h.assign("T1", "bob", "lead2", None);
    let row = h.row("T1");
    assert_eq!(row.state, TaskState::Assigned);
    assert_eq!(row.reminder_count, 0);
    assert_eq!(
        h.events("T1").last().unwrap().event,
        TaskEventKind::Reopened
    );
}

#[test]
fn reassign_releases_old_active_slot_in_same_transaction() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.start("T1", "alice").unwrap();
    h.assign("T1", "bob", "lead", None);
    h.assign("T2", "alice", "lead", None);
    h.start("T2", "alice").unwrap();
    assert_eq!(h.row("T2").state, TaskState::Active);
}

#[test]
fn same_agent_resend_refreshes_message_link_without_event() {
    let h = Harness::new();
    let first = h.assign("T1", "alice", "lead", None);
    let before = h.row("T1");
    let event_count = h.events("T1").len();
    let second = h.assign("T1", "alice", "lead2", Some(MoveTarget::Head));
    let after = h.row("T1");
    assert_eq!(event_count, h.events("T1").len());
    assert_eq!(before.position, after.position);
    assert_eq!(before.assigned_at, after.assigned_at);
    assert_eq!(
        after.assignment_message_id,
        second.envelope.message_id.unwrap()
    );
    assert!(
        h.backend
            .message_store()
            .load_message(&first.message_key)
            .unwrap()
            .unwrap()
            .envelope
            .acknowledged_at
            .is_some()
    );
}

#[test]
fn assign_with_before_survives_to_transaction() {
    let h = Harness::new();
    h.assign("T2", "alice", "lead", None);
    h.assign(
        "T1",
        "alice",
        "lead",
        Some(MoveTarget::Before {
            task_id: "T2".parse().unwrap(),
        }),
    );
    assert_eq!(h.positions("alice")["T1"], 1);
    let row = h
        .backend
        .message_store()
        .list_messages(&atm_storage::MessageQuery {
            team: h.team.clone(),
            agent: "alice".parse().unwrap(),
            sender: None,
            task_id: Some("T1".parse().unwrap()),
            limit: None,
        })
        .unwrap();
    assert_eq!(
        row[0].envelope.placement,
        Some(MoveTarget::Before {
            task_id: "T2".parse().unwrap()
        })
    );
}

#[test]
fn assign_before_invalid_target_is_rejected() {
    let h = Harness::new();
    h.assign("T1", "bob", "lead", None);
    let mut message = h.message("alice", "lead", "invalid before");
    message.envelope.task_id = Some("T2".parse().unwrap());
    message.envelope.placement = Some(MoveTarget::Before {
        task_id: "T1".parse().unwrap(),
    });
    let error = h.save(&message).expect_err("invalid target");
    assert_eq!(error.code(), AtmErrorCode::TaskMoveInvalid);
    assert!(error.message().contains("not an open queued task"));
    assert!(
        h.events("T2")
            .iter()
            .any(|event| event.event == TaskEventKind::Rejected)
    );
}

#[test]
fn start_when_another_task_active_is_rejected_active_elsewhere() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.assign("T2", "alice", "lead", None);
    h.start("T1", "alice").unwrap();
    let error = h.start("T2", "alice").expect_err("active conflict");
    assert_eq!(error.code(), AtmErrorCode::TaskMoveInvalid);
    assert!(error.message().contains("already has an active task"));
    assert_eq!(h.row("T2").state, TaskState::Assigned);
}

#[test]
fn start_moves_task_to_position_one_and_preserves_reminder_count() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.assign("T2", "alice", "lead", None);
    h.start("T2", "alice").unwrap();
    let row = h.row("T2");
    assert_eq!(row.position, Some(QueuePosition::HEAD));
    assert_eq!(row.reminder_count, 1);
    assert!(row.last_reminded_at.is_some());
}

#[test]
fn start_gate_rejects_member_actor_and_unrenderable_reminder() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let mut member_start = h.message("alice", "lead", "start");
    member_start.envelope.task_id = Some("T1".parse().unwrap());
    member_start.envelope.task_op = Some(TaskOp::Start);
    h.save(&member_start).expect_err("member start");
    let member = MemberKey::new(h.team.clone(), "alice".parse().unwrap());
    h.backend
        .task_store()
        .record_reminder(
            &member,
            &"T1".parse().unwrap(),
            IsoTimestamp::now(),
            ReminderOutcome::Unrenderable,
        )
        .unwrap();
    let mut daemon_start = h.message("alice", "atm-daemon", "start");
    daemon_start.envelope.task_id = Some("T1".parse().unwrap());
    daemon_start.envelope.task_op = Some(TaskOp::Start);
    h.save(&daemon_start).unwrap();
    assert_eq!(h.row("T1").state, TaskState::Assigned);
}

#[test]
fn close_renumbers_remaining_queue_contiguously() {
    let h = Harness::new();
    for task in ["T1", "T2", "T3", "T4"] {
        h.assign(task, "alice", "lead", None);
    }
    h.close("T2", "lead", "alice", TaskCloseOutcome::Completed)
        .unwrap();
    assert_eq!(
        h.positions("alice"),
        BTreeMap::from([("T1".into(), 1), ("T3".into(), 2), ("T4".into(), 3)])
    );
    assert_eq!(h.row("T2").position, None);
}

#[test]
fn writer_close_open_task_delivers_and_closes_in_one_write() {
    let h = Harness::new();
    for (task, outcome) in [
        ("T1", TaskCloseOutcome::Completed),
        ("T2", TaskCloseOutcome::Refused),
        ("T3", TaskCloseOutcome::Cancelled),
    ] {
        h.assign(task, "alice", "lead", None);
        h.close(task, "lead", "alice", outcome).unwrap();
        assert_eq!(h.row(task).state, TaskState::Complete(outcome));
        assert_eq!(
            h.events(task).last().unwrap().event.as_str(),
            outcome.as_str()
        );
    }
}

#[test]
fn writer_close_by_assigner_reports_to_assignee() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let report = h
        .close("T1", "alice", "lead", TaskCloseOutcome::Cancelled)
        .expect("assigner may close");
    assert_eq!(report.agent.as_str(), "alice");
    assert_eq!(
        h.row("T1").state,
        TaskState::Complete(TaskCloseOutcome::Cancelled)
    );
}

#[test]
fn writer_close_unknown_task_is_atomic() {
    let h = Harness::new();
    let mut report = h.message("lead", "alice", "unknown close");
    report.envelope.task_id = Some("UNKNOWN".parse().expect("task id"));
    report.envelope.task_op = Some(TaskOp::Close {
        outcome: TaskCloseOutcome::Completed,
        reason: Some("done".to_string()),
    });
    let key = report.message_key.clone();
    let error = h
        .save(&report)
        .expect_err("unknown task must fail atomically");
    assert_eq!(error.code(), AtmErrorCode::TaskNotFound);
    assert!(
        h.backend
            .message_store()
            .load_message(&key)
            .expect("message lookup")
            .is_none()
    );
}

#[test]
fn writer_close_already_closed_omits_task_event() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.close("T1", "lead", "alice", TaskCloseOutcome::Completed)
        .unwrap();
    let before = h.events("T1").len();
    let report = h
        .close("T1", "lead", "alice", TaskCloseOutcome::Refused)
        .unwrap();
    let stored = h
        .backend
        .message_store()
        .load_message(&report.message_key)
        .unwrap()
        .unwrap();
    assert_eq!(stored.envelope.task_id, None);
    assert_eq!(h.events("T1").len(), before);
    assert_eq!(
        h.row("T1").state,
        TaskState::Complete(TaskCloseOutcome::Completed)
    );
}

#[test]
fn writer_close_by_third_party_is_rejected() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let before = h.row("T1");
    let event_count = h.events("T1").len();
    let mut report = h.message("lead", "intruder", "third-party report");
    report.envelope.task_id = Some("T1".parse().unwrap());
    report.envelope.task_op = Some(TaskOp::Close {
        outcome: TaskCloseOutcome::Completed,
        reason: Some("reason".to_owned()),
    });
    let error = h.save(&report).expect_err("third party");
    assert_eq!(error.code(), AtmErrorCode::TaskNotCounterparty);
    assert!(error.message().contains("not assigned to or by"));
    assert!(error.message().contains("report delivered"));
    assert_eq!(h.row("T1"), before);
    let events = h.events("T1");
    assert_eq!(events.len(), event_count + 1);
    assert_eq!(events.last().unwrap().event, TaskEventKind::Rejected);
    let stored = h
        .backend
        .message_store()
        .load_message(&report.message_key)
        .unwrap()
        .expect("plain rejected report");
    assert_eq!(stored.envelope.text, "third-party report");
    assert_eq!(stored.envelope.task_id, None);
    assert_eq!(stored.envelope.task_op, None);
}

#[test]
fn writer_stale_counterparty_is_rejected() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let before = h.row("T1");
    let event_count = h.events("T1").len();
    let mut report = h.message("other-lead", "alice", "stale-counterparty report");
    report.envelope.task_id = Some("T1".parse().unwrap());
    report.envelope.task_op = Some(TaskOp::Close {
        outcome: TaskCloseOutcome::Completed,
        reason: Some("reason".to_owned()),
    });
    let error = h.save(&report).expect_err("stale recipient");
    assert_eq!(error.code(), AtmErrorCode::TaskStaleCounterparty);
    assert!(error.message().contains("no longer the counterparty"));
    assert!(error.message().contains("report delivered"));
    assert_eq!(h.row("T1"), before);
    let events = h.events("T1");
    assert_eq!(events.len(), event_count + 1);
    assert_eq!(events.last().unwrap().event, TaskEventKind::Rejected);
    let stored = h
        .backend
        .message_store()
        .load_message(&report.message_key)
        .unwrap()
        .expect("plain rejected report");
    assert_eq!(stored.envelope.text, "stale-counterparty report");
    assert_eq!(stored.envelope.task_id, None);
    assert_eq!(stored.envelope.task_op, None);
}

#[test]
fn writer_refusal_releases_next_queued_task() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.assign("T2", "alice", "lead", None);
    h.close("T1", "lead", "alice", TaskCloseOutcome::Refused)
        .expect("refusal");
    assert_eq!(h.positions("alice"), BTreeMap::from([("T2".into(), 1)]));
}

#[test]
fn writer_assign_to_active_member_stays_queued() {
    let h = Harness::new();
    h.assign("ACTIVE", "alice", "lead", None);
    h.start("ACTIVE", "alice").expect("start active task");
    h.assign("QUEUED", "alice", "lead", None);
    let queued = h.row("QUEUED");
    assert_eq!(queued.state, TaskState::Assigned);
    assert_eq!(queued.reminder_count, 0);
    assert_eq!(queued.lead_notified_count, 0);
    assert_eq!(queued.position.map(QueuePosition::get), Some(2));
}

#[test]
fn assigned_at_set_only_by_assignment() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let assigned = h.row("T1").assigned_at;
    h.start("T1", "alice").unwrap();
    assert_eq!(h.row("T1").assigned_at, assigned);
    h.close("T1", "lead", "alice", TaskCloseOutcome::Completed)
        .unwrap();
    assert_eq!(h.row("T1").assigned_at, assigned);
}

#[test]
fn trailing_refusal_run_counts_raw_stored_event_values() {
    let h = Harness::new();
    let connection = Connection::open(&h.path).unwrap();
    for (seq, event) in [
        "refused",
        "refused",
        "completed",
        "refused",
        "refused",
        "refused",
    ]
    .into_iter()
    .enumerate()
    {
        connection.execute("INSERT INTO task_events(team,task_id,assignee,seq,at,event,actor) VALUES (?1,?2,'alice',?3,?4,?5,'alice')", params![h.team.as_str(), format!("T{seq}"), seq + 1, IsoTimestamp::now().to_string(), event]).unwrap();
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let run = runtime
        .block_on(h.backend.async_task_ledger_reader().refusal_run(
            h.team.clone(),
            "alice".parse().unwrap(),
            atm_storage::ReadDeadline::new(std::time::Duration::from_secs(1)).unwrap(),
        ))
        .unwrap();
    assert_eq!(run.count, 3);
    assert!(run.started_at.is_some());
}

#[tokio::test]
async fn open_tasks_for_team_orders_by_assignee_position() {
    let h = Harness::new();
    h.assign("B2", "bob", "lead", None);
    h.assign("A2", "alice", "lead", None);
    h.assign("A1", "alice", "lead", Some(MoveTarget::Head));
    h.assign("DONE", "alice", "lead", None);
    h.close("DONE", "lead", "alice", TaskCloseOutcome::Completed)
        .unwrap();
    let rows = h
        .backend
        .async_task_ledger_reader()
        .open_tasks_for_team(
            h.team.clone(),
            atm_storage::ReadDeadline::new(std::time::Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| (row.assignee.as_str(), row.task_id.as_str()))
            .collect::<Vec<_>>(),
        vec![("alice", "A1"), ("alice", "A2"), ("bob", "B2")]
    );
}

#[tokio::test]
async fn refusal_run_reads_the_trailing_run() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.close("T1", "lead", "alice", TaskCloseOutcome::Refused)
        .unwrap();
    let run = h
        .backend
        .async_task_ledger_reader()
        .refusal_run(
            h.team.clone(),
            "alice".parse().unwrap(),
            atm_storage::ReadDeadline::new(std::time::Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(run.count, 1);
    assert_eq!(run.started_at, h.events("T1").last().map(|event| event.at));
}
