//! Shared no-op test doubles for storage contract traits.
//!
//! RBQA-F002/F003: `GraftReceiverEndpointStore` no-op test doubles were
//! independently duplicated in `atm-storage`'s own test module and in
//! `atm-core`'s `ack::admission_tests`. This module is the single shared
//! implementation both consume, reached from `atm-core` via the
//! `test-utils` feature the same way `atm-runtime-test-support` and other
//! cross-crate test-only surfaces are shared in this workspace.

use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::contract::{
    AsyncGraftReceiverEndpointStore, AsyncMailboxReader, AsyncTaskLedgerReader,
    GraftEndpointStoreError, GraftReceiverEndpointStore, GraftReceiverLease,
    GraftReceiverRegistration, MailboxScope, Message, MessageKey, MessageQuery, ReadDeadline,
    ReadLaneError, sealed,
};
use crate::task_state::{TaskEventRow, TaskRow};
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

/// Delegating mailbox reader that counts suppression-list reads.
///
/// Escalation runtime tests use this to prove that restart recovery performs
/// one durable duplicate check per target, without opening a second reader
/// implementation outside the crate that owns the sealed boundary.
pub struct CountingMailboxReader {
    inner: Arc<dyn AsyncMailboxReader + Send + Sync>,
    list_calls: AtomicUsize,
}

impl CountingMailboxReader {
    #[must_use]
    pub fn new(inner: Arc<dyn AsyncMailboxReader + Send + Sync>) -> Self {
        Self {
            inner,
            list_calls: AtomicUsize::new(0),
        }
    }

    #[must_use]
    pub fn list_call_count(&self) -> usize {
        self.list_calls.load(Ordering::SeqCst)
    }
}

impl sealed::Sealed for CountingMailboxReader {}

#[async_trait::async_trait]
impl AsyncMailboxReader for CountingMailboxReader {
    async fn list_messages(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.list_messages(scope, query, deadline).await
    }

    async fn load_message(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: ReadDeadline,
    ) -> Result<Option<Message>, ReadLaneError> {
        self.inner.load_message(scope, key, deadline).await
    }

    async fn mailbox_member_exists(
        &self,
        scope: MailboxScope,
        deadline: ReadDeadline,
    ) -> Result<bool, ReadLaneError> {
        self.inner.mailbox_member_exists(scope, deadline).await
    }

    async fn load_seen_watermark(
        &self,
        scope: MailboxScope,
        deadline: ReadDeadline,
    ) -> Result<Option<IsoTimestamp>, ReadLaneError> {
        self.inner.load_seen_watermark(scope, deadline).await
    }
}

/// Deterministic in-memory double for the sealed async task-ledger read
/// contract. It is intentionally available only through `test-utils`.
#[derive(Debug, Default)]
pub struct InMemoryTaskLedgerReader {
    tasks: std::sync::Mutex<Vec<TaskRow>>,
    events: std::sync::Mutex<Vec<TaskEventRow>>,
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
    async fn open_tasks_for_team(
        &self,
        team: TeamName,
        _deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        let mut rows = self
            .tasks
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory task-ledger reader task lock poisoned".to_owned(),
            })?
            .iter()
            .filter(|task| task.team == team && task.state.is_open())
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            (
                &left.assignee,
                left.position,
                left.assigned_at,
                &left.task_id,
            )
                .cmp(&(
                    &right.assignee,
                    right.position,
                    right.assigned_at,
                    &right.task_id,
                ))
        });
        Ok(rows)
    }

    async fn refusal_run(
        &self,
        team: TeamName,
        assignee: AgentName,
        _deadline: ReadDeadline,
    ) -> Result<crate::RefusalRun, ReadLaneError> {
        let events = self.events.lock().map_err(|_| ReadLaneError::Unavailable {
            message: "in-memory task-ledger reader event lock poisoned".to_owned(),
        })?;
        let mut relevant = events
            .iter()
            .filter(|event| event.team == team && event.assignee == assignee)
            .filter(|event| {
                matches!(
                    event.event,
                    crate::TaskEventKind::Assigned
                        | crate::TaskEventKind::Reassigned
                        | crate::TaskEventKind::Reopened
                        | crate::TaskEventKind::Completed
                        | crate::TaskEventKind::Refused
                        | crate::TaskEventKind::Cancelled
                )
            })
            .collect::<Vec<_>>();
        relevant.sort_by_key(|event| (event.at, event.seq));
        let refused = relevant
            .iter()
            .rev()
            .take_while(|event| event.event == crate::TaskEventKind::Refused)
            .copied()
            .collect::<Vec<_>>();
        Ok(crate::RefusalRun {
            count: u32::try_from(refused.len()).unwrap_or(u32::MAX),
            started_at: refused.last().map(|event| event.at),
        })
    }

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
}
