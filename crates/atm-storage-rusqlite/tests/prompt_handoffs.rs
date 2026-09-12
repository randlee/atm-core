use std::time::Duration;

use atm_storage::{
    AgentName, AtmMessageId, BuiltInNudgeTemplateKind, IsoTimestamp, Message, MessageEnvelope,
    MessageKey, PromptHandoff, PromptTrigger, ReadDeadline, TaskCloseOutcome, TaskId, TaskOp,
    TaskState, TeamName,
};
use atm_storage_rusqlite::SqliteStorageBackend;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde_json::Map;
use tempfile::TempDir;

const PRE_BB_DDL: &str = include_str!("fixtures/pre_bb_task_and_mail.sql");
const PROMPT_HANDOFF_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS prompt_handoffs (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    kind TEXT NOT NULL,
    task_id TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt >= 0),
    trigger TEXT NOT NULL CHECK(trigger IN ('steer', 'task_pass')),
    at TEXT NOT NULL,
    UNIQUE (team, agent, message_key, attempt)
);
CREATE INDEX IF NOT EXISTS prompt_handoffs_task ON prompt_handoffs(team, task_id, at);
"#;

struct Harness {
    _root: TempDir,
    path: std::path::PathBuf,
    backend: SqliteStorageBackend,
    team: TeamName,
    task_id: TaskId,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("mail.db");
        let backend = SqliteStorageBackend::new(&path).expect("backend");
        Self {
            _root: root,
            path,
            backend,
            team: "bb6-team".parse().expect("team"),
            task_id: "BB6-1".parse().expect("task id"),
        }
    }

    fn handoff(&self, key: &str, attempt: u32, trigger: PromptTrigger, at: &str) -> PromptHandoff {
        PromptHandoff {
            team: self.team.clone(),
            agent: "worker".parse().expect("agent"),
            message_key: key.parse().expect("message key"),
            kind: if attempt == 0 {
                BuiltInNudgeTemplateKind::TaskReady
            } else {
                BuiltInNudgeTemplateKind::TaskReminder
            },
            task_id: self.task_id.clone(),
            attempt,
            trigger,
            at: at.parse().expect("timestamp"),
        }
    }

    async fn list(&self) -> Vec<PromptHandoff> {
        self.backend
            .async_task_ledger_reader()
            .list_prompt_handoffs(
                self.team.clone(),
                self.task_id.clone(),
                ReadDeadline::new(Duration::from_secs(1)).expect("deadline"),
            )
            .await
            .expect("list prompt handoffs")
    }
}

#[tokio::test]
async fn record_prompt_handoff_round_trips_every_trigger() {
    let harness = Harness::new();
    let steer = harness.handoff(
        "atm:01M2BB60000000000000000001",
        0,
        PromptTrigger::Steer,
        "2026-09-12T15:27:34Z",
    );
    let task_pass = harness.handoff(
        "atm:01M2BB60000000000000000002",
        1,
        PromptTrigger::TaskPass,
        "2026-09-12T15:28:34Z",
    );
    harness
        .backend
        .task_store()
        .record_prompt_handoff(&steer)
        .expect("steer");
    harness
        .backend
        .task_store()
        .record_prompt_handoff(&task_pass)
        .expect("task pass");
    assert_eq!(harness.list().await, vec![steer, task_pass]);
}

#[tokio::test]
async fn record_prompt_handoff_ignores_duplicate_identity() {
    let harness = Harness::new();
    let row = harness.handoff(
        "atm:01M2BB60000000000000000003",
        0,
        PromptTrigger::Steer,
        "2026-09-12T15:27:34Z",
    );
    harness
        .backend
        .task_store()
        .record_prompt_handoff(&row)
        .expect("first");
    harness
        .backend
        .task_store()
        .record_prompt_handoff(&row)
        .expect("duplicate");
    assert_eq!(harness.list().await, vec![row]);
}

#[tokio::test]
async fn record_prompt_handoff_keeps_reminder_attempts_distinct() {
    let harness = Harness::new();
    let first = harness.handoff(
        "atm:01M2BB60000000000000000004",
        1,
        PromptTrigger::TaskPass,
        "2026-09-12T15:27:34Z",
    );
    let second = harness.handoff(
        "atm:01M2BB60000000000000000004",
        2,
        PromptTrigger::TaskPass,
        "2026-09-12T15:28:34Z",
    );
    harness
        .backend
        .task_store()
        .record_prompt_handoff(&first)
        .expect("first attempt");
    harness
        .backend
        .task_store()
        .record_prompt_handoff(&second)
        .expect("second attempt");
    assert_eq!(harness.list().await, vec![first, second]);
}

#[tokio::test]
async fn list_prompt_handoffs_orders_by_time_then_rowid() {
    let harness = Harness::new();
    let later = harness.handoff(
        "atm:01M2BB60000000000000000005",
        0,
        PromptTrigger::Steer,
        "2026-09-12T15:28:34Z",
    );
    let tied_first = harness.handoff(
        "atm:01M2BB60000000000000000006",
        0,
        PromptTrigger::Steer,
        "2026-09-12T15:27:34Z",
    );
    let tied_second = harness.handoff(
        "atm:01M2BB60000000000000000007",
        0,
        PromptTrigger::TaskPass,
        "2026-09-12T15:27:34Z",
    );
    for row in [&later, &tied_first, &tied_second] {
        harness
            .backend
            .task_store()
            .record_prompt_handoff(row)
            .expect("record");
    }
    assert_eq!(harness.list().await, vec![tied_first, tied_second, later]);
}

#[tokio::test]
async fn prompt_handoff_row_with_unknown_trigger_fails_decode() {
    let harness = Harness::new();
    let connection = Connection::open(&harness.path).expect("connection");
    connection
        .execute_batch("PRAGMA ignore_check_constraints = ON;")
        .expect("disable checks");
    connection.execute(
        "INSERT INTO prompt_handoffs(team, agent, message_key, kind, task_id, attempt, trigger, at)
         VALUES (?1, 'worker', 'atm:01M2BB60000000000000000008', 'task_ready', ?2, 0, 'unknown', '2026-09-12T15:27:34Z')",
        params![harness.team.as_str(), harness.task_id.as_str()],
    ).expect("malformed fixture row");
    drop(connection);
    let error = harness
        .backend
        .async_task_ledger_reader()
        .list_prompt_handoffs(
            harness.team.clone(),
            harness.task_id.clone(),
            ReadDeadline::new(Duration::from_secs(1)).expect("deadline"),
        )
        .await
        .expect_err("unknown trigger must fail decode");
    assert!(
        error
            .to_string()
            .contains("failed to decode prompt handoff"),
        "{error}"
    );
}

#[test]
fn pre_bb_fixture_opens_and_gains_prompt_handoffs_table() {
    let root = tempfile::tempdir().expect("tempdir");
    let path = root.path().join("mail.db");
    let connection = Connection::open(&path).expect("connection");
    connection.execute_batch(PRE_BB_DDL).expect("pre-BB schema");
    connection.execute(
        "INSERT INTO tasks(team, task_id, assignee, assigner, state, position,
          assignment_message_id, description, assigned_at, updated_at)
         VALUES ('bb6-team', 'existing', 'worker', 'lead', 'assigned', 1,
          '01M2BB60000000000000000009', 'existing task', '2026-09-12T15:27:34Z', '2026-09-12T15:27:34Z')",
        [],
    ).expect("existing task");
    drop(connection);
    let _backend = SqliteStorageBackend::new(&path).expect("open pre-BB database");
    let connection = Connection::open(path).expect("reopen");
    let table_count: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'prompt_handoffs'",
            [],
            |row| row.get(0),
        )
        .expect("table count");
    let existing: String = connection
        .query_row(
            "SELECT description FROM tasks WHERE team = 'bb6-team' AND task_id = 'existing'",
            [],
            |row| row.get(0),
        )
        .expect("existing row");
    assert_eq!(table_count, 1);
    assert_eq!(existing, "existing task");
}

#[test]
fn pre_bb_ddl_set_reads_and_writes_after_prompt_handoffs_created() {
    let root = tempfile::tempdir().expect("tempdir");
    let path = root.path().join("mail.db");
    let connection = Connection::open(&path).expect("connection");
    connection
        .execute_batch(PROMPT_HANDOFF_DDL)
        .expect("new additive table");
    connection
        .execute_batch(PRE_BB_DDL)
        .expect("frozen pre-BB DDL");
    drop(connection);

    let backend = SqliteStorageBackend::new(&path).expect("backend");
    let team: TeamName = "bb6-team".parse().expect("team");
    let task_id: TaskId = "BB6-COMPAT".parse().expect("task id");
    let assignee: AgentName = "worker".parse().expect("assignee");
    let assigner: AgentName = "lead".parse().expect("assigner");
    backend
        .message_store()
        .save_message(&task_message(
            team.clone(),
            assignee.clone(),
            assigner.clone(),
            task_id.clone(),
            None,
        ))
        .expect("assign");
    backend
        .message_store()
        .save_message(&task_message(
            team.clone(),
            assigner.clone(),
            assignee.clone(),
            task_id.clone(),
            Some(TaskOp::Start),
        ))
        .expect("start");
    backend
        .message_store()
        .save_message(&task_message(
            team.clone(),
            assigner,
            assignee,
            task_id.clone(),
            Some(TaskOp::Close {
                outcome: TaskCloseOutcome::Completed,
                reason: None,
            }),
        ))
        .expect("close");
    drop(backend);

    let connection = Connection::open(path).expect("pre-BB reader");
    let state: String = connection
        .query_row(
            "SELECT state FROM tasks WHERE team = ?1 AND task_id = ?2",
            params![team.as_str(), task_id.as_str()],
            |row| row.get(0),
        )
        .expect("frozen task read");
    let events: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM task_events WHERE team = ?1 AND task_id = ?2",
            params![team.as_str(), task_id.as_str()],
            |row| row.get(0),
        )
        .expect("frozen event read");
    assert_eq!(
        state,
        TaskState::Complete(TaskCloseOutcome::Completed).as_str()
    );
    assert_eq!(events, 3);
}

fn task_message(
    team: TeamName,
    recipient: AgentName,
    from: AgentName,
    task_id: TaskId,
    task_op: Option<TaskOp>,
) -> Message {
    let message_id = AtmMessageId::new();
    Message {
        team,
        agent: recipient,
        message_key: MessageKey::from(message_id),
        envelope: MessageEnvelope {
            from,
            source_chat_id: None,
            text: "task transition".to_owned(),
            timestamp: IsoTimestamp::from_datetime(
                DateTime::<Utc>::from_timestamp(1_757_690_854, 0).expect("timestamp"),
            ),
            read: false,
            source_team: None,
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
            task_id: Some(task_id),
            placement: None,
            task_op,
            task_complete: None,
            extra: Map::new(),
        },
    }
}
