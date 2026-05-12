use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::boundary;
use crate::error::AtmError;
use crate::mailbox;
use crate::mailbox::source::SourceFile;
use crate::schema::MessageEnvelope;
use crate::schema::TeamConfig;
use crate::service_runtime::LocalServiceRuntime;
use crate::types::{AgentName, TeamName};

#[derive(Debug, Default)]
struct LegacyMailStoreAdapter;

#[derive(Debug, Default)]
struct LegacyTaskStoreAdapter;

#[derive(Debug, Default)]
struct LegacyRosterStoreAdapter;

pub(crate) fn default_mail_store() -> Arc<dyn boundary::MailStore + Send + Sync> {
    Arc::new(LegacyMailStoreAdapter)
}

pub(crate) fn default_task_store() -> Arc<dyn boundary::TaskStore + Send + Sync> {
    Arc::new(LegacyTaskStoreAdapter)
}

pub(crate) fn default_roster_store() -> Arc<dyn boundary::RosterStore + Send + Sync> {
    Arc::new(LegacyRosterStoreAdapter)
}

pub(crate) fn legacy_runtime() -> LocalServiceRuntime {
    LocalServiceRuntime::new(
        default_mail_store(),
        default_task_store(),
        default_roster_store(),
    )
    .with_legacy_mailbox_files()
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
        imported_from: None,
        recorded_at: None,
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
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
        match self
            .mail_store
            .query_mailbox_metadata(boundary::MailStoreQueryMailboxMetadataRequest {
                team: team.clone(),
                agent: agent.clone(),
                limit,
            })
            .map(|response| response.rows)
        {
            Ok(rows) => Ok(rows),
            Err(error)
                if self.allows_legacy_mailbox_files()
                    && is_unsupported_boundary(&error, "MailStore::query_mailbox_metadata") =>
            {
                legacy_query_mailbox_metadata_rows(home_dir, team, agent, limit)
            }
            Err(error) => Err(error),
        }
    }

    fn load_message_record(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError> {
        match self
            .mail_store
            .load_message(boundary::MailStoreLoadMessageRequest {
                team: team.clone(),
                agent: agent.clone(),
                message_key: message_key.clone(),
            })
            .map(|response| response.record)
        {
            Ok(record) => Ok(record),
            Err(error)
                if self.allows_legacy_mailbox_files()
                    && is_unsupported_boundary(&error, "MailStore::load_message") =>
            {
                legacy_load_message_record(home_dir, team, agent, message_key)
            }
            Err(error) => Err(error),
        }
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

fn unsupported(what: &str) -> AtmError {
    AtmError::validation(format!(
        "retained runtime placeholder adapter does not implement {what}"
    ))
    .with_recovery(
        "Use the owning Phase R adapter crate for this boundary operation instead of the retained runtime placeholder.",
    )
}

impl boundary::sealed::Sealed for LegacyMailStoreAdapter {}

impl boundary::MailStore for LegacyMailStoreAdapter {
    fn bootstrap(
        &self,
        request: boundary::MailStoreBootstrapRequest,
    ) -> Result<boundary::MailStoreBootstrapResponse, AtmError> {
        Ok(boundary::MailStoreBootstrapResponse {
            team: request.team,
            bootstrapped: true,
            opened: true,
        })
    }

    fn run_transaction(
        &self,
        request: boundary::MailStoreTransactionRequest,
    ) -> Result<boundary::MailStoreTransactionResponse, AtmError> {
        Ok(boundary::MailStoreTransactionResponse {
            team: request.team,
            committed: true,
            operations_executed: request.requested_operations.len(),
        })
    }

    fn upsert_message(
        &self,
        request: boundary::MailStoreUpsertMessageRequest,
    ) -> Result<boundary::MailStoreUpsertMessageResponse, AtmError> {
        Ok(boundary::MailStoreUpsertMessageResponse {
            record: request.record,
            inserted: true,
        })
    }

    fn load_message(
        &self,
        _request: boundary::MailStoreLoadMessageRequest,
    ) -> Result<boundary::MailStoreLoadMessageResponse, AtmError> {
        Err(unsupported("MailStore::load_message"))
    }

    fn query_mailbox_metadata(
        &self,
        _request: boundary::MailStoreQueryMailboxMetadataRequest,
    ) -> Result<boundary::MailStoreQueryMailboxMetadataResponse, AtmError> {
        Err(unsupported("MailStore::query_mailbox_metadata"))
    }

    fn query_mailbox_metadata_counts(
        &self,
        _request: boundary::MailStoreQueryMailboxMetadataCountsRequest,
    ) -> Result<boundary::MailStoreQueryMailboxMetadataCountsResponse, AtmError> {
        Err(unsupported("MailStore::query_mailbox_metadata_counts"))
    }

    fn upsert_message_state(
        &self,
        request: boundary::UpsertMailMessageStateRequest,
    ) -> Result<boundary::UpsertMailMessageStateResponse, AtmError> {
        Ok(boundary::UpsertMailMessageStateResponse {
            state: request.state,
        })
    }

    fn load_message_state(
        &self,
        _request: boundary::LoadMailMessageStateRequest,
    ) -> Result<boundary::LoadMailMessageStateResponse, AtmError> {
        Ok(boundary::LoadMailMessageStateResponse { state: None })
    }

    fn record_ingest_replay_state(
        &self,
        request: boundary::MailStoreRecordIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreRecordIngestReplayStateResponse, AtmError> {
        Ok(boundary::MailStoreRecordIngestReplayStateResponse {
            state: request.state,
        })
    }

    fn load_ingest_replay_state(
        &self,
        _request: boundary::MailStoreLoadIngestReplayStateRequest,
    ) -> Result<boundary::MailStoreLoadIngestReplayStateResponse, AtmError> {
        Ok(boundary::MailStoreLoadIngestReplayStateResponse { state: None })
    }

    fn health_snapshot(
        &self,
        request: boundary::MailStoreHealthSnapshotRequest,
    ) -> Result<boundary::MailStoreHealthSnapshotResponse, AtmError> {
        Ok(boundary::MailStoreHealthSnapshotResponse {
            snapshot: boundary::MailStoreHealthSnapshot {
                team: request.team,
                agent: request.agent,
                total_messages: 0,
                pending_ack_messages: 0,
                read_messages: 0,
                latest_message_timestamp: None,
            },
        })
    }
}

impl boundary::sealed::Sealed for LegacyTaskStoreAdapter {}

impl boundary::TaskStore for LegacyTaskStoreAdapter {
    fn create_task(
        &self,
        request: boundary::TaskStoreCreateTaskRequest,
    ) -> Result<boundary::TaskStoreCreateTaskResponse, AtmError> {
        Ok(boundary::TaskStoreCreateTaskResponse {
            record: request.record,
        })
    }

    fn load_task(
        &self,
        _request: boundary::TaskStoreLoadTaskRequest,
    ) -> Result<boundary::TaskStoreLoadTaskResponse, AtmError> {
        Ok(boundary::TaskStoreLoadTaskResponse { record: None })
    }

    fn update_task(
        &self,
        _request: boundary::TaskStoreUpdateTaskRequest,
    ) -> Result<boundary::TaskStoreUpdateTaskResponse, AtmError> {
        Err(unsupported("TaskStore::update_task"))
    }

    fn attach_message_link(
        &self,
        _request: boundary::TaskStoreAttachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreAttachMessageLinkResponse, AtmError> {
        Err(unsupported("TaskStore::attach_message_link"))
    }

    fn detach_message_link(
        &self,
        _request: boundary::TaskStoreDetachMessageLinkRequest,
    ) -> Result<boundary::TaskStoreDetachMessageLinkResponse, AtmError> {
        Err(unsupported("TaskStore::detach_message_link"))
    }

    fn record_ack_transition(
        &self,
        _request: boundary::TaskStoreRecordAckTransitionRequest,
    ) -> Result<boundary::TaskStoreRecordAckTransitionResponse, AtmError> {
        Err(unsupported("TaskStore::record_ack_transition"))
    }

    fn query_task_metadata(
        &self,
        _request: boundary::TaskStoreQueryTaskMetadataRequest,
    ) -> Result<boundary::TaskStoreQueryTaskMetadataResponse, AtmError> {
        Ok(boundary::TaskStoreQueryTaskMetadataResponse {
            records: Vec::new(),
        })
    }
}

impl boundary::sealed::Sealed for LegacyRosterStoreAdapter {}

impl boundary::RosterStore for LegacyRosterStoreAdapter {
    fn replace_roster(
        &self,
        request: boundary::RosterStoreReplaceRosterRequest,
    ) -> Result<boundary::RosterStoreReplaceRosterResponse, AtmError> {
        Ok(boundary::RosterStoreReplaceRosterResponse {
            team: request.team,
            previous_member_count: 0,
            current_member_count: request.roster.members.len() as u64,
            replaced: true,
        })
    }

    fn load_roster(
        &self,
        request: boundary::RosterStoreLoadRosterRequest,
    ) -> Result<boundary::RosterStoreLoadRosterResponse, AtmError> {
        Ok(boundary::RosterStoreLoadRosterResponse {
            team: request.team,
            roster: TeamConfig {
                members: Vec::new(),
                extra: serde_json::Map::new(),
            },
        })
    }

    fn query_membership(
        &self,
        request: boundary::RosterStoreQueryMembershipRequest,
    ) -> Result<boundary::RosterStoreQueryMembershipResponse, AtmError> {
        Ok(boundary::RosterStoreQueryMembershipResponse {
            team: request.team,
            member: None,
            is_member: false,
            pid: None,
        })
    }

    fn record_heartbeat(
        &self,
        _request: boundary::RosterStoreRecordHeartbeatRequest,
    ) -> Result<boundary::RosterStoreRecordHeartbeatResponse, AtmError> {
        Err(unsupported("RosterStore::record_heartbeat"))
    }

    fn health_snapshot(
        &self,
        request: boundary::RosterStoreHealthSnapshotRequest,
    ) -> Result<boundary::RosterStoreHealthSnapshotResponse, AtmError> {
        Ok(boundary::RosterStoreHealthSnapshotResponse {
            snapshot: boundary::RosterStoreHealthSnapshot {
                team: request.team,
                member_count: 0,
                stale: false,
                refreshed_at: None,
            },
        })
    }
}
