//! Canonical task CLI adapter.
//!
//! This module parses and renders the public `atm task` surface. Lifecycle
//! policy, roster authorization, durable writes, and idempotency live only in
//! `atm-core::task_command` and the injected HTTP service.

use std::io::Read as _;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use atm_core::load_atm_config;
use atm_core::send::{TemplateSendSource, input, render_template_source_for_task};
use atm_core::task_command::{
    AbortInput, AssignmentInput, ComposedMessageInput, HandoffInput, NonEmptyText, TaskAction,
    TaskCommandRequest, TaskCommandResponse, TaskEventQuery, TaskListQuery, TaskListScope,
    TaskMutationCommand, TaskPage,
};
use atm_core::types::{AgentName, TaskId};
use atm_storage::{MemberKey, TaskOperationId, TaskPriority};
use chrono::{DateTime, Utc};
use clap::{Args, Subcommand, ValueEnum};

use crate::commands::caller_context::{
    CallerContextOverrides, CallerTeamOverride, resolve_cli_caller_context,
    resolve_cli_mutation_caller_context,
};
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;

/// Query and mutate durable task lifecycle state through the maintained HTTP API.
#[derive(Debug, Args)]
pub struct TaskCommand {
    #[command(subcommand)]
    command: TaskSubcommand,
}

#[derive(Debug, Subcommand)]
enum TaskSubcommand {
    /// List actionable tasks, or closed task history with --closed.
    List(TaskListCommand),
    /// List lifecycle events and attempts for a stable task id.
    Events(TaskEventsCommand),
    /// Assign a new task to a same-host team member.
    Assign(TaskAssignCommand),
    /// Begin the caller's assigned task.
    Start(TaskSimpleCommand),
    /// Block the caller's assigned task with a durable reason.
    Block(TaskReasonCommand),
    /// Return a blocked task to Assigned with a durable resolution.
    Unblock(TaskResolutionCommand),
    /// Assign a new attempt for an existing non-active task.
    Reassign(TaskReassignCommand),
    /// Reopen a closed task with a new assignment attempt.
    Reopen(TaskReassignCommand),
    /// Close an active task successfully and atomically send a handoff.
    Complete(TaskTerminalCommand),
    /// Close an active task as failed and atomically send a handoff.
    Fail(TaskTerminalCommand),
    /// Abort a task as cancelled or superseded.
    Abort(TaskAbortCommand),
}

impl TaskCommand {
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        self.command.run(observability).await
    }
}

#[derive(Debug, Args)]
struct TaskListCommand {
    /// Equivalent to --as for one selected assignee.
    agent: Option<AgentName>,
    #[arg(long = "as", conflicts_with = "agent")]
    assignee: Option<AgentName>,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    closed: bool,
    #[arg(long, conflicts_with = "all")]
    limit: Option<usize>,
    #[arg(long, conflicts_with = "limit")]
    all: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct TaskEventsCommand {
    task_id: TaskId,
    #[arg(long)]
    team: Option<String>,
    #[arg(long = "as")]
    assignee: Option<AgentName>,
    #[arg(long, conflicts_with = "all")]
    limit: Option<usize>,
    #[arg(long, conflicts_with = "limit")]
    all: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct TaskAssignCommand {
    to: AgentName,
    task_id: TaskId,
    #[arg(long, value_enum, default_value_t = PriorityArg::Normal)]
    priority: PriorityArg,
    #[command(flatten)]
    source: MessageSource,
    #[command(flatten)]
    actor: MutationActor,
}

#[derive(Debug, Args)]
struct TaskReassignCommand {
    task_id: TaskId,
    assignee: AgentName,
    #[command(flatten)]
    source: MessageSource,
    #[command(flatten)]
    actor: MutationActor,
}

#[derive(Debug, Args)]
struct TaskSimpleCommand {
    task_id: TaskId,
    #[command(flatten)]
    actor: MutationActor,
}

#[derive(Debug, Args)]
struct TaskReasonCommand {
    task_id: TaskId,
    #[arg(long)]
    reason: String,
    #[command(flatten)]
    actor: MutationActor,
}

#[derive(Debug, Args)]
struct TaskResolutionCommand {
    task_id: TaskId,
    #[arg(long)]
    resolution: String,
    #[command(flatten)]
    actor: MutationActor,
}

#[derive(Debug, Args)]
struct TaskTerminalCommand {
    task_id: TaskId,
    #[arg(long)]
    handoff: AgentName,
    #[command(flatten)]
    source: MessageSource,
    #[command(flatten)]
    actor: MutationActor,
}

#[derive(Debug, Args)]
struct TaskAbortCommand {
    task_id: TaskId,
    #[arg(long)]
    handoff: AgentName,
    #[arg(long, value_enum)]
    reason: AbortReasonArg,
    #[arg(long, required_if_eq("reason", "superseded"))]
    successor_task_id: Option<TaskId>,
    #[arg(long, required_if_eq("reason", "superseded"))]
    assign_to: Option<AgentName>,
    #[arg(long, value_enum, default_value_t = PriorityArg::Normal)]
    successor_priority: PriorityArg,
    #[arg(long = "assignment-text", required_if_eq("reason", "superseded"))]
    assignment_text: Option<String>,
    #[command(flatten)]
    source: MessageSource,
    #[command(flatten)]
    actor: MutationActor,
}

#[derive(Debug, Args)]
struct MutationActor {
    #[arg(long)]
    team: Option<String>,
    #[arg(long = "as")]
    actor: Option<String>,
}

#[derive(Debug, Args)]
struct MessageSource {
    /// Plain text message body. Exactly one source is required.
    #[arg(value_name = "MESSAGE", conflicts_with_all = ["file", "stdin", "template"])]
    text: Option<String>,
    #[arg(long, conflicts_with_all = ["text", "stdin", "template"])]
    file: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["text", "file", "template"])]
    stdin: bool,
    /// Render a local template before requesting the durable task mutation.
    #[arg(long, conflicts_with_all = ["text", "file", "stdin"])]
    template: Option<PathBuf>,
    /// JSON object supplying variables to --template.
    #[arg(long, requires = "template")]
    vars: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PriorityArg {
    High,
    Normal,
    Low,
}

impl From<PriorityArg> for TaskPriority {
    fn from(value: PriorityArg) -> Self {
        match value {
            PriorityArg::High => Self::High,
            PriorityArg::Normal => Self::Normal,
            PriorityArg::Low => Self::Low,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AbortReasonArg {
    Cancelled,
    Superseded,
}

impl TaskSubcommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        match self {
            Self::List(command) => command.run(observability).await,
            Self::Events(command) => command.run(observability).await,
            Self::Assign(command) => command.run(observability, TaskActionKind::Assign).await,
            Self::Reassign(command) => command.run(observability, TaskActionKind::Reassign).await,
            Self::Reopen(command) => command.run(observability, TaskActionKind::Reopen).await,
            Self::Start(command) => command.run(observability, TaskAction::Start).await,
            Self::Block(command) => {
                command
                    .run(observability, |value| TaskAction::Block { reason: value })
                    .await
            }
            Self::Unblock(command) => {
                command
                    .run(observability, |value| TaskAction::Unblock {
                        resolution: value,
                    })
                    .await
            }
            Self::Complete(command) => command.run(observability, false).await,
            Self::Fail(command) => command.run(observability, true).await,
            Self::Abort(command) => command.run(observability).await,
        }
    }
}

impl TaskListCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        let context = resolve_cli_caller_context(CallerContextOverrides {
            identity_override: None,
            chat_id_override: None,
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        let page = page(self.limit, self.all)?;
        let request = TaskCommandRequest::List(TaskListQuery {
            team: context.caller_team,
            assignee: self.agent.or(self.assignee),
            scope: if self.closed {
                TaskListScope::Closed
            } else {
                TaskListScope::Open
            },
            page,
        });
        let response = execute(observability, request).await?;
        print_response(response, self.json)
    }
}

impl TaskEventsCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        let context = resolve_cli_caller_context(CallerContextOverrides {
            identity_override: None,
            chat_id_override: None,
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        let response = execute(
            observability,
            TaskCommandRequest::Events(TaskEventQuery {
                team: context.caller_team,
                task_id: self.task_id,
                assignee: self.assignee,
                page: page(self.limit, self.all)?,
            }),
        )
        .await?;
        print_response(response, self.json)
    }
}

#[derive(Clone, Copy)]
enum TaskActionKind {
    Assign,
    Reassign,
    Reopen,
}

impl TaskAssignCommand {
    async fn run(self, observability: &CliObservability, _: TaskActionKind) -> Result<()> {
        let actor = self.actor.member()?;
        let message = self.source.compose()?;
        mutate(
            observability,
            actor.clone(),
            self.task_id,
            TaskAction::Assign(AssignmentInput {
                assignee: MemberKey::new(actor.team().clone(), self.to),
                priority: self.priority.into(),
                message,
            }),
        )
        .await
    }
}

impl TaskReassignCommand {
    async fn run(self, observability: &CliObservability, kind: TaskActionKind) -> Result<()> {
        let actor = self.actor.member()?;
        let input = AssignmentInput {
            assignee: MemberKey::new(actor.team().clone(), self.assignee),
            priority: TaskPriority::Normal,
            message: self.source.compose()?,
        };
        let action = match kind {
            TaskActionKind::Reassign => TaskAction::Reassign(input),
            TaskActionKind::Reopen => TaskAction::Reopen(input),
            TaskActionKind::Assign => unreachable!(),
        };
        mutate(observability, actor, self.task_id, action).await
    }
}

impl TaskSimpleCommand {
    async fn run(self, observability: &CliObservability, action: TaskAction) -> Result<()> {
        mutate(observability, self.actor.member()?, self.task_id, action).await
    }
}

impl TaskReasonCommand {
    async fn run(
        self,
        observability: &CliObservability,
        action: impl FnOnce(NonEmptyText) -> TaskAction,
    ) -> Result<()> {
        mutate(
            observability,
            self.actor.member()?,
            self.task_id,
            action(NonEmptyText::new(self.reason)?),
        )
        .await
    }
}

impl TaskResolutionCommand {
    async fn run(
        self,
        observability: &CliObservability,
        action: impl FnOnce(NonEmptyText) -> TaskAction,
    ) -> Result<()> {
        mutate(
            observability,
            self.actor.member()?,
            self.task_id,
            action(NonEmptyText::new(self.resolution)?),
        )
        .await
    }
}

impl TaskTerminalCommand {
    async fn run(self, observability: &CliObservability, failed: bool) -> Result<()> {
        let actor = self.actor.member()?;
        let handoff = HandoffInput {
            recipient: MemberKey::new(actor.team().clone(), self.handoff),
            message: self.source.compose()?,
        };
        mutate(
            observability,
            actor,
            self.task_id,
            if failed {
                TaskAction::Fail(handoff)
            } else {
                TaskAction::Complete(handoff)
            },
        )
        .await
    }
}

impl TaskAbortCommand {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        let actor = self.actor.member()?;
        let handoff = HandoffInput {
            recipient: MemberKey::new(actor.team().clone(), self.handoff),
            message: self.source.compose()?,
        };
        let reason = match self.reason {
            AbortReasonArg::Cancelled => AbortInput::Cancelled,
            AbortReasonArg::Superseded => AbortInput::Superseded {
                successor_task_id: self.successor_task_id.expect("clap requires successor id"),
                successor: AssignmentInput {
                    assignee: MemberKey::new(
                        actor.team().clone(),
                        self.assign_to.expect("clap requires successor assignee"),
                    ),
                    priority: self.successor_priority.into(),
                    message: ComposedMessageInput {
                        body: NonEmptyText::new(
                            self.assignment_text.expect("clap requires successor text"),
                        )?,
                        template_sha: None,
                    },
                },
            },
        };
        mutate(
            observability,
            actor,
            self.task_id,
            TaskAction::Abort { reason, handoff },
        )
        .await
    }
}

impl MutationActor {
    fn member(&self) -> Result<MemberKey> {
        let context =
            resolve_cli_mutation_caller_context(self.team.as_deref().map(CallerTeamOverride))?;
        if let Some(value) = self.actor.as_deref() {
            let requested = AgentName::from_str(value)?;
            if requested != context.caller_identity {
                return Err(atm_core::error::AtmError::caller_context_request_invalid(
                    "--as must identify the authenticated task actor",
                )
                .into());
            }
        }
        Ok(MemberKey::new(context.caller_team, context.caller_identity))
    }
}

impl MessageSource {
    fn compose(self) -> Result<ComposedMessageInput> {
        let body = match (self.text, self.file, self.stdin, self.template) {
            (Some(text), None, false, None) => text,
            (None, Some(path), false, None) => std::fs::read_to_string(path)?,
            (None, None, true, None) => {
                let mut text = String::new();
                std::io::stdin().read_to_string(&mut text)?;
                text
            }
            (None, None, false, Some(path)) => {
                let (body, template_sha) = render_task_template(&path, self.vars.as_deref())?;
                return Ok(ComposedMessageInput {
                    body: NonEmptyText::new(body)?,
                    template_sha: Some(template_sha),
                });
            }
            _ => {
                return Err(atm_core::error::AtmError::validation(
                    "supply exactly one task message source",
                )
                .into());
            }
        };
        Ok(ComposedMessageInput {
            body: NonEmptyText::new(body)?,
            template_sha: None,
        })
    }
}

/// Verify and render a task template using the same captured-source admission
/// policy as ordinary sends. Only the rendered body and immutable SHA cross
/// into the task service; task metadata never carries a source path or vars.
fn render_task_template(
    template: &std::path::Path,
    vars: Option<&std::path::Path>,
) -> Result<(String, atm_storage::TemplateSha)> {
    let (_, current_dir) = resolve_command_runtime_context("task")?;
    let path = if template.is_absolute() {
        template.to_path_buf()
    } else {
        current_dir.join(template)
    };
    let canonical_template_path = std::fs::canonicalize(&path)?;
    let canonical_template_root = canonical_template_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("template path has no parent directory"))?
        .to_path_buf();
    let raw_file_bytes = std::fs::read(&canonical_template_path)?;
    let var_file_values = match vars {
        Some(path) => {
            let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
            value
                .as_object()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("--vars must contain a JSON object"))?
        }
        None => serde_json::Map::new(),
    };
    let max_message_bytes = load_atm_config(&current_dir)?
        .map(|config| {
            config.max_message_bytes.as_usize().ok_or_else(|| {
                anyhow::anyhow!("configured max_message_bytes does not fit this platform")
            })
        })
        .transpose()?
        .unwrap_or(input::default_message_max_bytes());
    let source = TemplateSendSource {
        canonical_template_path,
        canonical_template_root,
        raw_file_bytes,
        input_defaults: serde_json::Map::new(),
        var_file_values,
        explicit_values: serde_json::Map::new(),
        environment_values: serde_json::Map::new(),
    };
    let composer = atm_daemon_bootstrap::template_composer();
    render_template_source_for_task(composer.as_ref(), &source, max_message_bytes)
        .map_err(Into::into)
}

async fn mutate(
    observability: &CliObservability,
    actor: MemberKey,
    task_id: TaskId,
    action: TaskAction,
) -> Result<()> {
    let response = execute(
        observability,
        TaskCommandRequest::Mutate(Box::new(TaskMutationCommand {
            operation_id: TaskOperationId::new(),
            actor,
            task_id,
            expected_revision: None,
            action,
        })),
    )
    .await?;
    print_response(response, true)
}

/// Run the deprecated `atm send --task-id` path through the one maintained
/// task service. The send adapter owns only compatibility parsing and warning
/// text; authorization and task mutation remain service-owned.
pub(crate) async fn run_legacy_assign(
    observability: &CliObservability,
    actor: MemberKey,
    task_id: TaskId,
    assignee: MemberKey,
    message: ComposedMessageInput,
) -> Result<()> {
    eprintln!(
        "warning: `atm send --task-id` is deprecated; use `atm task assign <to> <task-id> <message-source>`"
    );
    mutate(
        observability,
        actor,
        task_id,
        TaskAction::LegacyAssign(AssignmentInput {
            assignee,
            priority: TaskPriority::Normal,
            message,
        }),
    )
    .await
}

/// Run the deprecated `atm send --task-complete` path through the canonical
/// task service while retaining its deliberately narrow provenance.
pub(crate) async fn run_legacy_complete(
    observability: &CliObservability,
    actor: MemberKey,
    task_id: TaskId,
    recipient: MemberKey,
    message: ComposedMessageInput,
) -> Result<()> {
    eprintln!(
        "warning: `atm send --task-complete` is deprecated; use `atm task complete <task-id> --handoff <agent> <message-source>`"
    );
    mutate(
        observability,
        actor,
        task_id,
        TaskAction::LegacyComplete(atm_core::task_command::LegacyCompletionNoticeInput {
            recipient,
            message,
        }),
    )
    .await
}

pub(crate) async fn execute(
    observability: &CliObservability,
    request: TaskCommandRequest,
) -> Result<TaskCommandResponse> {
    let (home, current) = resolve_command_runtime_context("task")?;
    CliComposition::bootstrap(
        "task",
        observability,
        InvocationDir::new(&current),
        AtmHomePath::new(&home),
    )?
    .task(request)
    .await
    .map_err(Into::into)
}

pub(crate) fn page(limit: Option<usize>, all: bool) -> Result<TaskPage> {
    if all {
        Ok(TaskPage::All)
    } else {
        limit
            .map(TaskPage::bounded)
            .transpose()
            .map(|value| value.unwrap_or_else(TaskPage::default_bounded))
            .map_err(Into::into)
    }
}

pub(crate) fn print_response(response: TaskCommandResponse, json: bool) -> Result<()> {
    print_response_at(response, json, Utc::now())
}

fn print_response_at(response: TaskCommandResponse, json: bool, now: DateTime<Utc>) -> Result<()> {
    if json || matches!(response, TaskCommandResponse::Mutation(_)) {
        println!(
            "{}",
            serde_json::to_string_pretty(&json_response_with_assignment_age(&response, now)?)?
        );
        return Ok(());
    }
    match response {
        TaskCommandResponse::List(value) => {
            println!("TASK_ID\tSTATE\tASSIGNEE\tPRIORITY\tAGE");
            for row in value.rows {
                let age = format_task_age(row.original_assigned_at.into_inner(), now);
                println!(
                    "{}\t{:?}\t{}\t{:?}\t{age}",
                    row.task_id, row.state, row.current_assignee, row.priority
                );
            }
        }
        TaskCommandResponse::Events(value) => {
            println!("{}", serde_json::to_string_pretty(&value.rows)?)
        }
        TaskCommandResponse::Mutation(_) => unreachable!("mutation handled above"),
    }
    Ok(())
}

fn json_response_with_assignment_age(
    response: &TaskCommandResponse,
    now: DateTime<Utc>,
) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(response)?;
    let TaskCommandResponse::List(list) = response else {
        return Ok(value);
    };
    let rows = value
        .get_mut("list")
        .and_then(|list| list.get_mut("rows"))
        .and_then(serde_json::Value::as_array_mut)
        .expect("TaskCommandResponse list serialization has rows");
    for (row, logical_task) in rows.iter_mut().zip(&list.rows) {
        row.as_object_mut()
            .expect("logical task serialization is an object")
            .insert(
                "assigned_age_seconds".to_owned(),
                serde_json::Value::from(assigned_age_seconds(
                    logical_task.original_assigned_at.into_inner(),
                    now,
                )),
            );
    }
    Ok(value)
}

fn assigned_age_seconds(original_assigned_at: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    (now - original_assigned_at).num_seconds().max(0)
}

fn format_task_age(original_assigned_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let age = assigned_age_seconds(original_assigned_at, now);
    if age < 60 {
        format!("{age}s")
    } else if age < 3600 {
        format!("{}m", age / 60)
    } else {
        format!("{}h", age / 3600)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use clap::Parser;

    use super::format_task_age;

    #[test]
    fn task_list_age_uses_member_bucket_boundaries_and_clamps_future_values() {
        let now = Utc.with_ymd_and_hms(2026, 9, 10, 3, 0, 0).unwrap();
        for (seconds_ago, expected) in [
            (0, "0s"),
            (59, "59s"),
            (60, "1m"),
            (3_599, "59m"),
            (3_600, "1h"),
            (-1, "0s"),
        ] {
            assert_eq!(
                format_task_age(now - chrono::Duration::seconds(seconds_ago), now),
                expected
            );
        }
    }

    #[test]
    fn parser_exposes_every_canonical_task_subcommand() {
        for command in [
            "list", "events", "assign", "start", "block", "unblock", "reassign", "reopen",
            "complete", "fail", "abort",
        ] {
            assert!(
                crate::commands::Cli::try_parse_from(["atm", "task", command, "--help"]).is_err()
            );
        }
    }

    #[test]
    fn list_accepts_both_specific_agent_forms() {
        crate::commands::Cli::try_parse_from(["atm", "task", "list", "worker"])
            .expect("positional agent");
        crate::commands::Cli::try_parse_from(["atm", "task", "list", "--as", "worker"])
            .expect("flag agent");
    }
}
