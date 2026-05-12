use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::boundary;
use crate::error::AtmError;
use crate::mailbox;
use crate::mailbox::source::SourceFile;
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::types::{AgentName, TeamName};
use crate::unsupported_adapters::{
    unsupported_mail_store, unsupported_roster_store, unsupported_task_store,
};

pub(crate) struct LegacyMailboxRuntime {
    inner: LocalServiceRuntime,
}

pub(crate) enum DefaultMailboxRuntime {
    Sqlite(LocalServiceRuntime),
    Legacy(LegacyMailboxRuntime),
}

type DefaultRuntimeFactory = fn() -> Result<LocalServiceRuntime, AtmError>;

static DEFAULT_RUNTIME_FACTORY: OnceLock<DefaultRuntimeFactory> = OnceLock::new();

impl std::ops::Deref for LegacyMailboxRuntime {
    type Target = LocalServiceRuntime;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub(crate) fn legacy_runtime() -> LegacyMailboxRuntime {
    LegacyMailboxRuntime {
        inner: LocalServiceRuntime::new(
            unsupported_mail_store(),
            unsupported_task_store(),
            unsupported_roster_store(),
        )
        .with_legacy_mailbox_files(),
    }
}

pub fn install_default_runtime_factory(factory: DefaultRuntimeFactory) {
    let _ = DEFAULT_RUNTIME_FACTORY.set(factory);
}

pub(crate) fn default_runtime() -> Result<DefaultMailboxRuntime, AtmError> {
    match DEFAULT_RUNTIME_FACTORY.get().copied() {
        Some(factory) => factory().map(DefaultMailboxRuntime::Sqlite),
        None => Ok(DefaultMailboxRuntime::Legacy(legacy_runtime())),
    }
}

pub(crate) trait RetainedMailboxRuntime {
    fn allows_legacy_mailbox_files(&self) -> bool;
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
    fn observe_source_files(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Vec<SourceFile>, AtmError>;
    fn commit_source_files(&self, source_files: &[SourceFile]) -> Result<(), AtmError>;
    fn read_messages(&self, path: &Path) -> Result<Vec<MessageEnvelope>, AtmError>;
    fn commit_mailbox_state(
        &self,
        path: &Path,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError>;
    fn with_locked_source_files<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: std::time::Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<T, AtmError>;
}

const LEGACY_MESSAGE_KEY_PREFIX: &str = "legacy:";

fn is_unsupported_boundary(error: &AtmError, what: &str) -> bool {
    error.is_validation() && error.message.contains(what)
}

fn encode_legacy_message_key(path: &Path, index: usize) -> Result<boundary::MessageKey, AtmError> {
    boundary::MessageKey::new(format!(
        "{LEGACY_MESSAGE_KEY_PREFIX}{index}:{}",
        path.display()
    ))
}

fn decode_legacy_message_key(message_key: &boundary::MessageKey) -> Option<(PathBuf, usize)> {
    let encoded = message_key
        .as_ref()
        .strip_prefix(LEGACY_MESSAGE_KEY_PREFIX)?;
    let (index, path) = encoded.split_once(':')?;
    Some((PathBuf::from(path), index.parse().ok()?))
}

fn legacy_query_mailbox_metadata_rows(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    limit: Option<usize>,
) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
    let source_files = observe_source_files(home_dir, team, agent)?;
    let mut rows = Vec::new();

    for source in source_files {
        for (index, envelope) in source.messages.into_iter().enumerate() {
            rows.push(boundary::MailStoreMailboxMetadataRow {
                message_key: encode_legacy_message_key(&source.path, index)?,
                message_id: envelope.message_id,
                parent_message_id: envelope.parent_message_id,
                thread_mode: envelope.thread_mode,
                from_agent: envelope.from,
                summary: envelope.summary,
                message_at: envelope.timestamp,
                read: envelope.read,
                pending_ack: envelope.pending_ack_at.is_some()
                    && envelope.acknowledged_at.is_none(),
                acknowledged_at: envelope.acknowledged_at,
                expires_at: envelope.expires_at,
                task_id: envelope.task_id,
            });
            if let Some(limit) = limit
                && rows.len() >= limit
            {
                return Ok(rows);
            }
        }
    }

    Ok(rows)
}

fn legacy_load_message_record(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    message_key: &boundary::MessageKey,
) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError> {
    let Some((path, index)) = decode_legacy_message_key(message_key) else {
        return Ok(None);
    };
    let source_files = observe_source_files(home_dir, team, agent)?;
    let envelope = source_files
        .into_iter()
        .find(|source| source.path == path)
        .and_then(|source| source.messages.into_iter().nth(index));

    Ok(envelope.map(|envelope| boundary::MailStoreMessageRecord {
        team: team.clone(),
        agent: agent.clone(),
        message_key: message_key.clone(),
        envelope,
    }))
}

pub(crate) fn observe_source_files(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceFile>, AtmError> {
    mailbox::store::observe_source_files(home_dir, team, agent)
}

pub(crate) fn commit_source_files(source_files: &[SourceFile]) -> Result<(), AtmError> {
    mailbox::store::commit_source_files(source_files)
}

pub(crate) fn commit_mailbox_state(
    path: &Path,
    messages: &[MessageEnvelope],
) -> Result<(), AtmError> {
    mailbox::store::commit_mailbox_state(path, messages)
}

pub(crate) fn with_locked_source_files<T, I, F>(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    extra_write_paths: I,
    timeout: std::time::Duration,
    body: F,
) -> Result<T, AtmError>
where
    I: IntoIterator<Item = PathBuf>,
    F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<T, AtmError>,
{
    mailbox::store::with_locked_source_files(
        home_dir,
        team,
        agent,
        extra_write_paths,
        timeout,
        body,
    )
}

impl RetainedMailboxRuntime for LocalServiceRuntime {
    fn allows_legacy_mailbox_files(&self) -> bool {
        self.allows_legacy_mailbox_files()
    }

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
        match self
            .mail_store
            .upsert_message(boundary::MailStoreUpsertMessageRequest { record })
        {
            Ok(_) => Ok(()),
            Err(error) if is_unsupported_boundary(&error, "MailStore::upsert_message") => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn persist_message_state(&self, state: boundary::MailMessageState) -> Result<(), AtmError> {
        match self
            .mail_store
            .upsert_message_state(boundary::UpsertMailMessageStateRequest {
                team: state.team.clone(),
                agent: state.agent.clone(),
                actor: state.actor.clone(),
                state,
            }) {
            Ok(_) => Ok(()),
            Err(error)
                if self.allows_legacy_mailbox_files()
                    && is_unsupported_boundary(&error, "MailStore::upsert_message_state") =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn observe_source_files(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Vec<SourceFile>, AtmError> {
        // The retained runtime still fronts the legacy file-backed mailbox
        // path; keep the declared boundary adapter attached until the mailbox
        // persistence path is fully lifted behind the store boundary.
        let _ = self.mail_store.as_ref();
        observe_source_files(home_dir, team, agent)
    }

    fn commit_source_files(&self, source_files: &[SourceFile]) -> Result<(), AtmError> {
        let _ = self.mail_store.as_ref();
        commit_source_files(source_files)
    }

    fn read_messages(&self, path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
        let _ = self.mail_store.as_ref();
        crate::mailbox::read_messages(path)
    }

    fn commit_mailbox_state(
        &self,
        path: &Path,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError> {
        let _ = self.mail_store.as_ref();
        commit_mailbox_state(path, messages)
    }

    fn with_locked_source_files<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: std::time::Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<T, AtmError>,
    {
        let _ = self.mail_store.as_ref();
        with_locked_source_files(home_dir, team, agent, extra_write_paths, timeout, body)
    }
}

impl RetainedServiceRuntime for LegacyMailboxRuntime {
    fn load_config(
        &self,
        current_dir: &Path,
    ) -> Result<Option<crate::config::AtmConfig>, AtmError> {
        self.inner.load_config(current_dir)
    }

    fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError> {
        self.inner.load_team_config(team_dir)
    }

    fn team_dir(&self, home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
        self.inner.team_dir(home_dir, team)
    }

    fn inbox_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError> {
        self.inner.inbox_path(home_dir, team, agent)
    }

    fn workflow_state_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError> {
        self.inner.workflow_state_path(home_dir, team, agent)
    }

    fn load_workflow_state(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<crate::workflow::WorkflowStateFile, AtmError> {
        self.inner.load_workflow_state(home_dir, team, agent)
    }

    fn save_workflow_state(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        state: &crate::workflow::WorkflowStateFile,
    ) -> Result<(), AtmError> {
        self.inner.save_workflow_state(home_dir, team, agent, state)
    }

    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::types::IsoTimestamp>, AtmError> {
        self.inner.load_seen_watermark(home_dir, team, agent)
    }

    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        timestamp: crate::types::IsoTimestamp,
    ) -> Result<(), AtmError> {
        self.inner
            .save_seen_watermark(home_dir, team, agent, timestamp)
    }

    fn mailbox_timeout_policy(&self) -> crate::service_runtime::RetainedMailboxTimeoutPolicy {
        self.inner.mailbox_timeout_policy()
    }

    fn maybe_run_post_send_hook(
        &self,
        warnings: &mut Vec<crate::send::WarningEntry>,
        config: Option<&crate::config::AtmConfig>,
        context: crate::send::PostSendHookContext<'_>,
    ) {
        self.inner
            .maybe_run_post_send_hook(warnings, config, context)
    }

    fn commit_workflow_state<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: std::time::Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&mut crate::workflow::WorkflowStateFile) -> Result<(T, bool), AtmError>,
    {
        self.inner
            .commit_workflow_state(home_dir, team, agent, extra_write_paths, timeout, body)
    }
}

impl RetainedServiceRuntime for DefaultMailboxRuntime {
    fn load_config(
        &self,
        current_dir: &Path,
    ) -> Result<Option<crate::config::AtmConfig>, AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.load_config(current_dir),
            Self::Legacy(runtime) => runtime.load_config(current_dir),
        }
    }

    fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.load_team_config(team_dir),
            Self::Legacy(runtime) => runtime.load_team_config(team_dir),
        }
    }

    fn team_dir(&self, home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.team_dir(home_dir, team),
            Self::Legacy(runtime) => runtime.team_dir(home_dir, team),
        }
    }

    fn inbox_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.inbox_path(home_dir, team, agent),
            Self::Legacy(runtime) => runtime.inbox_path(home_dir, team, agent),
        }
    }

    fn workflow_state_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.workflow_state_path(home_dir, team, agent),
            Self::Legacy(runtime) => runtime.workflow_state_path(home_dir, team, agent),
        }
    }

    fn load_workflow_state(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<crate::workflow::WorkflowStateFile, AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.load_workflow_state(home_dir, team, agent),
            Self::Legacy(runtime) => runtime.load_workflow_state(home_dir, team, agent),
        }
    }

    fn save_workflow_state(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        state: &crate::workflow::WorkflowStateFile,
    ) -> Result<(), AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.save_workflow_state(home_dir, team, agent, state),
            Self::Legacy(runtime) => runtime.save_workflow_state(home_dir, team, agent, state),
        }
    }

    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::types::IsoTimestamp>, AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.load_seen_watermark(home_dir, team, agent),
            Self::Legacy(runtime) => runtime.load_seen_watermark(home_dir, team, agent),
        }
    }

    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        timestamp: crate::types::IsoTimestamp,
    ) -> Result<(), AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.save_seen_watermark(home_dir, team, agent, timestamp),
            Self::Legacy(runtime) => runtime.save_seen_watermark(home_dir, team, agent, timestamp),
        }
    }

    fn mailbox_timeout_policy(&self) -> crate::service_runtime::RetainedMailboxTimeoutPolicy {
        match self {
            Self::Sqlite(runtime) => runtime.mailbox_timeout_policy(),
            Self::Legacy(runtime) => runtime.mailbox_timeout_policy(),
        }
    }

    fn maybe_run_post_send_hook(
        &self,
        warnings: &mut Vec<crate::send::WarningEntry>,
        config: Option<&crate::config::AtmConfig>,
        context: crate::send::PostSendHookContext<'_>,
    ) {
        match self {
            Self::Sqlite(runtime) => runtime.maybe_run_post_send_hook(warnings, config, context),
            Self::Legacy(runtime) => runtime.maybe_run_post_send_hook(warnings, config, context),
        }
    }

    fn commit_workflow_state<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: std::time::Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&mut crate::workflow::WorkflowStateFile) -> Result<(T, bool), AtmError>,
    {
        match self {
            Self::Sqlite(runtime) => runtime.commit_workflow_state(
                home_dir,
                team,
                agent,
                extra_write_paths,
                timeout,
                body,
            ),
            Self::Legacy(runtime) => runtime.commit_workflow_state(
                home_dir,
                team,
                agent,
                extra_write_paths,
                timeout,
                body,
            ),
        }
    }
}

impl RetainedMailboxRuntime for LegacyMailboxRuntime {
    fn allows_legacy_mailbox_files(&self) -> bool {
        true
    }

    fn query_mailbox_metadata_rows(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
        legacy_query_mailbox_metadata_rows(home_dir, team, agent, limit)
    }

    fn load_message_record(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError> {
        legacy_load_message_record(home_dir, team, agent, message_key)
    }

    fn persist_message_record(
        &self,
        record: boundary::MailStoreMessageRecord,
    ) -> Result<(), AtmError> {
        self.inner.persist_message_record(record)
    }

    fn persist_message_state(&self, state: boundary::MailMessageState) -> Result<(), AtmError> {
        self.inner.persist_message_state(state)
    }

    fn observe_source_files(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Vec<SourceFile>, AtmError> {
        observe_source_files(home_dir, team, agent)
    }

    fn commit_source_files(&self, source_files: &[SourceFile]) -> Result<(), AtmError> {
        commit_source_files(source_files)
    }

    fn read_messages(&self, path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
        crate::mailbox::read_messages(path)
    }

    fn commit_mailbox_state(
        &self,
        path: &Path,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError> {
        commit_mailbox_state(path, messages)
    }

    fn with_locked_source_files<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: std::time::Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<T, AtmError>,
    {
        with_locked_source_files(home_dir, team, agent, extra_write_paths, timeout, body)
    }
}

impl RetainedMailboxRuntime for DefaultMailboxRuntime {
    fn allows_legacy_mailbox_files(&self) -> bool {
        match self {
            Self::Sqlite(runtime) => runtime.allows_legacy_mailbox_files(),
            Self::Legacy(runtime) => runtime.allows_legacy_mailbox_files(),
        }
    }

    fn query_mailbox_metadata_rows(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
        match self {
            Self::Sqlite(runtime) => {
                runtime.query_mailbox_metadata_rows(home_dir, team, agent, limit)
            }
            Self::Legacy(runtime) => {
                runtime.query_mailbox_metadata_rows(home_dir, team, agent, limit)
            }
        }
    }

    fn load_message_record(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError> {
        match self {
            Self::Sqlite(runtime) => {
                runtime.load_message_record(home_dir, team, agent, message_key)
            }
            Self::Legacy(runtime) => {
                runtime.load_message_record(home_dir, team, agent, message_key)
            }
        }
    }

    fn persist_message_record(
        &self,
        record: boundary::MailStoreMessageRecord,
    ) -> Result<(), AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.persist_message_record(record),
            Self::Legacy(runtime) => runtime.persist_message_record(record),
        }
    }

    fn persist_message_state(&self, state: boundary::MailMessageState) -> Result<(), AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.persist_message_state(state),
            Self::Legacy(runtime) => runtime.persist_message_state(state),
        }
    }

    fn observe_source_files(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Vec<SourceFile>, AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.observe_source_files(home_dir, team, agent),
            Self::Legacy(runtime) => runtime.observe_source_files(home_dir, team, agent),
        }
    }

    fn commit_source_files(&self, source_files: &[SourceFile]) -> Result<(), AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.commit_source_files(source_files),
            Self::Legacy(runtime) => runtime.commit_source_files(source_files),
        }
    }

    fn read_messages(&self, path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.read_messages(path),
            Self::Legacy(runtime) => runtime.read_messages(path),
        }
    }

    fn commit_mailbox_state(
        &self,
        path: &Path,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError> {
        match self {
            Self::Sqlite(runtime) => runtime.commit_mailbox_state(path, messages),
            Self::Legacy(runtime) => runtime.commit_mailbox_state(path, messages),
        }
    }

    fn with_locked_source_files<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: std::time::Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<T, AtmError>,
    {
        match self {
            Self::Sqlite(runtime) => runtime.with_locked_source_files(
                home_dir,
                team,
                agent,
                extra_write_paths,
                timeout,
                body,
            ),
            Self::Legacy(runtime) => runtime.with_locked_source_files(
                home_dir,
                team,
                agent,
                extra_write_paths,
                timeout,
                body,
            ),
        }
    }
}
