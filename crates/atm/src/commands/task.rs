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
    /// Start an assigned task: moves it to active and tells the assigner.
    Start(TaskStartCommand),
    /// Deliver a report and close a task with a typed outcome.
    Close(TaskCloseCommand),
    /// Reorder one member's queue.
    Move(TaskMoveCommand),
}

/// Start an assigned task: moves it to active and tells the assigner.
#[derive(Debug, Args)]
struct TaskStartCommand {
    task_id: TaskId,
    /// Optional note to the assigner (what you will do first). Also accepts --stdin/--file/--template.
    #[command(flatten)]
    report: MessageSourceArgs,
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    caller: CallerArgs,
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
            TaskSubcommand::Start(command) => command.run(observability).await,
            TaskSubcommand::Close(command) => command.run(observability).await,
            TaskSubcommand::Move(command) => command.run(observability).await,
        }
    }
}

impl TaskStartCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("task start")?;
        let caller = resolve_context(&self.caller)?;
        let composition = composition("task start", observability, &home_dir, &current_dir)?;
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
        preflight_daemon_api(composition, HttpApiVersion::parse("1.8.0")?, "task start").await?;
        let query = task_list_request(
            home_dir.clone(),
            current_dir.clone(),
            caller.caller_identity.clone(),
            caller.caller_team.clone(),
            None,
            Some(&self.task_id),
        )?;
        let row = composition
            .list(query)
            .await?
            .task_rows
            .into_iter()
            .find(|row| row.task_id == self.task_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "task {} does not exist on team {}",
                    self.task_id,
                    caller.caller_team
                )
            })?;
        if row.assignee != caller.caller_identity {
            return Err(anyhow::anyhow!(
                "task {} is not assigned to {}",
                self.task_id,
                caller.caller_identity
            ));
        }
        let mut report = self.report;
        if !report.is_present() {
            report.text = Some(format!("started {}", self.task_id));
        }
        let mut request = SendCommand::for_task(report.into_send_options(
            row.assigner.to_string(),
            self.caller,
            None,
            self.json,
        ))
        .build_task_start_request(home_dir, current_dir, self.task_id.clone())?;
        atm_core::send::validate_task_request(&mut request)?;
        let result = composition
            .send(request)
            .await
            .map_err(anyhow::Error::from)?;
        if self.json {
            Ok(format!("{}\n", serde_json::to_string_pretty(&result)?))
        } else {
            Ok(format!("started {}\n", self.task_id))
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
mod tests;
