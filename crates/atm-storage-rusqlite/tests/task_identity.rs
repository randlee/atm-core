use std::collections::BTreeMap;

use atm_storage::contract::{Message, MessageKey};
use atm_storage::schema::{AtmMessageId, MessageEnvelope};
use atm_storage::{
    AgentName, AtmErrorCode, IsoTimestamp, MailboxScope, MemberKey, MessageAdmissionOutcome,
    MessageQuery, MessageSearchQuery, MessageWriteOrigin, MoveTarget, QueuePosition, ReadDeadline,
    ReminderOutcome, SearchAtom, SearchExpression, SearchMatchField, TaskCloseOutcome,
    TaskEventKind, TaskId, TaskOp, TaskState, TeamName,
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
        self.assign_with_outcome(task, assignee, assigner, placement)
            .0
    }

    fn assign_with_outcome(
        &self,
        task: &str,
        assignee: &str,
        assigner: &str,
        placement: Option<MoveTarget>,
    ) -> (Message, MessageAdmissionOutcome) {
        let task_id: TaskId = task.parse().expect("task");
        let mut message = self.message(assignee, assigner, &format!("assign {task}"));
        message.envelope.task_id = Some(task_id);
        message.envelope.placement = placement;
        message.envelope.requires_ack = false;
        message.envelope.pending_ack_at = None;
        let outcome = self
            .backend
            .message_store()
            .admit_message_with_provenance(&message, MessageWriteOrigin::Local)
            .expect("assign");
        (message, outcome)
    }

    fn start(&self, task: &str, assignee: &str) -> Result<(), atm_storage::AtmError> {
        let message = self.start_message(task, assignee);
        self.save(&message)
    }

    fn start_message(&self, task: &str, actor: &str) -> Message {
        let mut message = self.message("lead", actor, &format!("start {task}"));
        message.envelope.task_id = Some(task.parse().expect("task"));
        message.envelope.task_op = Some(TaskOp::Start);
        message
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

    fn move_task(&self, task: &str, actor: &str, target: MoveTarget) {
        self.backend
            .task_store()
            .move_task(
                &self.team,
                &task.parse().expect("task"),
                &actor.parse().expect("actor"),
                &target,
                IsoTimestamp::now(),
            )
            .expect("move task");
    }
}

#[test]
fn assignment_at_every_position_emits_task_queued_with_position() {
    let h = Harness::new();
    for (task, expected) in [("T1", 1), ("T2", 2), ("T3", 3)] {
        let (message, outcome) = h.assign_with_outcome(task, "alice", "lead", None);
        assert!(!message.envelope.requires_ack);
        assert!(message.envelope.pending_ack_at.is_none());
        assert_eq!(outcome.queued_position, Some(expected));
        assert!(outcome.reassign_notice.is_none());
    }
}

#[test]
fn assignment_write_leaves_requires_ack_false_and_pending_ack_null() {
    let h = Harness::new();
    let assignment = h.assign("T1", "alice", "lead", None);
    let stored = h
        .backend
        .message_store()
        .load_message(&assignment.message_key)
        .unwrap()
        .expect("stored assignment");
    assert!(!stored.envelope.requires_ack);
    assert!(stored.envelope.pending_ack_at.is_none());
}

#[test]
fn assignment_write_creates_no_pending_marker() {
    let h = Harness::new();
    let assignment = h.assign("T1", "alice", "lead", None);
    let nudge_pending_at: Option<String> = Connection::open(&h.path)
        .unwrap()
        .query_row(
            "SELECT nudge_pending_at FROM mail_message_states WHERE message_key = ?1",
            params![assignment.message_key.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(nudge_pending_at.is_none());
}

#[test]
pub(crate) fn assignee_task_report_is_plain_message_and_leaves_task_unchanged() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.backend
        .task_store()
        .record_reminder(
            &MemberKey::new(h.team.clone(), "alice".parse().unwrap()),
            &"T1".parse().unwrap(),
            IsoTimestamp::now(),
            ReminderOutcome::Emitted,
        )
        .expect("record reminder");
    let before = h.row("T1");
    let event_count = h.events("T1").len();
    let mut report = h.message("lead", "alice", "progress report");
    report.envelope.task_id = Some("T1".parse().unwrap());

    let outcome = h
        .backend
        .message_store()
        .admit_message_with_provenance(&report, MessageWriteOrigin::Local)
        .expect("admit task-linked report");

    assert_eq!(h.row("T1"), before);
    assert_eq!(h.events("T1").len(), event_count);
    assert!(outcome.queued_position.is_none());
    assert!(outcome.reassign_notice.is_none());
}

#[test]
fn reassign_inserts_closed_reassigned_message_to_old_assignee_in_same_transaction() {
    let h = Harness::new();
    let old = h.assign("T1", "alice", "lead", None);
    h.assign("T2", "alice", "lead", None);
    h.assign("T3", "bob", "lead", None);
    let (_, outcome) = h.assign_with_outcome("T1", "bob", "lead", Some(MoveTarget::Head));
    let row = h.row("T1");
    assert_eq!(row.assignee.as_str(), "bob");
    assert_eq!(h.positions("alice")["T2"], 1);
    assert_eq!(h.positions("bob")["T1"], 1);
    assert_eq!(outcome.queued_position, Some(1));
    assert_eq!(
        outcome.reassign_notice.as_ref().unwrap().agent.as_str(),
        "alice"
    );
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
            .is_none()
    );
    let notices = h
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
    let notice = notices
        .iter()
        .find(|message| message.envelope.summary.as_deref() == Some("task_closed:T1"))
        .expect("reassignment notice to old assignee");
    assert_eq!(notice.envelope.from.as_str(), "lead");
    assert_eq!(notice.envelope.text, "task T1 was reassigned to bob");
    assert!(!notice.envelope.requires_ack);
    assert!(notice.envelope.task_op.is_none());
    let state_rows: u32 = Connection::open(&h.path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM mail_message_states WHERE team = ?1 AND agent = ?2 AND message_key = ?3",
            params![h.team.as_str(), "alice", notice.message_key.as_ref()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state_rows, 1);
    assert!(!h.positions("alice").contains_key("T1"));

    let rollback = Harness::new();
    rollback.assign("T1", "alice", "lead", None);
    let before_row = rollback.row("T1");
    let before_events = rollback.events("T1");
    Connection::open(&rollback.path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER fail_reassign_notice_state
             BEFORE INSERT ON mail_message_states
             WHEN EXISTS (
                 SELECT 1 FROM mail_messages
                  WHERE message_key = NEW.message_key
                    AND summary = 'task_closed:T1'
             )
             BEGIN SELECT RAISE(ABORT, 'forced notice state failure'); END;",
        )
        .unwrap();
    let mut reassignment = rollback.message("bob", "lead", "reassign T1");
    reassignment.envelope.task_id = Some("T1".parse().unwrap());
    let error = rollback.save(&reassignment).expect_err("forced rollback");
    assert!(error.message().contains("message state"));
    assert_eq!(rollback.row("T1"), before_row);
    assert_eq!(rollback.events("T1"), before_events);
    let connection = Connection::open(&rollback.path).unwrap();
    for table in ["mail_messages", "mail_message_states"] {
        let count: u32 = connection
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM {table} WHERE message_key = ?1 OR message_key IN (SELECT message_key FROM mail_messages WHERE summary = 'task_closed:T1')"
                ),
                params![reassignment.message_key.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "{table} retained rolled-back reassignment");
    }
}

#[test]
fn reassign_notice_row_is_never_admitted_as_an_assignment() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.assign("T1", "bob", "lead", None);
    assert!(!h.positions("alice").contains_key("T1"));
    assert_eq!(h.positions("bob")["T1"], 1);
    let assigned_events = h
        .events("T1")
        .into_iter()
        .filter(|event| event.event == TaskEventKind::Assigned)
        .count();
    assert_eq!(assigned_events, 1);
}

#[tokio::test]
async fn reassign_notice_has_state_row_and_projection() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let (_, outcome) = h.assign_with_outcome("T1", "bob", "lead", None);
    let notice = outcome.reassign_notice.expect("reassignment notice");
    let scope = MailboxScope::new(h.team.clone(), "alice".parse().unwrap());
    let listed = h
        .backend
        .async_mailbox_reader()
        .list_messages(
            scope.clone(),
            MessageQuery {
                team: h.team.clone(),
                agent: "alice".parse().unwrap(),
                sender: None,
                task_id: Some("T1".parse().unwrap()),
                limit: None,
            },
            ReadDeadline::new(std::time::Duration::from_secs(1)).unwrap(),
        )
        .await
        .expect("read reassignment notice");
    assert_eq!(
        listed
            .iter()
            .filter(|message| message.message_key == notice.message_key)
            .count(),
        1
    );
    h.backend
        .async_message_store()
        .apply_read_display_state_async(scope, vec![notice.message_key.clone()], None)
        .await
        .expect("mark notice read");
    let read: bool = Connection::open(&h.path)
        .unwrap()
        .query_row(
            "SELECT read FROM mail_message_states WHERE message_key = ?1",
            params![notice.message_key.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(read);
    let search = h
        .backend
        .message_search_store()
        .search(&MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::phrase("task T1 was reassigned to bob").unwrap(),
            )),
            ..MessageSearchQuery::default()
        })
        .expect("search projection");
    assert!(
        search
            .matches
            .iter()
            .any(|matched| matched.key.message_key == notice.message_key
                && matched.match_fields.contains(&SearchMatchField::BodyText))
    );
}

#[test]
fn insert_message_canonical_is_the_only_insert_path() {
    let h = Harness::new();
    let assignment = h.assign("T1", "alice", "lead", None);
    let before_events = h.events("T1");
    let duplicate = h
        .backend
        .message_store()
        .save_message_if_absent(&assignment)
        .expect("duplicate admission");
    assert!(duplicate.is_some());
    assert_eq!(h.events("T1"), before_events);
    let connection = Connection::open(&h.path).unwrap();
    for table in [
        "mail_messages",
        "mail_message_states",
        "mail_message_search_documents",
    ] {
        let count: u32 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE message_key = ?1"),
                params![assignment.message_key.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "duplicate inserted another {table} row");
    }
}

#[test]
fn reopen_complete_task_emits_task_queued() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.close("T1", "lead", "alice", TaskCloseOutcome::Refused)
        .unwrap();
    let (_, admission) = h.assign_with_outcome("T1", "bob", "lead", None);
    let row = h.row("T1");
    assert_eq!(row.state, TaskState::Assigned);
    assert_eq!(row.reminder_count, 0);
    assert_eq!(admission.queued_position, Some(1));
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
fn same_assignee_reassign_refreshes_row_and_emits_task_queued() {
    let h = Harness::new();
    let first = h.assign("T1", "alice", "lead", None);
    let before = h.row("T1");
    let event_count = h.events("T1").len();
    let (second, admission) = h.assign_with_outcome("T1", "alice", "lead", Some(MoveTarget::Head));
    let after = h.row("T1");
    assert_eq!(event_count, h.events("T1").len());
    assert_eq!(before.position, after.position);
    assert_eq!(before.assigned_at, after.assigned_at);
    assert_eq!(admission.queued_position, Some(1));
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
            .is_none()
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
pub(crate) fn start_while_another_task_is_active_is_rejected() {
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
pub(crate) fn start_by_assignee_moves_assigned_task_to_head_and_active() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.assign("T2", "alice", "lead", None);
    h.assign("T3", "alice", "lead", None);
    let start = h.start_message("T3", "alice");
    let message_id = start.envelope.message_id;
    h.save(&start).unwrap();
    let row = h.row("T3");
    assert_eq!(row.position, Some(QueuePosition::HEAD));
    assert_eq!(row.state, TaskState::Active);
    assert_eq!(row.reminder_count, 0);
    assert!(row.last_reminded_at.is_none());
    assert_eq!(
        h.positions("alice"),
        BTreeMap::from([("T1".into(), 2), ("T2".into(), 3), ("T3".into(), 1)])
    );
    let started: Vec<_> = h
        .events("T3")
        .into_iter()
        .filter(|event| event.event == TaskEventKind::Started)
        .collect();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].message_id, message_id);
}

#[test]
pub(crate) fn start_without_prior_reminder_succeeds() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.start("T1", "alice").expect("assignee start");
    assert_eq!(h.row("T1").state, TaskState::Active);
}

#[test]
pub(crate) fn daemon_actor_can_no_longer_start_a_task() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let daemon_start = h.start_message("T1", "atm-daemon");
    let error = h.save(&daemon_start).expect_err("daemon cannot start");
    assert_eq!(error.code(), AtmErrorCode::TaskNotCounterparty);
    assert_eq!(h.row("T1").state, TaskState::Assigned);
}

#[test]
pub(crate) fn start_by_non_assignee_is_rejected() {
    let h = Harness::new();
    for (task, actor) in [("ASSIGNER", "lead"), ("THIRD", "bob")] {
        h.assign(task, "alice", "lead", None);
        let error = h
            .save(&h.start_message(task, actor))
            .expect_err("only the assignee starts");
        assert_eq!(error.code(), AtmErrorCode::TaskNotCounterparty);
        assert_eq!(h.row(task).state, TaskState::Assigned);
    }
}

#[test]
pub(crate) fn start_on_complete_task_is_rejected() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.close("T1", "lead", "alice", TaskCloseOutcome::Completed)
        .unwrap();
    let error = h
        .save(&h.start_message("T1", "alice"))
        .expect_err("complete task cannot start");
    assert_eq!(error.code(), AtmErrorCode::TaskAlreadyClosed);
    let rejected = h.events("T1").pop().expect("rejected event");
    assert_eq!(rejected.event, TaskEventKind::Rejected);
    assert_eq!(
        rejected.from_state,
        Some(TaskState::Complete(TaskCloseOutcome::Completed))
    );
    assert_eq!(rejected.to_state, rejected.from_state);
}

#[test]
pub(crate) fn duplicate_start_is_rejected_nothing_delivered_one_rejected_row() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.start("T1", "alice").expect("first start");
    let duplicate = h.start_message("T1", "alice");
    let error = h.save(&duplicate).expect_err("duplicate start");
    assert_eq!(error.code(), AtmErrorCode::TaskAlreadyActive);
    assert!(error.message().starts_with("task T1 is already active"));
    assert!(
        h.backend
            .message_store()
            .load_message(&duplicate.message_key)
            .unwrap()
            .is_none()
    );
    let events = h.events("T1");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event == TaskEventKind::Started)
            .count(),
        1
    );
    let rejected: Vec<_> = events
        .iter()
        .filter(|event| event.event == TaskEventKind::Rejected)
        .collect();
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].from_state, Some(TaskState::Active));
    assert_eq!(rejected[0].to_state, Some(TaskState::Active));
    assert_eq!(rejected[0].message_id, duplicate.envelope.message_id);
}

#[test]
pub(crate) fn rejected_start_rolls_back_report_state_and_projection_atomically() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.start("T1", "alice").expect("first start");
    let duplicate = h.start_message("T1", "alice");
    h.save(&duplicate).expect_err("duplicate start");
    let connection = Connection::open(&h.path).expect("open database");
    for table in [
        "mail_messages",
        "mail_message_states",
        "mail_message_search_documents",
    ] {
        let count: u32 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE message_key=?1"),
                params![duplicate.message_key.as_str()],
                |row| row.get(0),
            )
            .expect("count rolled-back row");
        assert_eq!(count, 0, "{table} retained rejected start");
    }
    assert_eq!(
        h.events("T1")
            .iter()
            .filter(|event| event.event == TaskEventKind::Rejected)
            .count(),
        1
    );
}

#[test]
pub(crate) fn concurrent_starts_admit_exactly_one_started_event() {
    use std::sync::{Arc, Barrier};

    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let first = h.start_message("T1", "alice");
    let second = h.start_message("T1", "alice");
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [first, second]
        .into_iter()
        .map(|message| {
            let backend = h.backend.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                backend.message_store().save_message(&message)
            })
        })
        .collect();
    barrier.wait();
    let writer_count = handles.len();
    let (joined_tx, joined_rx) = std::sync::mpsc::channel();
    for handle in handles {
        let joined_tx = joined_tx.clone();
        std::thread::spawn(move || {
            let result = handle.join().expect("writer thread");
            joined_tx.send(result).expect("join result receiver");
        });
    }
    drop(joined_tx);
    let results: Vec<_> = (0..writer_count)
        .map(|_| {
            joined_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("writer thread join timed out")
        })
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .filter(|error| error.code() == AtmErrorCode::TaskAlreadyActive)
            .count(),
        1
    );
    let events = h.events("T1");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event == TaskEventKind::Started)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event == TaskEventKind::Rejected)
            .count(),
        1
    );
}

#[test]
pub(crate) fn start_racing_reassign_is_rejected_as_not_counterparty() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let stale_start = h.start_message("T1", "alice");
    h.assign("T1", "bob", "lead", None);
    let error = h.save(&stale_start).expect_err("stale assignee start");
    assert_eq!(error.code(), AtmErrorCode::TaskNotCounterparty);
    assert_eq!(h.row("T1").assignee.as_str(), "bob");
    assert_eq!(h.row("T1").state, TaskState::Assigned);
}

#[test]
pub(crate) fn start_of_missing_task_appends_null_state_rejected_row() {
    let h = Harness::new();
    let mut start = h.start_message("MISSING", "alice");
    start.agent = "alice".parse().expect("requested assignee");
    let error = h.save(&start).expect_err("missing task");
    assert_eq!(error.code(), AtmErrorCode::TaskNotFound);
    let event = h.events("MISSING").pop().expect("rejected event");
    assert_eq!(event.event, TaskEventKind::Rejected);
    assert_eq!(event.assignee.as_str(), "alice");
    assert_eq!(event.from_state, None);
    assert_eq!(event.to_state, None);
}

#[test]
pub(crate) fn move_of_active_task_appends_moved_head_to_head() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.start("T1", "alice").expect("start");
    h.move_task("T1", "lead", MoveTarget::End);
    let event = h.events("T1").pop().expect("moved event");
    assert_eq!(event.event, TaskEventKind::Moved);
    assert_eq!(event.detail.as_deref(), Some("1→1"));
    assert_eq!(h.row("T1").position, Some(QueuePosition::HEAD));
}

#[test]
pub(crate) fn move_of_complete_task_appends_rejected_row() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    h.close("T1", "lead", "alice", TaskCloseOutcome::Completed)
        .unwrap();
    let error = h
        .backend
        .task_store()
        .move_task(
            &h.team,
            &"T1".parse().unwrap(),
            &"lead".parse().unwrap(),
            &MoveTarget::Head,
            IsoTimestamp::now(),
        )
        .expect_err("complete task cannot move");
    assert_eq!(error.code(), AtmErrorCode::TaskAlreadyClosed);
    let event = h.events("T1").pop().expect("rejected event");
    assert_eq!(event.event, TaskEventKind::Rejected);
    assert_eq!(
        event.from_state,
        Some(TaskState::Complete(TaskCloseOutcome::Completed))
    );
    assert_eq!(event.to_state, event.from_state);
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
fn close_each_outcome_persists_column_and_event() {
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
pub(crate) fn close_of_complete_task_retains_report_and_strips_link() {
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
pub(crate) fn close_by_non_party_is_rejected_and_retained() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    let before = h.row("T1");
    let before_events = h.events("T1");
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
    assert_eq!(events.len(), before_events.len() + 1);
    let rejected = events.last().expect("rejected event");
    assert_eq!(rejected.event, TaskEventKind::Rejected);
    assert_eq!(rejected.assignee.as_str(), "alice");
    assert_eq!(
        rejected.actor,
        atm_storage::TaskActor::Member("intruder".parse().unwrap())
    );
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
fn close_by_stale_counterparty_is_rejected_atomically() {
    let h = Harness::new();
    h.assign("T1", "alice", "lead", None);
    Connection::open(&h.path)
        .unwrap()
        .execute(
            "UPDATE tasks SET assigner = 'new-lead' WHERE team = ?1 AND task_id = 'T1'",
            params![h.team.as_str()],
        )
        .unwrap();
    let before = h.row("T1");
    let before_events = h.events("T1");
    let mut report = h.message("lead", "alice", "stale close report");
    report.envelope.task_id = Some("T1".parse().unwrap());
    report.envelope.task_op = Some(TaskOp::Close {
        outcome: TaskCloseOutcome::Completed,
        reason: Some("done".to_owned()),
    });
    let error = h.save(&report).expect_err("superseded assigner is stale");
    assert_eq!(error.code(), AtmErrorCode::TaskStaleCounterparty);
    assert!(error.message().contains("no longer the counterparty"));
    assert!(error.message().contains("report delivered"));
    assert_eq!(h.row("T1"), before);
    let events = h.events("T1");
    assert_eq!(events.len(), before_events.len() + 1);
    let rejected = events.last().expect("rejected event");
    assert_eq!(rejected.event, TaskEventKind::Rejected);
    assert_eq!(rejected.assignee.as_str(), "alice");
    assert_eq!(
        rejected.actor,
        atm_storage::TaskActor::Member("alice".parse().unwrap())
    );
    let stored = h
        .backend
        .message_store()
        .load_message(&report.message_key)
        .unwrap()
        .expect("plain rejected report");
    assert_eq!(stored.envelope.text, "stale close report");
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

    h.move_task("T1", "lead", MoveTarget::End);
    assert_eq!(h.row("T1").assigned_at, assigned);

    h.start("T1", "alice").unwrap();
    assert_eq!(h.row("T1").assigned_at, assigned);

    h.assign("T1", "bob", "lead", None);
    let reassigned = h.row("T1");
    let reassigned_event = h.events("T1").last().cloned().expect("reassigned event");
    assert_eq!(reassigned_event.event, TaskEventKind::Reassigned);
    assert_eq!(reassigned.assigned_at, reassigned_event.at);
    assert_eq!(reassigned.last_reminded_at, None);

    h.close("T1", "lead", "bob", TaskCloseOutcome::Completed)
        .unwrap();
    assert_eq!(h.row("T1").assigned_at, reassigned.assigned_at);

    h.assign("T1", "alice", "lead", None);
    let reopened = h.row("T1");
    let reopened_event = h.events("T1").last().cloned().expect("reopened event");
    assert_eq!(reopened_event.event, TaskEventKind::Reopened);
    assert_eq!(reopened.assigned_at, reopened_event.at);
    assert_eq!(reopened.last_reminded_at, None);
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
