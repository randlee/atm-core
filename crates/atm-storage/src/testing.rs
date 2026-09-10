//! Shared no-op test doubles for storage contract traits.
//!
//! RBQA-F002/F003: `GraftReceiverEndpointStore` no-op test doubles were
//! independently duplicated in `atm-storage`'s own test module and in
//! `atm-core`'s `ack::admission_tests`. This module is the single shared
//! implementation both consume, reached from `atm-core` via the
//! `test-utils` feature the same way `atm-runtime-test-support` and other
//! cross-crate test-only surfaces are shared in this workspace.

use chrono::{DateTime, Utc};

use crate::contract::{
    AsyncGraftReceiverEndpointStore, AsyncMailboxReader, AsyncTaskLedgerReader,
    GraftEndpointStoreError, GraftReceiverEndpointStore, GraftReceiverLease,
    GraftReceiverRegistration, MailboxScope, Message, MessageKey, MessageQuery, ReadDeadline,
    ReadLaneError, sealed,
};
use crate::task_state::{
    AssignmentAttempt, LogicalTaskRow, TaskAssignmentAttempt, TaskEventRow, TaskLedgerScope,
    TaskLifecycleEventRow, TaskLifecycleState, TaskPriority, TaskRow, TaskState,
};
use crate::types::{AgentName, IsoTimestamp, OwnerGeneration, TaskId, TeamName};

/// A `GraftReceiverEndpointStore` that accepts every write and reports no
/// lease. Used by callers that need a wired store to compile against but
/// exercise no graft-receiver behavior in the fixture under test.
#[derive(Debug, Default)]
pub struct NoopGraftReceiverEndpointStore;

impl sealed::Sealed for NoopGraftReceiverEndpointStore {}

impl GraftReceiverEndpointStore for NoopGraftReceiverEndpointStore {
    fn register(
        &self,
        _registration: &GraftReceiverRegistration,
        _now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError> {
        Ok(())
    }

    fn refresh(
        &self,
        _team: &TeamName,
        _agent: &AgentName,
        _owner_generation: &OwnerGeneration,
        _now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError> {
        Ok(())
    }

    fn unregister(
        &self,
        _team: &TeamName,
        _agent: &AgentName,
        _owner_generation: &OwnerGeneration,
    ) -> Result<(), GraftEndpointStoreError> {
        Ok(())
    }

    fn lookup(
        &self,
        _team: &TeamName,
        _agent: &AgentName,
    ) -> Result<Option<GraftReceiverLease>, GraftEndpointStoreError> {
        Ok(None)
    }

    fn mark_unreachable(
        &self,
        _team: &TeamName,
        _agent: &AgentName,
        _owner_generation: &OwnerGeneration,
        _now: DateTime<Utc>,
    ) -> Result<(), GraftEndpointStoreError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl AsyncGraftReceiverEndpointStore for NoopGraftReceiverEndpointStore {}

/// Deterministic in-memory double for the sealed async mailbox-read contract.
/// It is intentionally available only through the `test-utils` feature.
#[derive(Debug, Default)]
pub struct InMemoryMailboxReader {
    messages: std::sync::Mutex<Vec<Message>>,
    seen_watermarks:
        std::sync::Mutex<std::collections::BTreeMap<(TeamName, AgentName), IsoTimestamp>>,
}

impl InMemoryMailboxReader {
    #[must_use]
    pub fn with_messages(messages: Vec<Message>) -> Self {
        Self {
            messages: std::sync::Mutex::new(messages),
            seen_watermarks: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        }
    }
}

impl sealed::Sealed for InMemoryMailboxReader {}

#[async_trait::async_trait]
impl AsyncMailboxReader for InMemoryMailboxReader {
    async fn list_messages(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        _deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        if !scope.permits(&query) {
            return Err(ReadLaneError::UnauthorizedScope);
        }
        let messages = self
            .messages
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory mailbox reader lock poisoned".to_owned(),
            })?;
        let mut selected = messages
            .iter()
            .filter(|message| message.team == query.team && message.agent == query.agent)
            .cloned()
            .collect::<Vec<_>>();
        if let Some(limit) = query.limit {
            selected.truncate(limit);
        }
        Ok(selected)
    }

    async fn load_message(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        _deadline: ReadDeadline,
    ) -> Result<Option<Message>, ReadLaneError> {
        let messages = self
            .messages
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory mailbox reader lock poisoned".to_owned(),
            })?;
        match messages.iter().find(|message| message.message_key == key) {
            Some(message) if message.team == scope.team && message.agent == scope.agent => {
                Ok(Some(message.clone()))
            }
            Some(_) => Err(ReadLaneError::UnauthorizedScope),
            None => Ok(None),
        }
    }

    async fn mailbox_member_exists(
        &self,
        scope: MailboxScope,
        _deadline: ReadDeadline,
    ) -> Result<bool, ReadLaneError> {
        let messages = self
            .messages
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory mailbox reader lock poisoned".to_owned(),
            })?;
        Ok(messages
            .iter()
            .any(|message| message.team == scope.team && message.agent == scope.agent))
    }

    async fn load_seen_watermark(
        &self,
        scope: MailboxScope,
        _deadline: ReadDeadline,
    ) -> Result<Option<IsoTimestamp>, ReadLaneError> {
        self.seen_watermarks
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory mailbox reader seen-state lock poisoned".to_owned(),
            })
            .map(|watermarks| watermarks.get(&(scope.team, scope.agent)).copied())
    }
}

/// Deterministic in-memory double for the sealed async task-ledger read
/// contract. It is intentionally available only through `test-utils`.
#[derive(Debug, Default)]
pub struct InMemoryTaskLedgerReader {
    tasks: std::sync::Mutex<Vec<TaskRow>>,
    events: std::sync::Mutex<Vec<TaskEventRow>>,
}

/// A test-only delegating reader that pauses exactly once after reading a
/// logical task. It makes an authorization/write race reproducible while the
/// caller continues to use the production reader and mutation store.
pub struct GateAfterTaskRead {
    inner: std::sync::Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
    reached: std::sync::mpsc::Sender<()>,
    resume: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    pending_gate: std::sync::atomic::AtomicBool,
}

impl GateAfterTaskRead {
    #[must_use]
    pub fn new(
        inner: std::sync::Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
    ) -> (
        Self,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (reached, reached_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume) = std::sync::mpsc::channel();
        (
            Self {
                inner,
                reached,
                resume: std::sync::Mutex::new(resume),
                pending_gate: std::sync::atomic::AtomicBool::new(true),
            },
            reached_rx,
            resume_tx,
        )
    }
}

impl sealed::Sealed for GateAfterTaskRead {}

#[async_trait::async_trait]
impl AsyncTaskLedgerReader for GateAfterTaskRead {
    async fn list_tasks(
        &self,
        team: TeamName,
        member: Option<AgentName>,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        self.inner.list_tasks(team, member, deadline).await
    }

    async fn list_task_events(
        &self,
        team: TeamName,
        task_id: TaskId,
        member: Option<AgentName>,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskEventRow>, ReadLaneError> {
        self.inner
            .list_task_events(team, task_id, member, deadline)
            .await
    }

    async fn list_logical_tasks(
        &self,
        team: TeamName,
        member: Option<AgentName>,
        scope: TaskLedgerScope,
        limit: Option<usize>,
        deadline: ReadDeadline,
    ) -> Result<Vec<LogicalTaskRow>, ReadLaneError> {
        self.inner
            .list_logical_tasks(team, member, scope, limit, deadline)
            .await
    }

    async fn top_runnable_task(
        &self,
        team: TeamName,
        member: AgentName,
        deadline: ReadDeadline,
    ) -> Result<Option<LogicalTaskRow>, ReadLaneError> {
        self.inner.top_runnable_task(team, member, deadline).await
    }

    async fn load_logical_task(
        &self,
        team: TeamName,
        task_id: TaskId,
        deadline: ReadDeadline,
    ) -> Result<Option<LogicalTaskRow>, ReadLaneError> {
        let row = self
            .inner
            .load_logical_task(team, task_id, deadline)
            .await?;
        if self
            .pending_gate
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            self.reached
                .send(())
                .map_err(|_| ReadLaneError::Unavailable {
                    message: "task-read gate receiver dropped".to_owned(),
                })?;
            self.resume
                .lock()
                .map_err(|_| ReadLaneError::Unavailable {
                    message: "task-read gate lock poisoned".to_owned(),
                })?
                .recv()
                .map_err(|_| ReadLaneError::Unavailable {
                    message: "task-read gate resume sender dropped".to_owned(),
                })?;
        }
        Ok(row)
    }

    async fn list_task_assignment_attempts(
        &self,
        team: TeamName,
        task_id: TaskId,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskAssignmentAttempt>, ReadLaneError> {
        self.inner
            .list_task_assignment_attempts(team, task_id, deadline)
            .await
    }

    async fn list_task_lifecycle_events(
        &self,
        team: TeamName,
        task_id: TaskId,
        limit: Option<usize>,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskLifecycleEventRow>, ReadLaneError> {
        self.inner
            .list_task_lifecycle_events(team, task_id, limit, deadline)
            .await
    }
}

impl InMemoryTaskLedgerReader {
    #[must_use]
    pub fn with_rows(tasks: Vec<TaskRow>, events: Vec<TaskEventRow>) -> Self {
        Self {
            tasks: std::sync::Mutex::new(tasks),
            events: std::sync::Mutex::new(events),
        }
    }

    pub fn replace_rows(&self, tasks: Vec<TaskRow>, events: Vec<TaskEventRow>) {
        *self.tasks.lock().expect("in-memory task rows lock") = tasks;
        *self.events.lock().expect("in-memory task events lock") = events;
    }
}

impl sealed::Sealed for InMemoryTaskLedgerReader {}

#[async_trait::async_trait]
impl AsyncTaskLedgerReader for InMemoryTaskLedgerReader {
    async fn list_tasks(
        &self,
        team: TeamName,
        member: Option<AgentName>,
        _deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        self.tasks
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory task-ledger reader task lock poisoned".to_owned(),
            })
            .map(|tasks| {
                tasks
                    .iter()
                    .filter(|task| {
                        task.team == team
                            && member.as_ref().is_none_or(|agent| &task.assignee == agent)
                    })
                    .cloned()
                    .collect()
            })
    }

    async fn list_task_events(
        &self,
        team: TeamName,
        task_id: TaskId,
        member: Option<AgentName>,
        _deadline: ReadDeadline,
    ) -> Result<Vec<TaskEventRow>, ReadLaneError> {
        self.events
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory task-ledger reader event lock poisoned".to_owned(),
            })
            .map(|events| {
                events
                    .iter()
                    .filter(|event| {
                        event.team == team
                            && event.task_id == task_id
                            && member.as_ref().is_none_or(|agent| &event.assignee == agent)
                    })
                    .cloned()
                    .collect()
            })
    }

    async fn top_runnable_task(
        &self,
        team: TeamName,
        member: AgentName,
        _deadline: ReadDeadline,
    ) -> Result<Option<LogicalTaskRow>, ReadLaneError> {
        self.tasks
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory task-ledger reader task lock poisoned".to_owned(),
            })
            .map(|tasks| {
                let mut rows: Vec<_> = tasks
                    .iter()
                    .filter(|task| {
                        task.team == team
                            && task.assignee == member
                            && task.state != TaskState::Complete
                    })
                    .collect();
                rows.sort_by(|left, right| {
                    let left_rank = u8::from(left.state != TaskState::Active);
                    let right_rank = u8::from(right.state != TaskState::Active);
                    left_rank
                        .cmp(&right_rank)
                        .then_with(|| left.assigned_at.cmp(&right.assigned_at))
                        .then_with(|| left.task_id.as_str().cmp(right.task_id.as_str()))
                });
                rows.into_iter().next().map(|row| LogicalTaskRow {
                    team: row.team.clone(),
                    task_id: row.task_id.clone(),
                    current_assignee: row.assignee.clone(),
                    state: match row.state {
                        TaskState::Assigned => TaskLifecycleState::Assigned,
                        TaskState::Active => TaskLifecycleState::Active,
                        TaskState::Complete => unreachable!("closed tasks were filtered"),
                    },
                    priority: TaskPriority::Normal,
                    original_assigned_at: row.assigned_at,
                    current_attempt: AssignmentAttempt::FIRST,
                    assignment_message_id: row.assignment_message_id,
                    last_reminded_at: row.last_reminded_at,
                    reminder_ordinal: u64::from(row.reminder_count),
                    revision: 0,
                    updated_at: row.updated_at,
                })
            })
    }
}
