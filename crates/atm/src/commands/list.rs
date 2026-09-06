use anyhow::Result;
use atm_core::list::{ListQuery, TaskLedgerQuery};
use clap::Args;

use crate::commands::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride, resolve_cli_caller_context,
};
use crate::commands::task_ledger::print_task_ledger;
use crate::commands::util::parse_timestamp;
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// List one ATM mailbox surface as bounded metadata rows.
pub struct ListCommand {
    #[arg(conflicts_with_all = ["tasks", "task_events"])]
    target: Option<String>,

    #[arg(long)]
    team: Option<String>,

    #[arg(long, conflicts_with_all = ["unread", "pending_ack"])]
    all: bool,

    #[arg(long, conflicts_with = "pending_ack")]
    unread: bool,

    #[arg(long = "pending-ack", conflicts_with = "unread")]
    pending_ack: bool,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long)]
    since: Option<String>,

    #[arg(long)]
    from: Option<String>,

    #[arg(long)]
    task: Option<String>,

    #[arg(long)]
    contains: Option<String>,

    /// List the durable task ledger for the selected team.
    #[arg(
        long,
        conflicts_with_all = ["target", "all", "unread", "pending_ack", "limit", "since", "from", "task", "contains", "task_events"]
    )]
    tasks: bool,

    /// List the append-only audit events for one task identifier.
    #[arg(
        long = "task-events",
        value_name = "TASK_ID",
        conflicts_with_all = ["target", "all", "unread", "pending_ack", "limit", "since", "from", "task", "contains", "tasks"]
    )]
    task_events: Option<atm_core::types::TaskId>,

    /// Restrict a task-ledger view to one assignee.
    #[arg(long, value_name = "NAME")]
    member: Option<atm_core::types::AgentName>,

    #[arg(long)]
    json: bool,

    #[arg(long = "as")]
    actor: Option<String>,
}

impl ListCommand {
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("list")?;
        let json = self.json;
        let query = self.build_query(home_dir.clone(), current_dir.clone())?;
        let task_ledger = query.task_ledger.clone();
        let composition = CliComposition::bootstrap(
            "list",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        let outcome = composition.list(query).await?;
        match task_ledger {
            Some(task_ledger) => print_task_ledger(&outcome, &task_ledger, json),
            None => output::print_list_result(&outcome, json),
        }
    }

    fn build_query(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<ListQuery> {
        let caller_context = resolve_cli_caller_context(CallerContextOverrides {
            identity_override: self.actor.as_deref().map(CallerIdentityOverride),
            chat_id_override: None,
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        let selection_mode = self.selection_mode();
        let timestamp_filter = self.since.as_deref().map(parse_timestamp).transpose()?;
        let query = ListQuery::new(
            home_dir,
            current_dir,
            caller_context.caller_identity,
            self.target.as_deref(),
            caller_context.caller_team,
            selection_mode,
            selection_mode != atm_core::types::ReadSelection::All,
            self.limit,
            self.from.as_deref(),
            timestamp_filter,
            self.task.as_deref(),
            self.contains.as_deref(),
        )?;
        match self.task_ledger()? {
            Some(task_ledger) => Ok(query.with_task_ledger(task_ledger)),
            None => Ok(query),
        }
    }

    fn selection_mode(&self) -> atm_core::types::ReadSelection {
        if self.all {
            atm_core::types::ReadSelection::All
        } else if self.unread {
            atm_core::types::ReadSelection::Unread
        } else if self.pending_ack {
            atm_core::types::ReadSelection::PendingAck
        } else {
            atm_core::types::ReadSelection::Actionable
        }
    }

    fn task_ledger(&self) -> Result<Option<TaskLedgerQuery>> {
        if self.tasks {
            Ok(Some(TaskLedgerQuery::Tasks {
                member: self.member.clone(),
            }))
        } else {
            let task_ledger = self
                .task_events
                .clone()
                .map(|task_id| TaskLedgerQuery::Events {
                    task_id,
                    member: self.member.clone(),
                });
            if self.member.is_some() && task_ledger.is_none() {
                return Err(atm_core::error::AtmError::validation(
                    "--member requires --tasks or --task-events <TASK_ID>",
                )
                .into());
            }
            Ok(task_ledger)
        }
    }
}

#[cfg(test)]
mod tests {
    use atm_core::list::{ListOutcome, TaskLedgerQuery};
    use atm_core::read::BucketCounts;
    use atm_core::test_support::EnvGuard;
    use atm_core::test_support::{ROLE_TEAM_LEAD, TEST_TEAM};
    use atm_core::types::{CommandAction, IsoTimestamp, ReadSelection};
    use atm_storage::{AtmMessageId, TaskActor, TaskEventKind, TaskEventRow, TaskRow, TaskState};
    use clap::Parser;
    use serial_test::serial;

    use super::ListCommand;
    use crate::commands::task_ledger::render_task_ledger;

    #[test]
    fn selection_mode_maps_cli_flags_to_expected_bucket() {
        let mut command = base_command();
        command.all = true;
        assert_eq!(command.selection_mode(), ReadSelection::All);

        let mut command = base_command();
        command.unread = true;
        assert_eq!(command.selection_mode(), ReadSelection::Unread);

        let mut command = base_command();
        command.pending_ack = true;
        assert_eq!(command.selection_mode(), ReadSelection::PendingAck);

        let command = base_command();
        assert_eq!(command.selection_mode(), ReadSelection::Actionable);
    }

    #[test]
    fn build_query_preserves_limit_and_filters() {
        let mut command = base_command();
        command.target = Some("recipient-a@test-team".to_string());
        command.team = Some("override-team".to_string());
        command.actor = Some(ROLE_TEAM_LEAD.to_string());
        command.limit = Some(12);
        command.task = Some("TASK-22".to_string());
        command.contains = Some("needle".to_string());

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(query.selection_mode, ReadSelection::Actionable);
        assert_eq!(query.limit, Some(12));
        assert_eq!(
            query.task_filter.as_ref().map(|value| value.as_str()),
            Some("TASK-22")
        );
        assert_eq!(query.contains_filter.as_deref(), Some("needle"));
    }

    #[test]
    #[serial(env)]
    fn build_query_uses_environment_when_overrides_are_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a")),
            ("ATM_TEAM", Some("env-team")),
        ]);

        let query = base_command()
            .build_query(".".into(), ".".into())
            .expect("query");

        assert_eq!(query.caller_identity.as_str(), "sender-a");
        assert_eq!(query.caller_team.as_str(), "env-team");
    }

    #[test]
    #[serial(env)]
    fn build_query_prefers_cli_overrides_over_environment() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("env-sender")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let mut command = base_command();
        command.team = Some("override-team".to_string());
        command.actor = Some(ROLE_TEAM_LEAD.to_string());

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(query.caller_identity.as_str(), ROLE_TEAM_LEAD);
        assert_eq!(query.caller_team.as_str(), "override-team");
    }

    #[test]
    #[serial(env)]
    fn build_query_accepts_as_without_ambient_identity() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", None),
            ("ATM_CHAT_ID", None),
            ("ATM_TEAM", None),
        ]);
        let mut command = base_command();
        command.actor = Some(ROLE_TEAM_LEAD.to_string());
        command.team = Some(TEST_TEAM.to_string());

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(query.caller_identity.as_str(), ROLE_TEAM_LEAD);
        assert_eq!(query.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    fn cli_rejects_task_ledger_flags_with_mailbox_filters() {
        for arguments in [
            vec!["atm", "list", "--tasks", "--task", "t-1"],
            vec!["atm", "list", "--task-events", "t-1", "--unread"],
            vec!["atm", "list", "recipient@test-team", "--tasks"],
        ] {
            crate::commands::Cli::try_parse_from(arguments)
                .expect_err("task-ledger views must reject mailbox filters");
        }
    }

    #[test]
    #[serial(env)]
    fn task_ledger_query_preserves_member_and_event_identifier() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a")),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let mut command = base_command();
        command.task_events = Some("t-42".parse().expect("task id"));
        command.member = Some("cipher".parse().expect("member"));

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert!(matches!(
            query.task_ledger,
            Some(TaskLedgerQuery::Events { task_id, member: Some(member) })
                if task_id.as_str() == "t-42" && member.as_str() == "cipher"
        ));
    }

    #[test]
    fn member_requires_a_task_ledger_view() {
        let mut command = base_command();
        command.member = Some("cipher".parse().expect("member"));
        assert!(command.task_ledger().is_err());
    }

    #[test]
    fn task_ledger_rendering_matches_human_and_json_contracts() {
        let row = task_row();
        let event = TaskEventRow {
            team: TEST_TEAM.parse().expect("team"),
            task_id: "t-42".parse().expect("task"),
            assignee: "cipher".parse().expect("assignee"),
            seq: 1,
            at: timestamp(),
            event: TaskEventKind::Assigned,
            from_state: None,
            to_state: Some(TaskState::Assigned),
            actor: TaskActor::Member("fenix".parse().expect("actor")),
            message_id: Some(AtmMessageId::new()),
            outcome: None,
            marker: None,
            detail: None,
        };
        let outcome = task_outcome(vec![row], vec![event]);

        let tasks = render_task_ledger(&outcome, &TaskLedgerQuery::Tasks { member: None }, false)
            .expect("human tasks");
        assert_eq!(
            tasks,
            "TASK_ID     STATE     ASSIGNEE  ASSIGNER   ASSIGNED_AT               REMINDERS\n\
t-42        active    cipher    fenix      2026-09-05T10:12:03Z      3\n"
        );

        let events = render_task_ledger(
            &outcome,
            &TaskLedgerQuery::Events {
                task_id: "t-42".parse().expect("task"),
                member: None,
            },
            false,
        )
        .expect("human task events");
        assert_eq!(
            events,
            "SEQ  AT                        EVENT      FROM      TO        ACTOR    DETAIL\n\
1    2026-09-05T10:12:03Z      assigned   -         assigned  fenix    -\n"
        );

        let json = render_task_ledger(&outcome, &TaskLedgerQuery::Tasks { member: None }, true)
            .expect("JSON tasks");
        let json: serde_json::Value = serde_json::from_str(&json).expect("task row array");
        assert!(json.is_array());
        assert_eq!(json[0]["task_id"], "t-42");
        assert_eq!(json[0]["state"], "active");

        let event_json = render_task_ledger(
            &outcome,
            &TaskLedgerQuery::Events {
                task_id: "t-42".parse().expect("task"),
                member: None,
            },
            true,
        )
        .expect("JSON task events");
        let event_json: serde_json::Value =
            serde_json::from_str(&event_json).expect("task event row array");
        assert!(event_json.is_array());
        assert_eq!(event_json[0]["event"], "assigned");
    }

    fn timestamp() -> IsoTimestamp {
        "2026-09-05T10:12:03Z".parse().expect("timestamp")
    }

    fn task_row() -> TaskRow {
        TaskRow {
            team: TEST_TEAM.parse().expect("team"),
            task_id: "t-42".parse().expect("task"),
            assignee: "cipher".parse().expect("assignee"),
            assigner: "fenix".parse().expect("assigner"),
            state: TaskState::Active,
            assignment_message_id: AtmMessageId::new(),
            description: "finish AX4".to_string(),
            assigned_at: timestamp(),
            updated_at: timestamp(),
            last_reminded_at: None,
            reminder_count: 3,
            lead_notified_count: 0,
        }
    }

    fn task_outcome(task_rows: Vec<TaskRow>, task_event_rows: Vec<TaskEventRow>) -> ListOutcome {
        ListOutcome {
            action: CommandAction::List,
            team: TEST_TEAM.parse().expect("team"),
            agent: "cipher".parse().expect("agent"),
            selection_mode: ReadSelection::Actionable,
            history_collapsed: false,
            count: task_rows.len() + task_event_rows.len(),
            rows: Vec::new(),
            bucket_counts: BucketCounts {
                unread: 0,
                pending_ack: 0,
                history: 0,
            },
            task_rows,
            task_event_rows,
        }
    }

    fn base_command() -> ListCommand {
        ListCommand {
            target: None,
            team: None,
            all: false,
            unread: false,
            pending_ack: false,
            limit: None,
            since: None,
            from: None,
            task: None,
            contains: None,
            tasks: false,
            task_events: None,
            member: None,
            json: false,
            actor: None,
        }
    }
}
