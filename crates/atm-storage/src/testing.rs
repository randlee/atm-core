//! Shared configurable test doubles for storage contract traits.
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
    GraftReceiverRegistration, MailboxScope, Message, MessageKey, MessageQuery, NudgeClaim,
    PendingNudgeStore, ReadDeadline, ReadLaneError, sealed,
};
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::task_state::{TaskEventRow, TaskRow};
use crate::types::{AgentName, IsoTimestamp, MemberKey, OwnerGeneration, TaskId, TeamName};

pub use crate::contract::DummyPendingNudgeStore;

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
pub struct InMemoryMailboxReader {
    messages: std::sync::Mutex<Vec<Message>>,
    seen_watermarks:
        std::sync::Mutex<std::collections::BTreeMap<(TeamName, AgentName), IsoTimestamp>>,
    delegate: Option<Arc<dyn AsyncMailboxReader + Send + Sync>>,
    list_calls: AtomicUsize,
}

impl Default for InMemoryMailboxReader {
    fn default() -> Self {
        Self {
            messages: std::sync::Mutex::new(Vec::new()),
            seen_watermarks: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            delegate: None,
            list_calls: AtomicUsize::new(0),
        }
    }
}

impl InMemoryMailboxReader {
    #[must_use]
    pub fn with_messages(messages: Vec<Message>) -> Self {
        Self {
            messages: std::sync::Mutex::new(messages),
            seen_watermarks: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            delegate: None,
            list_calls: AtomicUsize::new(0),
        }
    }

    #[must_use]
    pub fn delegating(inner: Arc<dyn AsyncMailboxReader + Send + Sync>) -> Self {
        Self {
            delegate: Some(inner),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn list_call_count(&self) -> usize {
        self.list_calls.load(Ordering::SeqCst)
    }
}

impl sealed::Sealed for InMemoryMailboxReader {}

#[async_trait::async_trait]
impl AsyncMailboxReader for InMemoryMailboxReader {
    async fn list_messages(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(delegate) = &self.delegate {
            return delegate.list_messages(scope, query, deadline).await;
        }
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
        deadline: ReadDeadline,
    ) -> Result<Option<Message>, ReadLaneError> {
        if let Some(delegate) = &self.delegate {
            return delegate.load_message(scope, key, deadline).await;
        }
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
        deadline: ReadDeadline,
    ) -> Result<bool, ReadLaneError> {
        if let Some(delegate) = &self.delegate {
            return delegate.mailbox_member_exists(scope, deadline).await;
        }
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
        deadline: ReadDeadline,
    ) -> Result<Option<IsoTimestamp>, ReadLaneError> {
        if let Some(delegate) = &self.delegate {
            return delegate.load_seen_watermark(scope, deadline).await;
        }
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
pub struct InMemoryTaskLedgerReader {
    tasks: std::sync::Mutex<Vec<TaskRow>>,
    events: std::sync::Mutex<Vec<TaskEventRow>>,
    delegate: Option<Arc<dyn AsyncTaskLedgerReader + Send + Sync>>,
    open_tasks_hook: std::sync::Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    refusal_error: std::sync::Mutex<Option<ReadLaneError>>,
}

impl Default for InMemoryTaskLedgerReader {
    fn default() -> Self {
        Self {
            tasks: std::sync::Mutex::new(Vec::new()),
            events: std::sync::Mutex::new(Vec::new()),
            delegate: None,
            open_tasks_hook: std::sync::Mutex::new(None),
            refusal_error: std::sync::Mutex::new(None),
        }
    }
}

impl InMemoryTaskLedgerReader {
    #[must_use]
    pub fn with_rows(tasks: Vec<TaskRow>, events: Vec<TaskEventRow>) -> Self {
        Self {
            tasks: std::sync::Mutex::new(tasks),
            events: std::sync::Mutex::new(events),
            delegate: None,
            open_tasks_hook: std::sync::Mutex::new(None),
            refusal_error: std::sync::Mutex::new(None),
        }
    }

    #[must_use]
    pub fn delegating_with_open_tasks_hook(
        inner: Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
        hook: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            delegate: Some(inner),
            open_tasks_hook: std::sync::Mutex::new(Some(Box::new(hook))),
            ..Self::default()
        }
    }

    pub fn replace_rows(&self, tasks: Vec<TaskRow>, events: Vec<TaskEventRow>) {
        *self.tasks.lock().expect("in-memory task rows lock") = tasks;
        *self.events.lock().expect("in-memory task events lock") = events;
    }

    /// Injects a refusal-history read outcome without affecting the other
    /// task-ledger reads exercised by a runtime invariant test.
    #[must_use]
    pub fn with_refusal_error(self, error: ReadLaneError) -> Self {
        *self
            .refusal_error
            .lock()
            .expect("in-memory refusal error lock") = Some(error);
        self
    }
}

impl sealed::Sealed for InMemoryTaskLedgerReader {}

#[async_trait::async_trait]
impl AsyncTaskLedgerReader for InMemoryTaskLedgerReader {
    async fn load_task(
        &self,
        team: TeamName,
        task_id: TaskId,
        deadline: ReadDeadline,
    ) -> Result<Option<TaskRow>, ReadLaneError> {
        if let Some(delegate) = &self.delegate {
            return delegate.load_task(team, task_id, deadline).await;
        }
        Ok(self
            .tasks
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory task-ledger reader task lock poisoned".to_owned(),
            })?
            .iter()
            .find(|task| task.team == team && task.task_id == task_id)
            .cloned())
    }

    async fn open_tasks_for_team(
        &self,
        team: TeamName,
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        let hook = self
            .open_tasks_hook
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory task-ledger hook lock poisoned".to_owned(),
            })?
            .take();
        if let Some(hook) = hook {
            hook();
        }
        if let Some(delegate) = &self.delegate {
            return delegate.open_tasks_for_team(team, deadline).await;
        }
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
        deadline: ReadDeadline,
    ) -> Result<crate::RefusalRun, ReadLaneError> {
        if let Some(error) = self
            .refusal_error
            .lock()
            .map_err(|_| ReadLaneError::Unavailable {
                message: "in-memory refusal error lock poisoned".to_owned(),
            })?
            .clone()
        {
            return Err(error);
        }
        if let Some(delegate) = &self.delegate {
            return delegate.refusal_run(team, assignee, deadline).await;
        }
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
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskRow>, ReadLaneError> {
        if let Some(delegate) = &self.delegate {
            return delegate.list_tasks(team, member, deadline).await;
        }
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
        deadline: ReadDeadline,
    ) -> Result<Vec<TaskEventRow>, ReadLaneError> {
        if let Some(delegate) = &self.delegate {
            return delegate
                .list_task_events(team, task_id, member, deadline)
                .await;
        }
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

pub(crate) struct PendingStoreState {
    inner: Option<std::sync::Arc<dyn PendingNudgeStore + Send + Sync>>,
    mark_failure: Option<AtmError>,
    mark_failures_remaining: std::sync::atomic::AtomicUsize,
    rearm_failure: Option<AtmError>,
    release_started: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    release_blocker: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    operation_calls: std::sync::atomic::AtomicUsize,
    rearm_calls: std::sync::atomic::AtomicUsize,
    mark_pending_calls: std::sync::Mutex<Vec<(MemberKey, AtmMessageId)>>,
}

impl Default for DummyPendingNudgeStore {
    fn default() -> Self {
        Self(PendingStoreState {
            inner: None,
            mark_failure: None,
            mark_failures_remaining: std::sync::atomic::AtomicUsize::new(0),
            rearm_failure: None,
            release_started: None,
            release_blocker: None,
            operation_calls: std::sync::atomic::AtomicUsize::new(0),
            rearm_calls: std::sync::atomic::AtomicUsize::new(0),
            mark_pending_calls: std::sync::Mutex::new(Vec::new()),
        })
    }
}

impl DummyPendingNudgeStore {
    #[must_use]
    pub fn delegating(inner: std::sync::Arc<dyn PendingNudgeStore + Send + Sync>) -> Self {
        Self(PendingStoreState {
            inner: Some(inner),
            ..Self::default().0
        })
    }

    #[must_use]
    pub fn with_mark_failure(mut self, failure: AtmError, count: usize) -> Self {
        self.0.mark_failure = Some(failure);
        self.0.mark_failures_remaining = std::sync::atomic::AtomicUsize::new(count);
        self
    }

    #[must_use]
    pub fn with_rearm_failure(mut self, failure: AtmError) -> Self {
        self.0.rearm_failure = Some(failure);
        self
    }

    /// Blocks release operations until `blocker` becomes false, exposing the
    /// synchronous-store stall needed by bounded-shutdown tests.
    #[must_use]
    pub fn with_release_blocker(
        mut self,
        started: std::sync::Arc<std::sync::atomic::AtomicBool>,
        blocker: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        self.0.release_started = Some(started);
        self.0.release_blocker = Some(blocker);
        self
    }

    #[must_use]
    pub fn mark_pending_call_count(&self) -> usize {
        self.0
            .mark_pending_calls
            .lock()
            .expect("pending-nudge mark call lock")
            .len()
    }

    #[must_use]
    pub fn operation_call_count(&self) -> usize {
        self.0
            .operation_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    #[must_use]
    pub fn rearm_call_count(&self) -> usize {
        self.0.rearm_calls.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn record_operation(&self) {
        self.0
            .operation_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn next_mark_failure(&self) -> Option<AtmError> {
        let previous = self
            .0
            .mark_failures_remaining
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |remaining| remaining.checked_sub(1),
            )
            .unwrap_or(0);
        (previous > 0)
            .then(|| self.0.mark_failure.clone())
            .flatten()
    }

    pub(crate) fn mark(
        &self,
        member: &MemberKey,
        msg: &AtmMessageId,
        at: IsoTimestamp,
    ) -> Result<bool, AtmError> {
        self.record_operation();
        self.0
            .mark_pending_calls
            .lock()
            .expect("pending-nudge mark call lock")
            .push((member.clone(), *msg));
        if let Some(failure) = self.next_mark_failure() {
            return Err(failure);
        }
        self.0
            .inner
            .as_ref()
            .map_or(Ok(true), |inner| inner.mark_pending(member, msg, at))
    }

    pub(crate) fn claim(&self, member: &MemberKey) -> Result<Option<NudgeClaim>, AtmError> {
        self.record_operation();
        self.0
            .inner
            .as_ref()
            .map_or(Ok(None), |inner| inner.claim_next_pending(member))
    }

    pub(crate) fn requeue(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
        self.record_operation();
        self.0
            .inner
            .as_ref()
            .map_or(Ok(()), |inner| inner.requeue_pending(member, claim))
    }

    pub(crate) fn release(&self, member: &MemberKey, claim: &NudgeClaim) -> Result<(), AtmError> {
        self.record_operation();
        if let Some(started) = &self.0.release_started {
            started.store(true, std::sync::atomic::Ordering::Release);
        }
        if let Some(blocker) = &self.0.release_blocker {
            while blocker.load(std::sync::atomic::Ordering::Acquire) {
                std::thread::yield_now();
            }
        }
        self.0
            .inner
            .as_ref()
            .map_or(Ok(()), |inner| inner.release_pending(member, claim))
    }

    pub(crate) fn rearm_failure(&self) -> Option<AtmError> {
        self.record_operation();
        self.0
            .rearm_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.0.rearm_failure.clone()
    }

    pub(crate) fn inner(&self) -> Option<&std::sync::Arc<dyn PendingNudgeStore + Send + Sync>> {
        self.0.inner.as_ref()
    }

    pub(crate) fn list(&self) -> Result<Vec<MemberKey>, AtmError> {
        self.record_operation();
        self.0
            .inner
            .as_ref()
            .map_or_else(|| Ok(Vec::new()), |inner| inner.list_pending_members())
    }
}
