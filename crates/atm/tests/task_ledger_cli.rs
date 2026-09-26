//! End-to-end coverage for the task CLI's rusqlite-backed command pipeline.
//!
//! The public CLI delegates its task-list and task-completion requests to the
//! same `atm-core` operations exercised here. This keeps the integration
//! scenario backend-real while using the production bounded async reader lane
//! over an isolated rusqlite assembly for each test.

use std::sync::Arc;
use std::time::Duration;

use atm_core::ack::{AckRequest, ack_mail_with_runtime};
use atm_core::boundary::{RosterEntry, RosterHarness, RosterMemberKind};
use atm_core::list::{ListQuery, TaskLedgerQuery, list_task_ledger_with_runtime_async};
use atm_core::observability::NullObservability;
use atm_core::schema::AtmMessageId;
use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
use atm_core::task_query::MAX_TASK_PAGE_LIMIT;
use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER};
use atm_core::types::{AgentName, IsoTimestamp, ModelName, ReadSelection, TaskId, TeamName};
use atm_runtime_test_support::open_isolated_sqlite_boundary;
use atm_storage::testing::InMemoryTaskLedgerReader;
use atm_storage::{QueuePosition, ReadDeadline, RosterSnapshot, TaskEventKind, TaskRow, TaskState};

const ASSIGNER: &str = TEST_SENDER;
const ASSIGNEE: &str = TEST_RECIPIENT;

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

#[tokio::test]
async fn cli_task_ledger_surfaces_cover_ac1_on_a_real_rusqlite_runtime() {
    let fixture = Fixture::new();
    let first_message = fixture.assign("t-42");
    fixture.acknowledge(first_message);
    let second_message = fixture.assign("t-43");

    fixture.acknowledge(second_message);

    let deadline = ReadDeadline::new(Duration::from_secs(5)).expect("read deadline");
    let tasks = list_task_ledger_with_runtime_async(
        fixture.list_query(TaskLedgerQuery::Tasks { member: None }),
        &fixture.runtime,
        deadline,
    )
    .await
    .expect("atm list --tasks");
    let states: Vec<_> = tasks.task_rows.iter().map(|row| row.state).collect();
    assert_eq!(states, vec![TaskState::Assigned, TaskState::Assigned]);
    assert!(
        serde_json::to_value(&tasks.task_rows)
            .expect("task rows JSON")
            .is_array()
    );

    let first_events = list_task_ledger_with_runtime_async(
        fixture.list_query(TaskLedgerQuery::Events {
            task_id: "t-42".parse().expect("task id"),
            member: None,
        }),
        &fixture.runtime,
        deadline,
    )
    .await
    .expect("atm list --task-events t-42");
    assert_eq!(
        first_events
            .task_event_rows
            .iter()
            .map(|row| row.event)
            .collect::<Vec<_>>(),
        vec![TaskEventKind::Assigned]
    );

    let second_events = list_task_ledger_with_runtime_async(
        fixture.list_query(TaskLedgerQuery::Events {
            task_id: "t-43".parse().expect("task id"),
            member: None,
        }),
        &fixture.runtime,
        deadline,
    )
    .await
    .expect("second task events");
    assert_eq!(second_events.task_event_rows.len(), 1);
    assert!(second_events.task_event_rows[0].event == TaskEventKind::Assigned);
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
    assert_eq!(
        row.state,
        TaskState::Complete(atm_storage::TaskCloseOutcome::Completed)
    );

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
            .contains("task_id and task_complete name different tasks")
    );
}

fn history_task_row(team: &TeamName, task_id: &str) -> TaskRow {
    TaskRow {
        team: team.clone(),
        task_id: task_id.parse().expect("task id"),
        assignee: ASSIGNEE.parse().expect("assignee"),
        assigner: ASSIGNER.parse().expect("assigner"),
        state: TaskState::Assigned,
        position: Some(QueuePosition::new(1).expect("position")),
        assignment_message_id: AtmMessageId::new(),
        description: task_id.to_owned(),
        assigned_at: IsoTimestamp::now(),
        updated_at: IsoTimestamp::now(),
        last_reminded_at: None,
        reminder_count: 0,
        lead_notified_count: 0,
    }
}

/// QA-CONV-001: `atm task history --limit` is validated CLI-side by
/// `TaskHistoryCommand::resolved_limit`, but a `TaskLedgerQuery::History`
/// request can also be built directly by a caller that skips the CLI (a
/// future peer or graft caller). This proves the same bound is enforced at
/// the daemon/list layer the request actually reaches.
///
/// No-Claim: this does not prove every future construction path for a
/// `History` request goes through `list_task_ledger_with_runtime_async`;
/// it only proves that when it does, an invalid limit is rejected there.
#[tokio::test]
async fn direct_history_request_rejects_limit_zero_and_oversized_limit_at_the_list_layer() {
    let fixture = Fixture::new();
    let deadline = ReadDeadline::new(Duration::from_secs(5)).expect("read deadline");

    let zero_error = list_task_ledger_with_runtime_async(
        fixture.list_query(TaskLedgerQuery::History {
            member: None,
            limit: 0,
        }),
        &fixture.runtime,
        deadline,
    )
    .await
    .expect_err("limit 0 must be rejected at the daemon/list layer, not only the CLI");
    assert!(zero_error.message().contains("task limit"));

    let oversized_error = list_task_ledger_with_runtime_async(
        fixture.list_query(TaskLedgerQuery::History {
            member: None,
            limit: MAX_TASK_PAGE_LIMIT + 1,
        }),
        &fixture.runtime,
        deadline,
    )
    .await
    .expect_err("a limit above MAX_TASK_PAGE_LIMIT must be rejected at the daemon/list layer");
    assert!(oversized_error.message().contains("task limit"));
}

/// QA-CONV-002: `TaskHistoryCommand::execute` used to fetch each returned
/// row's events with its own ledger read (an N+1). The fix batches every
/// row's events into the one `list_task_events_for_tasks` read the History
/// arm of `list_task_ledger_with_runtime_async` now issues.
///
/// No-Claim: this proves the round trip count into the ledger reader stays
/// flat as row count grows; it does not measure wall-clock latency or
/// exercise the CLI's own `--events`/table rendering path.
#[tokio::test]
async fn history_round_trip_count_does_not_scale_with_row_count() {
    let fixture = Fixture::new();
    let deadline = ReadDeadline::new(Duration::from_secs(5)).expect("read deadline");

    for size in [3_usize, 30_usize] {
        let tasks = (0..size)
            .map(|index| history_task_row(&fixture.team, &format!("t-{index}")))
            .collect::<Vec<_>>();
        let reader = Arc::new(InMemoryTaskLedgerReader::with_rows(tasks, Vec::new()));
        let runtime = fixture
            .runtime
            .clone()
            .with_async_task_ledger_reader(reader.clone());

        let outcome = list_task_ledger_with_runtime_async(
            fixture.list_query(TaskLedgerQuery::History {
                member: None,
                limit: size,
            }),
            &runtime,
            deadline,
        )
        .await
        .expect("history request");

        assert_eq!(outcome.task_rows.len(), size);
        assert_eq!(
            reader.event_batch_call_count(),
            1,
            "one batched events call regardless of row count ({size} rows)"
        );
    }
}
