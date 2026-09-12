//! Closed task command surface.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::Result;
use atm_core::address::AgentAddress;
use atm_core::doctor::DoctorQuery;
use atm_core::list::{ListQuery, TaskLedgerQuery};
use atm_core::protocol::{
    HttpApiVersion, RequestEnvelope, ResponseEnvelope, RuntimeStatusSnapshot, TaskMoveRequest,
};
use atm_core::send::NudgeMode;
use atm_core::task_close::{ClosePreflight, preflight_close, report_recipient};
use atm_core::task_query::{
    TaskEventQuery, TaskListQuery, TaskPage, select_task_events, select_task_rows,
};
use atm_core::types::{AgentName, TaskId, TeamName};
use atm_storage::{
    DAEMON_ACTOR_NAME, MoveTarget, RuntimeMemberState, TaskActor, TaskCloseOutcome, TaskEventRow,
    TaskRow,
};
use chrono::SecondsFormat;
use clap::{ArgGroup, Args, Subcommand, ValueEnum};

use crate::commands::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride, resolve_cli_caller_context,
};
use crate::commands::send::{SendCommand, TaskSendOptions, preflight_daemon_api};
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;

#[derive(Debug, Args)]
pub struct TaskCommand {
    #[command(subcommand)]
    command: TaskSubcommand,
}

#[derive(Debug, Subcommand)]
enum TaskSubcommand {
    /// Queue view: the caller's open tasks; --all includes every member.
    List(TaskListCommand),
    /// Append-only event history of one task.
    Events(TaskEventsCommand),
    /// Assign a task with deferred notification.
    Assign(TaskAssignCommand),
    /// Deliver a report and close a task with a typed outcome.
    Close(TaskCloseCommand),
    /// Reorder one member's queue.
    Move(TaskMoveCommand),
}

#[derive(Debug, Args)]
struct TaskListCommand {
    #[arg(long)]
    all: bool,
    #[arg(long, value_name = "N", conflicts_with = "all")]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

#[derive(Debug, Args)]
struct TaskEventsCommand {
    task_id: TaskId,
    #[arg(long, value_name = "N", conflicts_with = "all")]
    limit: Option<usize>,
    #[arg(long)]
    all: bool,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

#[derive(Debug, Args)]
struct TaskAssignCommand {
    assignee: AgentAddress,
    #[arg(long = "task-id")]
    task_id: Option<TaskId>,
    #[arg(long, value_name = "OTHER_TASK_ID", group = "placement")]
    before: Option<TaskId>,
    #[arg(long, group = "placement")]
    head: bool,
    #[command(flatten)]
    message: MessageSourceArgs,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

impl TaskAssignCommand {
    fn placement(&self) -> Option<MoveTarget> {
        match (&self.before, self.head) {
            (Some(task_id), false) => Some(MoveTarget::Before {
                task_id: task_id.clone(),
            }),
            (None, true) => Some(MoveTarget::Head),
            (None, false) => None,
            (Some(_), true) => unreachable!("clap placement group"),
        }
    }
}

#[derive(Debug, Args)]
struct TaskCloseCommand {
    task_id: TaskId,
    #[arg(value_enum)]
    outcome: OutcomeArg,
    /// Recorded on the close event; also the report body when no source is given.
    reason: Option<String>,
    #[command(flatten)]
    report: MessageSourceArgs,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

impl TaskCloseCommand {
    fn validate(&self) -> Result<(), atm_core::error::AtmError> {
        if self.report.is_present() || self.reason.is_some() {
            Ok(())
        } else {
            Err(atm_core::error::AtmError::validation(
                "a close <id> <outcome> needs a reason or a report source",
            ))
        }
    }
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("target")
        .required(true)
        .multiple(false)
        .args(["head", "end", "before"])
))]
struct TaskMoveCommand {
    task_id: TaskId,
    #[arg(long)]
    head: bool,
    #[arg(long)]
    end: bool,
    #[arg(long, value_name = "OTHER_TASK_ID")]
    before: Option<TaskId>,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
enum OutcomeArg {
    Completed,
    Refused,
    Cancelled,
}

impl From<OutcomeArg> for TaskCloseOutcome {
    fn from(value: OutcomeArg) -> Self {
        match value {
            OutcomeArg::Completed => Self::Completed,
            OutcomeArg::Refused => Self::Refused,
            OutcomeArg::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Args)]
struct CallerArgs {
    #[arg(long = "as")]
    actor: Option<String>,
    #[arg(long)]
    team: Option<String>,
}

#[derive(Debug, Args)]
struct MessageSourceArgs {
    #[arg(value_name = "MESSAGE", conflicts_with_all = ["file", "stdin", "template"])]
    text: Option<String>,
    #[arg(long, conflicts_with_all = ["text", "stdin", "template"])]
    file: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["text", "file", "template"])]
    stdin: bool,
    #[arg(long, conflicts_with_all = ["text", "file", "stdin"])]
    template: Option<PathBuf>,
    #[arg(long, requires = "template")]
    vars: Option<String>,
}

impl MessageSourceArgs {
    fn is_present(&self) -> bool {
        self.text.is_some() || self.file.is_some() || self.stdin || self.template.is_some()
    }

    fn into_send_options(
        self,
        to: String,
        caller: CallerArgs,
        task_id: Option<TaskId>,
        json: bool,
    ) -> TaskSendOptions {
        TaskSendOptions {
            to,
            message: self.text,
            team: caller.team,
            actor: caller.actor,
            file: self.file,
            stdin: self.stdin,
            template: self.template,
            vars: self.vars,
            task_id,
            json,
        }
    }
}

impl TaskCommand {
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        match self.command {
            TaskSubcommand::List(command) => command.run(observability).await,
            TaskSubcommand::Events(command) => command.run(observability).await,
            TaskSubcommand::Assign(command) => command.run(observability).await,
            TaskSubcommand::Close(command) => command.run(observability).await,
            TaskSubcommand::Move(command) => command.run(observability).await,
        }
    }
}

impl TaskAssignCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("task assign")?;
        let composition = composition("task assign", observability, &home_dir, &current_dir)?;
        print!(
            "{}",
            self.execute(&composition, home_dir, current_dir).await?
        );
        Ok(())
    }

    async fn execute(
        self,
        composition: &CliComposition<'_>,
        home_dir: PathBuf,
        current_dir: PathBuf,
    ) -> Result<String> {
        let task_id = resolve_task_id(self.task_id.clone())?;
        let placement = self.placement();
        let json = self.json;
        let assignee = self.assignee.to_string();
        let mut request = SendCommand::for_task(self.message.into_send_options(
            assignee.clone(),
            self.caller,
            Some(task_id.clone()),
            json,
        ))
        .build_request_with_mode(
            home_dir.clone(),
            current_dir.clone(),
            NudgeMode::Deferred,
            None,
        )?;
        request.placement = placement;
        preflight_daemon_api(composition, HttpApiVersion::parse("1.5.0")?, "task assign").await?;
        composition.send(request).await?;
        if json {
            Ok(format!("{}\n", serde_json::json!({ "task_id": task_id })))
        } else {
            Ok(format!("assigned {task_id} to {assignee}\n"))
        }
    }
}

impl TaskCloseCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        self.validate()?;
        let (home_dir, current_dir) = resolve_command_runtime_context("task close")?;
        let caller = resolve_context(&self.caller)?;
        let composition = composition("task close", observability, &home_dir, &current_dir)?;
        print!(
            "{}",
            self.execute(&composition, caller, home_dir, current_dir)
                .await?
        );
        Ok(())
    }

    async fn execute(
        self,
        composition: &CliComposition<'_>,
        caller: atm_core::caller_context::CallerContext,
        home_dir: PathBuf,
        current_dir: PathBuf,
    ) -> Result<String> {
        preflight_daemon_api(composition, HttpApiVersion::parse("1.5.0")?, "task close").await?;
        let query = task_list_request(
            home_dir.clone(),
            current_dir.clone(),
            caller.caller_identity.clone(),
            caller.caller_team.clone(),
            None,
            Some(&self.task_id),
        )?;
        let rows = composition.list(query).await?.task_rows;
        let row = match preflight_close(rows, &self.task_id) {
            ClosePreflight::Proceed { row } => row,
            ClosePreflight::Unknown => {
                return Err(anyhow::anyhow!(
                    "task {} does not exist on team {}",
                    self.task_id,
                    caller.caller_team
                ));
            }
        };
        let recipient = report_recipient(&row, &caller.caller_identity);
        let outcome: TaskCloseOutcome = self.outcome.into();
        let reason = self.reason.clone();
        let report = if self.report.is_present() {
            self.report
        } else {
            MessageSourceArgs {
                text: self.reason,
                file: None,
                stdin: false,
                template: None,
                vars: None,
            }
        };
        let mut request = SendCommand::for_task(report.into_send_options(
            recipient.to_string(),
            self.caller,
            None,
            self.json,
        ))
        .build_task_close_request(
            home_dir.clone(),
            current_dir.clone(),
            self.task_id.clone(),
            outcome,
            reason,
        )?;
        atm_core::send::validate_task_request(&mut request)?;
        let result = composition
            .send(request)
            .await
            .map_err(anyhow::Error::from)?;
        if self.json {
            Ok(format!("{}\n", serde_json::to_string_pretty(&result)?))
        } else if let Some(closed) = result.already_closed {
            Ok(format!(
                "task {} was already closed ({}); report delivered",
                self.task_id,
                closed.as_str()
            ) + "\n")
        } else {
            Ok(format!("closed {} ({})\n", self.task_id, outcome.as_str()))
        }
    }
}

impl TaskListCommand {
    fn page(&self) -> Result<TaskPage, atm_core::error::AtmError> {
        if self.all {
            Ok(TaskPage::All)
        } else {
            self.limit
                .map_or_else(|| Ok(TaskPage::default_bounded()), TaskPage::bounded)
        }
    }

    async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("task list")?;
        let caller = resolve_context(&self.caller)?;
        let composition = composition("task list", observability, &home_dir, &current_dir)?;
        print!(
            "{}",
            self.execute(&composition, caller, home_dir, current_dir)
                .await?
        );
        Ok(())
    }

    async fn execute(
        self,
        composition: &CliComposition<'_>,
        caller: atm_core::caller_context::CallerContext,
        home_dir: PathBuf,
        current_dir: PathBuf,
    ) -> Result<String> {
        let contract = TaskListQuery {
            team: caller.caller_team.clone(),
            assignee: (!self.all).then_some(caller.caller_identity.clone()),
            page: self.page()?,
        };
        preflight_daemon_api(composition, HttpApiVersion::parse("1.5.0")?, "task list").await?;
        let outcome = composition
            .list(task_list_request(
                home_dir.clone(),
                current_dir.clone(),
                caller.caller_identity.clone(),
                contract.team.clone(),
                contract.assignee.clone(),
                None,
            )?)
            .await?;
        let selected = select_task_rows(outcome.task_rows, &contract);
        let runtime = if self.all {
            composition
                .doctor(DoctorQuery {
                    home_dir,
                    current_dir,
                    team_override: Some(contract.team.clone()),
                    all_teams: false,
                    caller_team: Some(contract.team),
                    caller_identity: Some(caller.caller_identity),
                })
                .await?
                .runtime_status
        } else {
            None
        };
        let output =
            render_task_rows_output(&selected.rows, self.json, self.all, runtime.as_ref())?;
        print_omitted_rows(selected.omitted);
        Ok(output)
    }
}

impl TaskEventsCommand {
    fn page(&self) -> Result<TaskPage, atm_core::error::AtmError> {
        if self.all {
            Ok(TaskPage::All)
        } else {
            self.limit
                .map_or_else(|| Ok(TaskPage::default_bounded()), TaskPage::bounded)
        }
    }

    async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("task events")?;
        let caller = resolve_context(&self.caller)?;
        let composition = composition("task events", observability, &home_dir, &current_dir)?;
        print!(
            "{}",
            self.execute(&composition, caller, home_dir, current_dir)
                .await?
        );
        Ok(())
    }

    async fn execute(
        self,
        composition: &CliComposition<'_>,
        caller: atm_core::caller_context::CallerContext,
        home_dir: PathBuf,
        current_dir: PathBuf,
    ) -> Result<String> {
        let contract = TaskEventQuery {
            team: caller.caller_team.clone(),
            task_id: self.task_id.clone(),
            assignee: None,
            page: self.page()?,
        };
        preflight_daemon_api(composition, HttpApiVersion::parse("1.5.0")?, "task events").await?;
        let ledger = TaskLedgerQuery::Events {
            task_id: contract.task_id.clone(),
            member: None,
        };
        let query = ListQuery::new(
            home_dir.clone(),
            current_dir.clone(),
            caller.caller_identity,
            None,
            contract.team.clone(),
            atm_core::types::ReadSelection::Actionable,
            false,
            None,
            None,
            None,
            None,
            None,
        )?
        .with_task_ledger(ledger.clone());
        let outcome = composition.list(query).await?;
        let selected = select_task_events(outcome.task_event_rows, &contract);
        let output = render_task_events(&selected.rows, self.json)?;
        print_omitted_rows(selected.omitted);
        Ok(output)
    }
}

fn print_omitted_rows(omitted: usize) {
    if omitted > 0 {
        eprintln!("{omitted} more rows omitted (--all)");
    }
}

impl TaskMoveCommand {
    fn target(&self) -> MoveTarget {
        match (&self.before, self.head, self.end) {
            (None, true, false) => MoveTarget::Head,
            (None, false, true) => MoveTarget::End,
            (Some(task_id), false, false) => MoveTarget::Before {
                task_id: task_id.clone(),
            },
            _ => unreachable!("clap target group"),
        }
    }

    async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("task move")?;
        let caller = resolve_context(&self.caller)?;
        let composition = composition("task move", observability, &home_dir, &current_dir)?;
        print!("{}", self.execute(&composition, caller).await?);
        Ok(())
    }

    async fn execute(
        self,
        composition: &CliComposition<'_>,
        caller: atm_core::caller_context::CallerContext,
    ) -> Result<String> {
        let target = self.target();
        preflight_daemon_api(composition, HttpApiVersion::parse("1.6.0")?, "task move").await?;
        let response = composition
            .execute_request(RequestEnvelope::TaskMove(TaskMoveRequest {
                caller_identity: caller.caller_identity,
                caller_team: caller.caller_team,
                task_id: self.task_id,
                target,
            }))
            .await?;
        match response {
            ResponseEnvelope::TaskMove(outcome) if self.json => {
                Ok(format!("{}\n", serde_json::to_string_pretty(&outcome)?))
            }
            ResponseEnvelope::TaskMove(outcome) => Ok(format!(
                "moved {} for {} from {} to {}",
                outcome.task_id,
                outcome.assignee,
                outcome.from.get(),
                outcome.to.get()
            ) + "\n"),
            other => Err(atm_daemon_client::unexpected_response("task move", other).into()),
        }
    }
}

fn resolve_context(
    caller: &CallerArgs,
) -> Result<atm_core::caller_context::CallerContext, atm_core::error::AtmError> {
    resolve_cli_caller_context(CallerContextOverrides {
        identity_override: caller.actor.as_deref().map(CallerIdentityOverride),
        chat_id_override: None,
        team_override: caller.team.as_deref().map(CallerTeamOverride),
    })
}

fn composition<'a>(
    command: &'static str,
    observability: &'a CliObservability,
    home_dir: &Path,
    current_dir: &Path,
) -> Result<CliComposition<'a>, atm_core::error::AtmError> {
    CliComposition::bootstrap(
        command,
        observability,
        InvocationDir::new(current_dir),
        AtmHomePath::new(home_dir),
    )
}

fn task_list_request(
    home_dir: PathBuf,
    current_dir: PathBuf,
    caller_identity: atm_core::types::AgentName,
    caller_team: TeamName,
    member: Option<atm_core::types::AgentName>,
    task_id: Option<&TaskId>,
) -> Result<ListQuery, atm_core::error::AtmError> {
    Ok(ListQuery::new(
        home_dir,
        current_dir,
        caller_identity,
        None,
        caller_team,
        atm_core::types::ReadSelection::Actionable,
        false,
        None,
        None,
        None,
        task_id.map(TaskId::as_str),
        None,
    )?
    .with_task_ledger(TaskLedgerQuery::Tasks { member }))
}

fn resolve_task_id(task_id: Option<TaskId>) -> Result<TaskId, atm_core::error::AtmError> {
    task_id.map_or_else(|| TaskId::from_str(&ulid::Ulid::new().to_string()), Ok)
}

fn render_task_rows_output(
    rows: &[TaskRow],
    json: bool,
    grouped: bool,
    runtime: Option<&RuntimeStatusSnapshot>,
) -> Result<String> {
    if json {
        return Ok(format!("{}\n", serde_json::to_string_pretty(rows)?));
    }
    Ok(render_task_rows(rows, grouped, runtime))
}

fn render_task_rows(
    rows: &[TaskRow],
    grouped: bool,
    runtime: Option<&RuntimeStatusSnapshot>,
) -> String {
    let mut output = String::new();
    let mut current_member = None;
    for row in rows {
        if grouped && current_member.as_ref() != Some(&row.assignee) {
            if current_member.is_some() {
                writeln!(output).expect("writing to String cannot fail");
            }
            let state = runtime
                .and_then(|snapshot| {
                    snapshot
                        .members
                        .iter()
                        .find(|member| member.team == row.team && member.member == row.assignee)
                })
                .map_or("unknown", |member| runtime_state_name(member.state));
            writeln!(output, "{}", member_state_header(&row.assignee, state))
                .expect("writing to String cannot fail");
            writeln!(
                output,
                "pos  state     task_id     assigned_at               reminders assigner"
            )
            .expect("writing to String cannot fail");
            current_member = Some(row.assignee.clone());
        } else if !grouped && current_member.is_none() {
            writeln!(
                output,
                "pos  state     task_id     assigned_at               reminders assigner"
            )
            .expect("writing to String cannot fail");
            current_member = Some(row.assignee.clone());
        }
        writeln!(
            output,
            "{:<4} {:<9} {:<11} {:<25} {:<9} {}",
            row.position.map_or(0, |position| position.get()),
            row.state.as_str(),
            row.task_id.as_str(),
            row.assigned_at
                .into_inner()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
            row.reminder_count,
            row.assigner,
        )
        .expect("writing to String cannot fail");
    }
    if rows.is_empty() {
        writeln!(
            output,
            "pos  state     task_id     assigned_at               reminders assigner"
        )
        .expect("writing to String cannot fail");
    }
    output
}

const fn runtime_state_name(state: RuntimeMemberState) -> &'static str {
    match state {
        RuntimeMemberState::Unknown => "unknown",
        RuntimeMemberState::IdentityConflict => "identity_conflict",
        RuntimeMemberState::Offline => "offline",
        RuntimeMemberState::Idle => "idle",
        RuntimeMemberState::Active => "active",
        RuntimeMemberState::Blocked => "blocked",
    }
}

fn member_state_header(member: &AgentName, state: &str) -> String {
    format!("{member} (state: {state})")
}

fn render_task_events(rows: &[TaskEventRow], json: bool) -> Result<String> {
    if json {
        return Ok(format!("{}\n", serde_json::to_string_pretty(rows)?));
    }
    let mut output = String::from("seq at event from→to actor detail\n");
    for row in rows {
        let actor = match &row.actor {
            TaskActor::Member(member) => member.as_str(),
            TaskActor::Daemon => DAEMON_ACTOR_NAME,
        };
        let from = row.from_state.map_or("-", |state| state.as_str());
        let to = row.to_state.map_or("-", |state| state.as_str());
        let detail = row
            .detail
            .as_deref()
            .or_else(|| row.marker.map(|marker| marker.as_str()))
            .or_else(|| row.outcome.map(|outcome| outcome.as_str()))
            .unwrap_or("-");
        writeln!(
            output,
            "{} {} {} {}→{} {} {}",
            row.seq,
            row.at
                .into_inner()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
            row.event.as_str(),
            from,
            to,
            actor,
            detail,
        )?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use atm_core::protocol::{CompatibilityVerdict, HttpApiVersion, ReleaseVersion};
    use atm_core::test_support::{EnvGuard, TEST_TEAM};
    use clap::{CommandFactory, Parser};
    use serial_test::serial;
    use tempfile::TempDir;

    use super::*;
    use crate::commands::send::require_daemon_api;
    use crate::commands::{Cli, Command};

    #[test]
    fn task_has_exactly_five_subcommands() {
        let command = Cli::command();
        let task = command.find_subcommand("task").expect("task command");
        let names: std::collections::BTreeSet<_> = task
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect();
        assert_eq!(names, ["assign", "close", "events", "list", "move"].into());
    }

    #[test]
    fn close_parses_positional_outcome_and_reason() {
        let cli = Cli::try_parse_from(["atm", "task", "close", "T1", "refused", "no capacity"])
            .expect("valid close");
        let Command::Task(TaskCommand {
            command: TaskSubcommand::Close(close),
        }) = cli.command
        else {
            panic!("expected task close");
        };
        assert!(matches!(close.outcome, OutcomeArg::Refused));
        assert_eq!(close.reason.as_deref(), Some("no capacity"));
        let error = Cli::try_parse_from(["atm", "task", "close", "T1", "bogus"])
            .expect_err("invalid outcome");
        let rendered = error.to_string();
        for expected in ["completed", "refused", "cancelled"] {
            assert!(rendered.contains(expected));
        }
    }

    #[test]
    fn close_without_reason_or_report_source_is_rejected() {
        let cli =
            Cli::try_parse_from(["atm", "task", "close", "T1", "refused"]).expect("clap shape");
        let Command::Task(TaskCommand {
            command: TaskSubcommand::Close(close),
        }) = cli.command
        else {
            panic!("expected task close");
        };
        assert!(close.validate().is_err());
    }

    #[test]
    fn close_with_source_and_no_reason_is_accepted() {
        for source in [
            vec!["--stdin"],
            vec!["--template", "report.j2", "--vars", "report.json"],
        ] {
            let mut args = vec!["atm", "task", "close", "T1", "completed"];
            args.extend(source);
            let cli = Cli::try_parse_from(args).expect("close report source");
            let Command::Task(TaskCommand {
                command: TaskSubcommand::Close(close),
            }) = cli.command
            else {
                panic!("expected task close");
            };
            assert!(close.reason.is_none());
            close.validate().expect("source makes close valid");
        }
    }

    #[test]
    fn move_requires_exactly_one_target() {
        assert!(Cli::try_parse_from(["atm", "task", "move", "T1"]).is_err());
        assert!(Cli::try_parse_from(["atm", "task", "move", "T1", "--head", "--end"]).is_err());
        for target in [vec!["--head"], vec!["--end"], vec!["--before", "T2"]] {
            let mut args = vec!["atm", "task", "move", "T1"];
            args.extend(target);
            Cli::try_parse_from(args).expect("one move target");
        }
    }

    #[test]
    fn assign_without_task_id_mints_ulid() {
        let minted = resolve_task_id(None).expect("generated task id");
        assert_eq!(minted.as_str().len(), 26);
        assert!(minted.as_str().chars().all(|character| {
            matches!(character, '0'..='9' | 'A'..='H' | 'J'..='N' | 'P'..='T' | 'V'..='Z')
        }));
        let supplied: TaskId = "T1".parse().expect("task id");
        assert_eq!(
            resolve_task_id(Some(supplied.clone())).expect("supplied task id"),
            supplied
        );
    }

    #[test]
    #[serial(env)]
    fn task_assign_template_load_failure_exits_two() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some("sender"))]);
        let home = TempDir::new().expect("home directory");
        let current = TempDir::new().expect("current directory");
        let command = SendCommand::for_task(TaskSendOptions {
            to: "recipient@test-team".to_string(),
            message: None,
            team: Some(TEST_TEAM.to_string()),
            actor: None,
            file: None,
            stdin: false,
            template: Some(current.path().join("missing.j2")),
            vars: None,
            task_id: Some("T1".parse().expect("task id")),
            json: false,
        });
        let error = command
            .build_request_with_mode(
                home.path().to_path_buf(),
                current.path().to_path_buf(),
                NudgeMode::Deferred,
                None,
            )
            .expect_err("missing task template");

        assert_eq!(crate::exit_code_for_error(&error), 2);
    }

    #[test]
    fn list_and_events_accept_bounded_or_all_paging() {
        Cli::try_parse_from(["atm", "task", "list", "--all", "--json"])
            .expect("documented list flags");
        Cli::try_parse_from(["atm", "task", "list", "--limit", "10"]).expect("bounded list");
        assert!(Cli::try_parse_from(["atm", "task", "list", "--all", "--limit", "10"]).is_err());
        Cli::try_parse_from(["atm", "task", "events", "T1", "--all"]).expect("all events");
        Cli::try_parse_from(["atm", "task", "events", "T1", "--limit", "10"])
            .expect("bounded events");
        assert!(
            Cli::try_parse_from(["atm", "task", "events", "T1", "--all", "--limit", "10"]).is_err()
        );
        assert!(Cli::try_parse_from(["atm", "task", "list", "--member", "fenix"]).is_err());
    }

    #[test]
    fn close_preflight_query_pushes_down_the_task_id() {
        let task_id: TaskId = "T1".parse().expect("task id");
        let home = TempDir::new().expect("home");
        let current = TempDir::new().expect("current");
        let query = task_list_request(
            home.path().to_path_buf(),
            current.path().to_path_buf(),
            "alice".parse().expect("agent"),
            TEST_TEAM.parse().expect("team"),
            None,
            Some(&task_id),
        )
        .expect("task query");
        assert_eq!(query.task_filter.as_ref(), Some(&task_id));
    }

    #[test]
    fn renderer_groups_every_member_with_state_header() {
        let row = |task_id: &str, assignee: &str, position: u32| -> TaskRow {
            serde_json::from_value(serde_json::json!({
                "team": "test-team",
                "task_id": task_id,
                "assignee": assignee,
                "assigner": "lead",
                "state": "assigned",
                "position": position,
                "assignment_message_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "description": task_id,
                "assigned_at": "2026-01-02T00:00:00Z",
                "updated_at": "2026-01-02T00:00:00Z",
                "reminder_count": 0,
                "lead_notified_count": 0
            }))
            .expect("task row")
        };
        let runtime: RuntimeStatusSnapshot = serde_json::from_value(serde_json::json!({
            "liveness": "running",
            "readiness": "ready",
            "members": [
                {"team": "test-team", "member": "alice", "state": "active"},
                {"team": "test-team", "member": "bob", "state": "idle"}
            ]
        }))
        .expect("runtime snapshot");
        let output = render_task_rows(
            &[
                row("A1", "alice", 1),
                row("A2", "alice", 2),
                row("B1", "bob", 1),
            ],
            true,
            Some(&runtime),
        );
        assert_eq!(
            output,
            concat!(
                "alice (state: active)\n",
                "pos  state     task_id     assigned_at               reminders assigner\n",
                "1    assigned  A1          2026-01-02T00:00:00Z      0         lead\n",
                "2    assigned  A2          2026-01-02T00:00:00Z      0         lead\n",
                "\n",
                "bob (state: idle)\n",
                "pos  state     task_id     assigned_at               reminders assigner\n",
                "1    assigned  B1          2026-01-02T00:00:00Z      0         lead\n",
            )
        );
    }

    #[test]
    fn require_daemon_api_refuses_older_daemon() {
        let verdict = |version: &str| CompatibilityVerdict::Compatible {
            daemon_release: ReleaseVersion::parse("1.5.14").expect("release"),
            daemon_schema_version: 1,
            daemon_http_api_version: HttpApiVersion::parse(version).expect("HTTP API"),
        };
        let min_15 = HttpApiVersion::parse("1.5.0").expect("minimum");
        let min_16 = HttpApiVersion::parse("1.6.0").expect("minimum");
        assert!(require_daemon_api(&verdict("1.4.0"), min_15.clone(), "task list").is_err());
        assert!(require_daemon_api(&verdict("1.5.0"), min_15, "task list").is_ok());
        assert!(require_daemon_api(&verdict("1.5.0"), min_16.clone(), "task move").is_err());
        assert!(require_daemon_api(&verdict("1.6.0"), min_16, "task move").is_ok());
    }

    #[test]
    fn compatibility_helper_refuses_task_move_below_1_6_0() {
        let verdict = CompatibilityVerdict::Compatible {
            daemon_release: ReleaseVersion::parse("1.5.14").expect("release"),
            daemon_schema_version: 1,
            daemon_http_api_version: HttpApiVersion::parse("1.5.0").expect("HTTP API"),
        };
        let error = require_daemon_api(
            &verdict,
            HttpApiVersion::parse("1.6.0").expect("minimum"),
            "task move",
        )
        .expect_err("preflight must reject before request execution");
        assert!(error.message().contains("1.5.0"));
        assert!(error.message().contains("1.6.0"));
    }

    #[test]
    fn parser_accepts_move_head_end_and_before() {
        let cases = [
            (vec!["--head"], MoveTarget::Head),
            (vec!["--end"], MoveTarget::End),
            (
                vec!["--before", "T2"],
                MoveTarget::Before {
                    task_id: "T2".parse().expect("task id"),
                },
            ),
        ];
        for (flags, expected) in cases {
            let mut args = vec!["atm", "task", "move", "T1"];
            args.extend(flags);
            let cli = Cli::try_parse_from(args).expect("valid task move");
            let Command::Task(TaskCommand {
                command: TaskSubcommand::Move(command),
            }) = cli.command
            else {
                panic!("expected task move");
            };
            assert_eq!(command.target(), expected);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/task_close.rs"]
mod task_close_tests;

#[cfg(test)]
#[path = "../../tests/task_assign.rs"]
mod task_assign_tests;

#[cfg(test)]
#[path = "../../tests/task_move.rs"]
mod task_move_tests;

#[cfg(test)]
#[path = "../../tests/task_list_events.rs"]
mod task_list_events_tests;
