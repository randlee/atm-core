#![allow(
    deprecated,
    reason = "the retained runtime bridge still consumes the transitional shared storage traits until the direct boundary fully replaces it"
)]

use std::path::Path;
#[cfg(any(test, feature = "test-utils"))]
use std::sync::Mutex;
use std::sync::OnceLock;

use atm_storage::{
    AckRequirementState, Message as SharedMessage, MessageQuery, derive_ack_requirement,
};

use crate::boundary;
use crate::error::AtmError;
use crate::service_runtime::LocalServiceRuntime;
use crate::types::{AgentName, TeamName};

type DefaultRuntimeFactory = fn() -> Result<LocalServiceRuntime, AtmError>;

#[derive(Clone)]
enum DefaultRuntimeProvider {
    Factory(DefaultRuntimeFactory),
    Instance(LocalServiceRuntime),
}

static DEFAULT_RUNTIME_PROVIDER: OnceLock<DefaultRuntimeProvider> = OnceLock::new();
#[cfg(any(test, feature = "test-utils"))]
static TEST_DEFAULT_RUNTIME_PROVIDER: OnceLock<Mutex<Option<DefaultRuntimeProvider>>> =
    OnceLock::new();

#[cfg(any(test, feature = "test-utils"))]
fn test_default_runtime_provider() -> &'static Mutex<Option<DefaultRuntimeProvider>> {
    TEST_DEFAULT_RUNTIME_PROVIDER.get_or_init(|| Mutex::new(None))
}

pub(crate) fn install_default_runtime_factory(factory: DefaultRuntimeFactory) {
    #[cfg(any(test, feature = "test-utils"))]
    {
        *test_default_runtime_provider()
            .lock()
            .expect("test default runtime provider") =
            Some(DefaultRuntimeProvider::Factory(factory));
    }
    let _ = DEFAULT_RUNTIME_PROVIDER.set(DefaultRuntimeProvider::Factory(factory));
}

pub(crate) fn install_default_runtime_instance(runtime: LocalServiceRuntime) {
    let _ = DEFAULT_RUNTIME_PROVIDER.set(DefaultRuntimeProvider::Instance(runtime));
}

pub(crate) fn default_runtime() -> Result<LocalServiceRuntime, AtmError> {
    #[cfg(any(test, feature = "test-utils"))]
    if let Some(provider) = test_default_runtime_provider()
        .lock()
        .expect("test default runtime provider")
        .clone()
    {
        return match provider {
            DefaultRuntimeProvider::Factory(factory) => factory(),
            DefaultRuntimeProvider::Instance(runtime) => Ok(runtime),
        };
    }

    DEFAULT_RUNTIME_PROVIDER
        .get()
        .cloned()
        .ok_or_else(|| {
            AtmError::daemon_unavailable(
                "sqlite-backed retained runtime is unavailable because no default runtime factory is installed",
            )

        })
        .and_then(|provider| match provider {
            DefaultRuntimeProvider::Factory(factory) => factory(),
            DefaultRuntimeProvider::Instance(runtime) => Ok(runtime),
        })
}

pub(crate) trait RetainedMailboxRuntime {
    fn query_mailbox_metadata_rows(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError>;
    fn load_message_record(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<Option<boundary::Message>, AtmError>;
    fn persist_message_record(&self, record: boundary::Message) -> Result<(), AtmError>;
    fn persist_message_state(&self, state: boundary::MailMessageState) -> Result<(), AtmError>;
}

impl RetainedMailboxRuntime for LocalServiceRuntime {
    fn query_mailbox_metadata_rows(
        &self,
        _home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
        self.message_store
            .list_messages(&MessageQuery {
                team: team.clone(),
                agent: agent.clone(),
                sender: None,
                task_id: None,
                limit,
            })
            .map(|messages| {
                messages
                    .into_iter()
                    .map(shared_message_to_metadata_row)
                    .collect()
            })
    }

    fn load_message_record(
        &self,
        _home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<Option<boundary::Message>, AtmError> {
        Ok(self
            .message_store
            .load_message(message_key)?
            .filter(|message| &message.team == team && &message.agent == agent)
            .map(shared_message_to_record))
    }

    fn persist_message_record(&self, record: boundary::Message) -> Result<(), AtmError> {
        self.message_store.save_message(&SharedMessage {
            team: record.team,
            agent: record.agent,
            message_key: record.message_key,
            envelope: record.envelope,
        })
    }

    fn persist_message_state(&self, state: boundary::MailMessageState) -> Result<(), AtmError> {
        if state.deleted_at.is_some() {
            return self.message_store.delete_message(&state.message_key);
        }

        let mut message = self
            .message_store
            .load_message(&state.message_key)?
            .filter(|message| message.team == state.team && message.agent == state.agent)
            .ok_or_else(|| {
                AtmError::mailbox_read(format!(
                    "message {} was not found for {}@{} while updating mailbox state",
                    state.message_key.as_ref(),
                    state.agent.as_str(),
                    state.team.as_str(),
                ))
            })?;
        message.envelope.read = state.read;
        message.envelope.pending_ack_at = state.pending_ack_at;
        message.envelope.acknowledged_at = state.acknowledged_at;
        message.envelope.expires_at = state.expires_at;
        self.message_store.save_message(&message)
    }
}

fn shared_message_to_record(message: SharedMessage) -> boundary::Message {
    boundary::Message {
        team: message.team,
        agent: message.agent,
        message_key: message.message_key,
        envelope: message.envelope,
    }
}

fn shared_message_to_metadata_row(message: SharedMessage) -> boundary::MailStoreMailboxMetadataRow {
    let ack_requirement = derive_ack_requirement(&message.envelope);
    boundary::MailStoreMailboxMetadataRow {
        message_key: message.message_key.clone(),
        message_id: message.envelope.message_id,
        parent_message_id: message.envelope.parent_message_id,
        thread_mode: message.envelope.thread_mode,
        from_agent: message.envelope.from,
        source_chat_id: message.envelope.source_chat_id,
        destination_chat_id: message.envelope.destination_chat_id,
        summary: message.envelope.summary,
        message_at: message.envelope.timestamp,
        read: message.envelope.read,
        requires_ack: !matches!(ack_requirement, AckRequirementState::NotRequired),
        pending_ack: matches!(ack_requirement, AckRequirementState::RequiredPending),
        acknowledged_at: message.envelope.acknowledged_at,
        expires_at: message.envelope.expires_at,
        task_id: message.envelope.task_id,
    }
}
