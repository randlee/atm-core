//! Closed task command surface.

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use atm_core::address::AgentAddress;
use atm_core::list::{ListQuery, TaskLedgerQuery};
use atm_core::protocol::{HttpApiVersion, RequestEnvelope, ResponseEnvelope, TaskMoveRequest};
use atm_core::send::NudgeMode;
use atm_core::task_close::report_recipient;
use atm_core::task_query::{TaskEventQuery, TaskListQuery, TaskPage};
use atm_core::types::{TaskId, TeamName};
use atm_storage::{
    DAEMON_ACTOR_NAME, MoveTarget, TaskActor, TaskCloseOutcome, TaskEventRow, TaskOp, TaskRow,
};
use chrono::SecondsFormat;
use clap::{ArgGroup, Args, Subcommand, ValueEnum};

use crate::commands::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride, resolve_cli_caller_context,
};
use crate::commands::send::{
    SendCommand, TaskSendOptions, preflight_daemon_api, validate_local_task_target,
};
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
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
}

#[derive(Debug, Args)]
struct TaskEventsCommand {
    task_id: TaskId,
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
        let task_id = resolve_task_id(self.task_id.clone())?;
        let placement = self.placement();
        let json = self.json;
        let assignee = self.assignee.to_string();
        let (home_dir, current_dir) = resolve_command_runtime_context("task assign")?;
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
        let composition = composition("task assign", observability, &home_dir, &current_dir)?;
        preflight_daemon_api(&composition, HttpApiVersion::parse("1.5.0")?, "task assign").await?;
        composition.send(request).await?;
        if json {
            println!("{}", serde_json::json!({ "task_id": task_id }));
        } else {
            println!("assigned {task_id} to {assignee}");
        }
        Ok(())
    }
}

impl TaskCloseCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        self.validate()?;
        let (home_dir, current_dir) = resolve_command_runtime_context("task close")?;
        let caller = resolve_context(&self.caller)?;
        let composition = composition("task close", observability, &home_dir, &current_dir)?;
        preflight_daemon_api(&composition, HttpApiVersion::parse("1.5.0")?, "task close").await?;
        let query = task_list_request(
            home_dir.clone(),
            current_dir.clone(),
            caller.caller_identity.clone(),
            caller.caller_team.clone(),
            None,
        )?;
        let rows = composition.list(query).await?.task_rows;
        let row = rows
            .into_iter()
            .find(|row| row.task_id == self.task_id)
            .ok_or_else(|| {
                atm_core::error::AtmError::validation(format!(
                    "task {} does not exist on team {}",
                    self.task_id, caller.caller_team
                ))
            })?;
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
        .build_request_with_mode(
            home_dir.clone(),
            current_dir.clone(),
            NudgeMode::Immediate,
            None,
        )?;
        request.task_id = Some(self.task_id.clone());
        request.task_op = Some(TaskOp::Close { outcome, reason });
        validate_local_task_target(&request)?;
        let result = composition.send(request).await?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else if let Some(closed) = result.already_closed {
            println!(
                "task {} was already closed ({}); report delivered",
                self.task_id,
                closed.as_str()
            );
        } else {
            println!("closed {} ({})", self.task_id, outcome.as_str());
        }
        Ok(())
    }
}

impl TaskListCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("task list")?;
        let caller = resolve_context(&self.caller)?;
        let contract = TaskListQuery {
            team: caller.caller_team.clone(),
            assignee: (!self.all).then_some(caller.caller_identity.clone()),
            page: TaskPage::default_bounded(),
        };
        let composition = composition("task list", observability, &home_dir, &current_dir)?;
        preflight_daemon_api(&composition, HttpApiVersion::parse("1.5.0")?, "task list").await?;
        let outcome = composition
            .list(task_list_request(
                home_dir.clone(),
                current_dir.clone(),
                caller.caller_identity,
                contract.team,
                contract.assignee,
            )?)
            .await?;
        print_task_rows(&outcome.task_rows, self.json, self.all)
    }
}

impl TaskEventsCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("task events")?;
        let caller = resolve_context(&self.caller)?;
        let contract = TaskEventQuery {
            team: caller.caller_team.clone(),
            task_id: self.task_id.clone(),
            assignee: None,
            page: TaskPage::default_bounded(),
        };
        let composition = composition("task events", observability, &home_dir, &current_dir)?;
        preflight_daemon_api(&composition, HttpApiVersion::parse("1.5.0")?, "task events").await?;
        let ledger = TaskLedgerQuery::Events {
            task_id: contract.task_id.clone(),
            member: None,
        };
        let query = ListQuery::new(
            home_dir.clone(),
            current_dir.clone(),
            caller.caller_identity,
            None,
            contract.team,
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
        print_task_events(&outcome.task_event_rows, self.json)
    }
}

impl TaskMoveCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("task move")?;
        let caller = resolve_context(&self.caller)?;
        let target = match (self.head, self.end, self.before) {
            (true, false, None) => MoveTarget::Head,
            (false, true, None) => MoveTarget::End,
            (false, false, Some(task_id)) => MoveTarget::Before { task_id },
            _ => unreachable!("clap target group"),
        };
        let composition = composition("task move", observability, &home_dir, &current_dir)?;
        preflight_daemon_api(&composition, HttpApiVersion::parse("1.6.0")?, "task move").await?;
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
                println!("{}", serde_json::to_string_pretty(&outcome)?);
                Ok(())
            }
            ResponseEnvelope::TaskMove(outcome) => {
                println!(
                    "moved {} for {} from {} to {}",
                    outcome.task_id,
                    outcome.assignee,
                    outcome.from.get(),
                    outcome.to.get()
                );
                Ok(())
            }
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
    home_dir: &PathBuf,
    current_dir: &PathBuf,
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
        None,
        None,
    )?
    .with_task_ledger(TaskLedgerQuery::Tasks { member }))
}

fn resolve_task_id(task_id: Option<TaskId>) -> Result<TaskId, atm_core::error::AtmError> {
    task_id.map_or_else(|| TaskId::from_str(&ulid::Ulid::new().to_string()), Ok)
}

fn print_task_rows(rows: &[TaskRow], json: bool, grouped: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(rows)?);
        return Ok(());
    }
    let mut current_member = None;
    for row in rows {
        if grouped && current_member.as_ref() != Some(&row.assignee) {
            if current_member.is_some() {
                println!();
            }
            println!("{} (state: unknown)", row.assignee);
            println!("pos  state     task_id     assigned_at               reminders assigner");
            current_member = Some(row.assignee.clone());
        } else if !grouped && current_member.is_none() {
            println!("pos  state     task_id     assigned_at               reminders assigner");
            current_member = Some(row.assignee.clone());
        }
        println!(
            "{:<4} {:<9} {:<11} {:<25} {:<9} {}",
            row.position.map_or(0, |position| position.get()),
            row.state.as_str(),
            row.task_id,
            row.assigned_at
                .into_inner()
                .to_rfc3339_opts(SecondsFormat::Secs, true),
            row.reminder_count,
            row.assigner,
        );
    }
    if rows.is_empty() {
        println!("pos  state     task_id     assigned_at               reminders assigner");
    }
    Ok(())
}

fn print_task_events(rows: &[TaskEventRow], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(rows)?);
        return Ok(());
    }
    println!("seq at event from→to actor detail");
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
        println!(
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
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use atm_core::protocol::{CompatibilityVerdict, HttpApiVersion, ReleaseVersion};
    use clap::{CommandFactory, Parser};

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
    fn list_has_only_all_and_json_flags() {
        Cli::try_parse_from(["atm", "task", "list", "--all", "--json"])
            .expect("documented list flags");
        assert!(Cli::try_parse_from(["atm", "task", "list", "--member", "fenix"]).is_err());
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
}
