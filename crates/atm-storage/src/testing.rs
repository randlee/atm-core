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
    AsyncMailboxReader, GraftEndpointStoreError, GraftReceiverEndpointStore, GraftReceiverLease,
    GraftReceiverRegistration, MailboxScope, Message, MessageKey, MessageQuery, ReadDeadline,
    ReadLaneError, sealed,
};
use crate::types::{AgentName, IsoTimestamp, OwnerGeneration, TeamName};

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
