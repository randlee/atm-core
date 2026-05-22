use std::path::Path;
#[cfg(any(test, feature = "test-utils"))]
use std::sync::Mutex;
use std::sync::OnceLock;

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

pub fn install_default_runtime_factory(factory: DefaultRuntimeFactory) {
    #[cfg(any(test, feature = "test-utils"))]
    {
        *test_default_runtime_provider()
            .lock()
            .expect("test default runtime provider") =
            Some(DefaultRuntimeProvider::Factory(factory));
    }
    let _ = DEFAULT_RUNTIME_PROVIDER.set(DefaultRuntimeProvider::Factory(factory));
}

pub fn install_default_runtime_instance(runtime: LocalServiceRuntime) {
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
            .with_recovery(
                "Start the daemon-backed ATM runtime or install the sqlite default runtime factory before retrying this command.",
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
    ) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError>;
    fn persist_message_record(
        &self,
        record: boundary::MailStoreMessageRecord,
    ) -> Result<(), AtmError>;
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
        self.mail_store
            .query_mailbox_metadata(boundary::MailStoreQueryMailboxMetadataRequest {
                team: team.clone(),
                agent: agent.clone(),
                limit,
            })
            .map(|response| response.rows)
    }

    fn load_message_record(
        &self,
        _home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError> {
        self.mail_store
            .load_message(boundary::MailStoreLoadMessageRequest {
                team: team.clone(),
                agent: agent.clone(),
                message_key: message_key.clone(),
            })
            .map(|response| response.record)
    }

    fn persist_message_record(
        &self,
        record: boundary::MailStoreMessageRecord,
    ) -> Result<(), AtmError> {
        self.mail_store
            .upsert_message(boundary::MailStoreUpsertMessageRequest { record })
            .map(|_| ())
    }

    fn persist_message_state(&self, state: boundary::MailMessageState) -> Result<(), AtmError> {
        self.mail_store
            .upsert_message_state(boundary::UpsertMailMessageStateRequest {
                team: state.team.clone(),
                agent: state.agent.clone(),
                actor: state.actor.clone(),
                state,
            })
            .map(|_| ())
    }
}
