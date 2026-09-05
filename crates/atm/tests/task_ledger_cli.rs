//! End-to-end coverage for the task CLI's rusqlite-backed command pipeline.
//!
//! The public CLI delegates its task-list and task-completion requests to the
//! same `atm-core` operations exercised here.  This keeps the integration
//! scenario backend-real while avoiding a host-wide daemon or Tokio test
//! process: the runtime is an isolated rusqlite assembly for each test.

use atm_core::ack::{AckRequest, ack_mail_with_runtime};
use atm_core::boundary::{RosterEntry, RosterHarness, RosterMemberKind};
use atm_core::list::{ListQuery, TaskLedgerQuery, list_task_ledger_with_runtime};
use atm_core::observability::NullObservability;
use atm_core::schema::AtmMessageId;
use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
use atm_core::types::{AgentName, IsoTimestamp, ModelName, ReadSelection, TaskId, TeamName};
use atm_runtime_test_support::open_isolated_sqlite_boundary;
use atm_storage::{RosterSnapshot, TaskEventKind, TaskState};

const ASSIGNER: &str = "fenix";
const ASSIGNEE: &str = "cipher";

struct Fixture {
    root: tempfile::TempDir,
    runtime: atm_core::LocalServiceRuntime,
    team: TeamName,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("task CLI fixture root");
        let assembly = open_isolated_sqlite_boundary(root.path()).expect("rusqlite runtime");
        let team: TeamName = "ax4-task-team".parse().expect("team");
        let members = [ASSIGNER, ASSIGNEE]
            .into_iter()
            .map(|agent| RosterEntry {
                team_name: team.clone(),
                agent_name: agent.parse().expect("agent"),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: atm_core::schema::AgentType::default(),
                model: ModelName::default(),
                recipient_pane_id: None,
                metadata_json: serde_json::Map::new(),
            })
            .collect();
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&RosterSnapshot {
                team_name: team.clone(),
                members,
                refreshed_at: None,
            })
            .expect("seed roster");
        Self {
            root,
            runtime: assembly.service_runtime,
            team,
        }
    }

    fn request(
        &self,
        sender: &str,
        task_id: Option<TaskId>,
        task_complete: Option<TaskId>,
    ) -> WriteRequest {
        let mut request = WriteRequest::new(
            self.root.path().to_path_buf(),
            self.root.path().to_path_buf(),
            sender.parse::<AgentName>().expect("sender"),
            &format!("{ASSIGNEE}@{}", self.team),
            self.team.clone(),
            SendMessageSource::Inline("task CLI integration message".to_owned()),
            None,
            task_id.is_some(),
            task_id,
            false,
        )
        .expect("write request")
        .with_nudge_mode(NudgeMode::Immediate)
        .with_origin_metadata(AtmMessageId::new(), IsoTimestamp::now());
        request.task_complete = task_complete;
        request
    }

    fn assign(&self, task_id: &str) -> AtmMessageId {
        let outcome = write_mail_with_runtime(
            self.request(ASSIGNER, Some(task_id.parse().expect("task id")), None),
            &NullObservability,
            &self.runtime,
        )
        .expect("task assignment");
        outcome.persisted_message_id()
    }

    fn acknowledge(&self, message_id: AtmMessageId) {
        ack_mail_with_runtime(
            AckRequest {
                home_dir: self.root.path().to_path_buf(),
                current_dir: self.root.path().to_path_buf(),
                caller_identity: ASSIGNEE.parse().expect("assignee"),
                caller_chat_id: None,
                caller_team: self.team.clone(),
                activity_observation: None,
                message_id,
                reply_body: "acknowledged".to_owned(),
            },
            &NullObservability,
            &self.runtime,
        )
        .expect("task acknowledgement");
    }

    fn list_query(&self, view: TaskLedgerQuery) -> ListQuery {
        ListQuery::new(
            self.root.path().to_path_buf(),
            self.root.path().to_path_buf(),
            ASSIGNER.parse().expect("assigner"),
            None,
            self.team.clone(),
            ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("list query")
        .with_task_ledger(view)
    }
}

#[test]
fn cli_task_ledger_surfaces_cover_ac1_on_a_real_rusqlite_runtime() {
    let fixture = Fixture::new();
    let first_message = fixture.assign("t-42");
    fixture.acknowledge(first_message);
    let second_message = fixture.assign("t-43");

    let rejected_ack = ack_mail_with_runtime(
        AckRequest {
            home_dir: fixture.root.path().to_path_buf(),
            current_dir: fixture.root.path().to_path_buf(),
            caller_identity: ASSIGNEE.parse().expect("assignee"),
            caller_chat_id: None,
            caller_team: fixture.team.clone(),
            activity_observation: None,
            message_id: second_message,
            reply_body: "blocked".to_owned(),
        },
        &NullObservability,
        &fixture.runtime,
    );
    assert!(
        rejected_ack.is_err(),
        "a second task cannot activate while one is active"
    );

    let tasks = list_task_ledger_with_runtime(
        fixture.list_query(TaskLedgerQuery::Tasks { member: None }),
        &fixture.runtime,
    )
    .expect("atm list --tasks");
    let states: Vec<_> = tasks.task_rows.iter().map(|row| row.state).collect();
    assert_eq!(states, vec![TaskState::Assigned, TaskState::Active]);
    assert!(
        serde_json::to_value(&tasks.task_rows)
            .expect("task rows JSON")
            .is_array()
    );

    let first_events = list_task_ledger_with_runtime(
        fixture.list_query(TaskLedgerQuery::Events {
            task_id: "t-42".parse().expect("task id"),
            member: None,
        }),
        &fixture.runtime,
    )
    .expect("atm list --task-events t-42");
    assert_eq!(
        first_events
            .task_event_rows
            .iter()
            .map(|row| row.event)
            .collect::<Vec<_>>(),
        vec![TaskEventKind::Assigned, TaskEventKind::Acked]
    );

    let second_events = list_task_ledger_with_runtime(
        fixture.list_query(TaskLedgerQuery::Events {
            task_id: "t-43".parse().expect("task id"),
            member: None,
        }),
        &fixture.runtime,
    )
    .expect("rejected task events");
    assert_eq!(
        second_events.task_event_rows[1].event,
        TaskEventKind::Rejected
    );
    assert!(
        second_events.task_event_rows[1]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("t-42"))
    );
}

#[test]
fn cli_task_completion_covers_ac2_success_unknown_id_and_conflict() {
    let fixture = Fixture::new();
    let task_id: TaskId = "t-42".parse().expect("task id");
    fixture.assign("t-42");

    write_mail_with_runtime(
        fixture.request(ASSIGNER, None, Some(task_id.clone())),
        &NullObservability,
        &fixture.runtime,
    )
    .expect("atm send --task-complete t-42");
    let row = fixture
        .runtime
        .task_store()
        .expect("task store")
        .list_tasks(&fixture.team, None)
        .expect("task rows")
        .into_iter()
        .find(|row| row.task_id == task_id)
        .expect("completed task");
    assert_eq!(row.state, TaskState::Complete);

    let before = fixture
        .runtime
        .task_store()
        .expect("task store")
        .list_tasks(&fixture.team, None)
        .expect("task rows")
        .len();
    let unknown = fixture.request(
        ASSIGNER,
        None,
        Some("missing-task".parse().expect("task id")),
    );
    let error = write_mail_with_runtime(unknown, &NullObservability, &fixture.runtime)
        .expect_err("unknown completion must fail");
    assert!(error.message().contains("no open task"));
    assert_eq!(
        fixture
            .runtime
            .task_store()
            .expect("task store")
            .list_tasks(&fixture.team, None)
            .expect("task rows")
            .len(),
        before
    );

    let mut conflict = fixture.request(
        ASSIGNER,
        Some("new-task".parse().expect("task id")),
        Some("t-42".parse().expect("task id")),
    );
    conflict.task_complete = Some(task_id);
    let conflict_error = write_mail_with_runtime(conflict, &NullObservability, &fixture.runtime)
        .expect_err("task assignment and completion must conflict");
    assert!(
        conflict_error
            .message()
            .contains("assign and complete a task")
    );
}
