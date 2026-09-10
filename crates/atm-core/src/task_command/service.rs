use std::time::Duration;

use async_trait::async_trait;
use serde_json::Map;

use super::{
    AbortInput, AssignmentInput, ComposedMessageInput, HandoffInput, LegacyCompletionNoticeInput,
    TaskAction, TaskCommandRequest, TaskCommandResponse, TaskCommandService, TaskEventQuery,
    TaskEventsResponse, TaskListQuery, TaskListResponse, TaskListScope, TaskMutationCommand,
    TaskMutationResponse, TaskPage,
};
use crate::boundary::{Message, MessageKey};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::schema::{AtmMessageId, InboxMessage};
use crate::send::summary::build_summary;
use crate::service_runtime::LocalServiceRuntime;
use atm_storage::{
    AgentType, AsyncTaskLedgerReader, AsyncTaskMutationStore, MessageWriteOrigin,
    PreparedAssignment, PreparedMessage, ReadDeadline, TaskAbortReason, TaskAssignmentAttempt,
    TaskLedgerScope, TaskLifecycleState, TaskMutationRequest, TaskOperation, TaskOutcome,
};

const TASK_COMMAND_READ_DEADLINE: Duration = Duration::from_secs(5);

/// The core-owned task command service. Its runtime owns injected reader and
/// writer capabilities, so command policy never sees a concrete SQLite type.
#[derive(Clone)]
pub struct CoreTaskCommandService {
    runtime: LocalServiceRuntime,
}

impl CoreTaskCommandService {
    #[must_use]
    pub fn new(runtime: LocalServiceRuntime) -> Self {
        Self { runtime }
    }

    async fn list(&self, query: TaskListQuery) -> Result<TaskListResponse, AtmError> {
        let rows = self
            .reader()?
            .list_logical_tasks(
                query.team,
                query.assignee,
                storage_scope(query.scope),
                storage_limit(query.page),
                read_deadline()?,
            )
            .await
            .map_err(AtmError::from)?;
        Ok(TaskListResponse { rows })
    }

    async fn events(&self, query: TaskEventQuery) -> Result<TaskEventsResponse, AtmError> {
        let rows = self
            .reader()?
            .list_task_lifecycle_events(
                query.team,
                query.task_id,
                storage_limit(query.page),
                read_deadline()?,
            )
            .await
            .map_err(AtmError::from)?;
        Ok(TaskEventsResponse { rows })
    }

    async fn mutate(&self, command: TaskMutationCommand) -> Result<TaskMutationResponse, AtmError> {
        let reader = self.reader()?;
        let existing = reader
            .load_logical_task(
                command.actor.team().clone(),
                command.task_id.clone(),
                read_deadline()?,
            )
            .await
            .map_err(AtmError::from)?;
        let attempts = load_attempts(&reader, &command, existing.is_some()).await?;
        let operation = self.prepare_operation(&command, existing.as_ref(), attempts.last())?;
        let outcome = self
            .mutation_store()?
            .apply(TaskMutationRequest {
                operation_id: command.operation_id,
                actor: command.actor,
                task_id: command.task_id,
                expected_revision: command.expected_revision,
                operation,
            })
            .await?;
        response_from_outcome(command.operation_id, existing, outcome)
    }

    fn prepare_operation(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
        latest_attempt: Option<&TaskAssignmentAttempt>,
    ) -> Result<TaskOperation, AtmError> {
        require_member(&self.runtime, &command.actor)?;
        match &command.action {
            TaskAction::Assign(input) => self.assign(command, existing, input),
            TaskAction::Start => self.start(command, existing),
            TaskAction::Block { reason } => self.block(command, existing, reason.as_str()),
            TaskAction::Unblock { resolution } => {
                self.unblock(command, existing, resolution.as_str())
            }
            TaskAction::Reassign(input) => self.reassign(command, existing, latest_attempt, input),
            TaskAction::Reopen(input) => self.reopen(command, existing, latest_attempt, input),
            TaskAction::Complete(handoff) => {
                self.close(command, existing, handoff, TaskOutcome::Succeeded)
            }
            TaskAction::Fail(handoff) => {
                self.close(command, existing, handoff, TaskOutcome::Failed)
            }
            TaskAction::LegacyComplete(notice) => {
                self.legacy_complete(command, existing, latest_attempt, notice)
            }
            TaskAction::Abort { reason, handoff } => {
                self.abort(command, existing, latest_attempt, reason, handoff)
            }
        }
    }

    fn assign(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
        input: &AssignmentInput,
    ) -> Result<TaskOperation, AtmError> {
        if existing.is_some() {
            return Err(task_error("task already exists; use reassign or reopen"));
        }
        require_distinct_member(&command.actor, &input.assignee, "initial assignment")?;
        Ok(TaskOperation::Assign(self.assignment(command, input)?))
    }

    fn start(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
    ) -> Result<TaskOperation, AtmError> {
        require_current_assignee(command, existing)?;
        Ok(TaskOperation::Start)
    }

    fn block(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
        reason: &str,
    ) -> Result<TaskOperation, AtmError> {
        require_current_assignee(command, existing)?;
        Ok(TaskOperation::Block {
            reason: reason.to_owned(),
        })
    }

    fn unblock(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
        resolution: &str,
    ) -> Result<TaskOperation, AtmError> {
        require_current_assignee(command, existing)?;
        Ok(TaskOperation::Unblock {
            resolution: resolution.to_owned(),
        })
    }

    fn reassign(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
        latest_attempt: Option<&TaskAssignmentAttempt>,
        input: &AssignmentInput,
    ) -> Result<TaskOperation, AtmError> {
        require_existing_state(existing, "reassign")?;
        require_reassignment_actor(&self.runtime, command, existing, latest_attempt)?;
        Ok(TaskOperation::Reassign(self.assignment(command, input)?))
    }

    fn reopen(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
        latest_attempt: Option<&TaskAssignmentAttempt>,
        input: &AssignmentInput,
    ) -> Result<TaskOperation, AtmError> {
        require_closed(existing, "reopen")?;
        require_reassignment_actor(&self.runtime, command, existing, latest_attempt)?;
        Ok(TaskOperation::Reopen(self.assignment(command, input)?))
    }

    fn close(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
        handoff: &HandoffInput,
        outcome: TaskOutcome,
    ) -> Result<TaskOperation, AtmError> {
        require_active_assignee(command, existing, "close")?;
        let prepared_handoff = self.handoff(command, handoff, true, Some(&outcome))?;
        Ok(TaskOperation::Close {
            outcome,
            handoff: prepared_handoff,
        })
    }

    fn legacy_complete(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
        latest_attempt: Option<&TaskAssignmentAttempt>,
        notice: &LegacyCompletionNoticeInput,
    ) -> Result<TaskOperation, AtmError> {
        require_assignee_or_assigner(command, existing, latest_attempt, "legacy completion")?;
        require_assigned_or_active(existing, "legacy completion")?;
        let handoff = HandoffInput {
            recipient: notice.recipient.clone(),
            message: notice.message.clone(),
        };
        Ok(TaskOperation::LegacyCloseSucceeded {
            completion_notice: self.handoff(
                command,
                &handoff,
                false,
                Some(&TaskOutcome::Succeeded),
            )?,
        })
    }

    fn abort(
        &self,
        command: &TaskMutationCommand,
        existing: Option<&atm_storage::LogicalTaskRow>,
        latest_attempt: Option<&TaskAssignmentAttempt>,
        reason: &AbortInput,
        handoff: &HandoffInput,
    ) -> Result<TaskOperation, AtmError> {
        require_open(existing, "abort")?;
        match reason {
            AbortInput::Cancelled => {
                require_assignee_assigner_or_lead(
                    &self.runtime,
                    command,
                    existing,
                    latest_attempt,
                )?;
                let outcome = TaskOutcome::Aborted(TaskAbortReason::Cancelled);
                Ok(TaskOperation::Close {
                    handoff: self.handoff(command, handoff, true, Some(&outcome))?,
                    outcome,
                })
            }
            AbortInput::Superseded {
                successor_task_id,
                successor,
            } => {
                require_assigner_or_lead(&self.runtime, command, latest_attempt)?;
                if successor_task_id == &command.task_id {
                    return Err(task_error("a successor task must use a new task id"));
                }
                let outcome = TaskOutcome::Aborted(TaskAbortReason::Superseded {
                    successor_task_id: successor_task_id.clone(),
                });
                Ok(TaskOperation::Supersede {
                    handoff: self.handoff(command, handoff, true, Some(&outcome))?,
                    successor_task_id: successor_task_id.clone(),
                    successor: Box::new(self.assignment_for(
                        command,
                        successor_task_id,
                        successor,
                    )?),
                })
            }
        }
    }

    fn assignment(
        &self,
        command: &TaskMutationCommand,
        input: &AssignmentInput,
    ) -> Result<PreparedAssignment, AtmError> {
        self.assignment_for(command, &command.task_id, input)
    }

    fn assignment_for(
        &self,
        command: &TaskMutationCommand,
        task_id: &crate::types::TaskId,
        input: &AssignmentInput,
    ) -> Result<PreparedAssignment, AtmError> {
        require_member(&self.runtime, &input.assignee)?;
        require_same_team(&command.actor, &input.assignee, "task assignment")?;
        Ok(PreparedAssignment {
            assignee: input.assignee.clone(),
            assigner: command.actor.clone(),
            priority: input.priority,
            delivery_origin: MessageWriteOrigin::Local,
            message: prepared_message(
                task_id,
                &command.actor,
                &input.assignee,
                &input.message,
                None,
            ),
            template_sha: input.message.template_sha.clone(),
        })
    }

    fn handoff(
        &self,
        command: &TaskMutationCommand,
        input: &HandoffInput,
        canonical: bool,
        outcome: Option<&TaskOutcome>,
    ) -> Result<PreparedMessage, AtmError> {
        require_member(&self.runtime, &input.recipient)?;
        require_same_team(&command.actor, &input.recipient, "task handoff")?;
        if canonical {
            require_distinct_member(&command.actor, &input.recipient, "task handoff")?;
        }
        Ok(prepared_message(
            &command.task_id,
            &command.actor,
            &input.recipient,
            &input.message,
            outcome,
        ))
    }

    fn reader(&self) -> Result<std::sync::Arc<dyn AsyncTaskLedgerReader + Send + Sync>, AtmError> {
        self.runtime.async_task_ledger_reader()
    }

    fn mutation_store(
        &self,
    ) -> Result<std::sync::Arc<dyn AsyncTaskMutationStore + Send + Sync>, AtmError> {
        self.runtime.async_task_mutation_store()
    }
}

impl crate::boundary::sealed::Sealed for CoreTaskCommandService {}

#[async_trait]
impl TaskCommandService for CoreTaskCommandService {
    async fn execute(&self, request: TaskCommandRequest) -> Result<TaskCommandResponse, AtmError> {
        match request {
            TaskCommandRequest::List(query) => {
                self.list(query).await.map(TaskCommandResponse::List)
            }
            TaskCommandRequest::Events(query) => {
                self.events(query).await.map(TaskCommandResponse::Events)
            }
            TaskCommandRequest::Mutate(command) => self
                .mutate(command)
                .await
                .map(TaskCommandResponse::Mutation),
        }
    }
}

fn storage_scope(scope: TaskListScope) -> TaskLedgerScope {
    match scope {
        TaskListScope::Open => TaskLedgerScope::Open,
        TaskListScope::Closed => TaskLedgerScope::Closed,
    }
}

fn storage_limit(page: TaskPage) -> Option<usize> {
    match page {
        TaskPage::Bounded { limit } => Some(limit),
        TaskPage::All => None,
    }
}

fn read_deadline() -> Result<ReadDeadline, AtmError> {
    ReadDeadline::new(TASK_COMMAND_READ_DEADLINE)
}

async fn load_attempts(
    reader: &std::sync::Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
    command: &TaskMutationCommand,
    exists: bool,
) -> Result<Vec<TaskAssignmentAttempt>, AtmError> {
    if !exists {
        return Ok(Vec::new());
    }
    reader
        .list_task_assignment_attempts(
            command.actor.team().clone(),
            command.task_id.clone(),
            read_deadline()?,
        )
        .await
        .map_err(AtmError::from)
}

fn response_from_outcome(
    operation_id: atm_storage::TaskOperationId,
    prior: Option<atm_storage::LogicalTaskRow>,
    outcome: atm_storage::TaskMutationOutcome,
) -> Result<TaskMutationResponse, AtmError> {
    let current_attempt = outcome.current_attempt.ok_or_else(|| {
        task_error("stored task mutation result lacks the committed assignment attempt")
    })?;
    let current_assignee = outcome
        .current_assignee
        .ok_or_else(|| task_error("stored task mutation result lacks the committed assignee"))?;
    let terminal_outcome = match &outcome.state {
        TaskLifecycleState::Closed(value) => Some(value.clone()),
        TaskLifecycleState::Assigned | TaskLifecycleState::Active | TaskLifecycleState::Blocked => {
            None
        }
    };
    Ok(TaskMutationResponse {
        operation_id,
        task_id: outcome.task_id,
        prior_state: prior.map(|row| row.state),
        state: outcome.state,
        revision: outcome.revision,
        current_attempt,
        current_assignee,
        outcome: terminal_outcome,
        message_id: outcome.message_id,
        successor_task_id: outcome.successor_task_id,
        replayed: outcome.replayed,
    })
}

fn prepared_message(
    task_id: &crate::types::TaskId,
    actor: &atm_storage::MemberKey,
    recipient: &atm_storage::MemberKey,
    input: &ComposedMessageInput,
    terminal_outcome: Option<&TaskOutcome>,
) -> PreparedMessage {
    let (message_id, timestamp) = AtmMessageId::new_with_timestamp();
    let mut extra = Map::new();
    if let Some(outcome) = terminal_outcome {
        extra.insert(
            "taskOutcome".to_owned(),
            serde_json::Value::String(outcome_name(outcome).to_owned()),
        );
    }
    let envelope = InboxMessage {
        from: actor.agent().clone(),
        source_chat_id: None,
        text: input.body.as_str().to_owned(),
        timestamp,
        read: false,
        source_team: Some(actor.team().clone()),
        destination_chat_id: None,
        summary: Some(build_summary(input.body.as_str(), None)),
        message_id: Some(message_id),
        requires_ack: true,
        pending_ack_at: Some(timestamp),
        acknowledged_at: None,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: Some(task_id.clone()),
        task_complete: None,
        extra,
    };
    PreparedMessage {
        message: Message {
            team: recipient.team().clone(),
            agent: recipient.agent().clone(),
            message_key: MessageKey::from(message_id),
            envelope,
        },
    }
}

fn outcome_name(outcome: &TaskOutcome) -> &'static str {
    match outcome {
        TaskOutcome::Succeeded => "succeeded",
        TaskOutcome::Failed => "failed",
        TaskOutcome::Aborted(TaskAbortReason::Cancelled) => "aborted_cancelled",
        TaskOutcome::Aborted(TaskAbortReason::Superseded { .. }) => "aborted_superseded",
    }
}

fn require_member(
    runtime: &LocalServiceRuntime,
    member: &atm_storage::MemberKey,
) -> Result<(), AtmError> {
    runtime
        .roster_member(member.team(), member.agent())
        .map(|_| ())
        .ok_or_else(|| task_error("task actor or recipient is not a roster member"))
}

fn require_same_team(
    left: &atm_storage::MemberKey,
    right: &atm_storage::MemberKey,
    operation: &str,
) -> Result<(), AtmError> {
    if left.team() != right.team() {
        return Err(task_error(format!("{operation} must remain in one team")));
    }
    Ok(())
}

fn require_distinct_member(
    left: &atm_storage::MemberKey,
    right: &atm_storage::MemberKey,
    operation: &str,
) -> Result<(), AtmError> {
    require_same_team(left, right, operation)?;
    if left.agent() == right.agent() {
        return Err(task_error(format!(
            "{operation} recipient must differ from the actor"
        )));
    }
    Ok(())
}

fn require_existing_state<'task>(
    task: Option<&'task atm_storage::LogicalTaskRow>,
    operation: &str,
) -> Result<&'task atm_storage::LogicalTaskRow, AtmError> {
    task.ok_or_else(|| task_error(format!("{operation} requires an existing task")))
}

fn require_open<'task>(
    task: Option<&'task atm_storage::LogicalTaskRow>,
    operation: &str,
) -> Result<&'task atm_storage::LogicalTaskRow, AtmError> {
    let task = require_existing_state(task, operation)?;
    if matches!(task.state, TaskLifecycleState::Closed(_)) {
        return Err(task_error(format!("{operation} requires an open task")));
    }
    Ok(task)
}

fn require_closed<'task>(
    task: Option<&'task atm_storage::LogicalTaskRow>,
    operation: &str,
) -> Result<&'task atm_storage::LogicalTaskRow, AtmError> {
    let task = require_existing_state(task, operation)?;
    if !matches!(task.state, TaskLifecycleState::Closed(_)) {
        return Err(task_error(format!("{operation} requires a closed task")));
    }
    Ok(task)
}

fn require_current_assignee(
    command: &TaskMutationCommand,
    task: Option<&atm_storage::LogicalTaskRow>,
) -> Result<(), AtmError> {
    let task = require_open(task, "task mutation")?;
    if command.actor.agent() != &task.current_assignee {
        return Err(task_error("task mutation requires the current assignee"));
    }
    Ok(())
}

fn require_active_assignee(
    command: &TaskMutationCommand,
    task: Option<&atm_storage::LogicalTaskRow>,
    operation: &str,
) -> Result<(), AtmError> {
    let task = require_open(task, operation)?;
    if task.state != TaskLifecycleState::Active || command.actor.agent() != &task.current_assignee {
        return Err(task_error(format!(
            "{operation} requires the active task assignee"
        )));
    }
    Ok(())
}

fn require_assigned_or_active(
    task: Option<&atm_storage::LogicalTaskRow>,
    operation: &str,
) -> Result<(), AtmError> {
    let task = require_open(task, operation)?;
    if !matches!(
        task.state,
        TaskLifecycleState::Assigned | TaskLifecycleState::Active
    ) {
        return Err(task_error(format!(
            "{operation} requires an assigned or active task"
        )));
    }
    Ok(())
}

fn require_reassignment_actor(
    runtime: &LocalServiceRuntime,
    command: &TaskMutationCommand,
    task: Option<&atm_storage::LogicalTaskRow>,
    latest_attempt: Option<&TaskAssignmentAttempt>,
) -> Result<(), AtmError> {
    let task = require_existing_state(task, "reassign")?;
    if task.state == TaskLifecycleState::Active {
        return Err(task_error("reassign cannot interrupt active work"));
    }
    if command.actor.agent() == &task.current_assignee
        || is_current_assigner(command, latest_attempt)
    {
        return Ok(());
    }
    require_unique_lead(runtime, command)
}

fn require_assignee_or_assigner(
    command: &TaskMutationCommand,
    task: Option<&atm_storage::LogicalTaskRow>,
    latest_attempt: Option<&TaskAssignmentAttempt>,
    operation: &str,
) -> Result<(), AtmError> {
    let task = require_existing_state(task, operation)?;
    if command.actor.agent() == &task.current_assignee
        || is_current_assigner(command, latest_attempt)
    {
        return Ok(());
    }
    Err(task_error(format!(
        "{operation} requires the assignee or assigner"
    )))
}

fn require_assignee_assigner_or_lead(
    runtime: &LocalServiceRuntime,
    command: &TaskMutationCommand,
    task: Option<&atm_storage::LogicalTaskRow>,
    latest_attempt: Option<&TaskAssignmentAttempt>,
) -> Result<(), AtmError> {
    if require_assignee_or_assigner(command, task, latest_attempt, "abort").is_ok() {
        return Ok(());
    }
    require_unique_lead(runtime, command)
}

fn require_assigner_or_lead(
    runtime: &LocalServiceRuntime,
    command: &TaskMutationCommand,
    latest_attempt: Option<&TaskAssignmentAttempt>,
) -> Result<(), AtmError> {
    if is_current_assigner(command, latest_attempt) {
        return Ok(());
    }
    require_unique_lead(runtime, command)
}

fn is_current_assigner(
    command: &TaskMutationCommand,
    latest_attempt: Option<&TaskAssignmentAttempt>,
) -> bool {
    latest_attempt.is_some_and(|attempt| command.actor.agent() == &attempt.assigner)
}

fn require_unique_lead(
    runtime: &LocalServiceRuntime,
    command: &TaskMutationCommand,
) -> Result<(), AtmError> {
    let leads: Vec<_> = runtime
        .team_roster(command.actor.team())
        .into_iter()
        .filter(|member| member.agent_type == AgentType::Lead)
        .collect();
    match leads.as_slice() {
        [] => Err(AtmError::new(
            AtmErrorCode::TaskLeadMissing,
            "task lead authority is unavailable because the team has no lead",
        )),
        [lead] if lead.agent_name == *command.actor.agent() => Ok(()),
        [_] => Err(task_error("task mutation requires the team lead")),
        _ => Err(AtmError::new(
            AtmErrorCode::TaskLeadAmbiguous,
            "task lead authority is ambiguous",
        )),
    }
}

fn task_error(message: impl Into<String>) -> AtmError {
    AtmError::validation_with_recovery(
        message,
        "Inspect task history with: atm task events <task-id>",
    )
}
