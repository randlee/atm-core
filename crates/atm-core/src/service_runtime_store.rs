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

impl Default for LocalServiceRuntime {
    fn default() -> Self {
        Self {
            mail_store: default_mail_store(),
            task_store: default_task_store(),
            roster_store: default_roster_store(),
        }
    }
}

pub(crate) trait RetainedMailboxRuntime {
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

pub(crate) fn observe_source_files(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceFile>, AtmError> {
    mailbox::store::observe_source_files(home_dir, team.as_ref(), agent.as_ref())
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
        team.as_ref(),
        agent.as_ref(),
        extra_write_paths,
        timeout,
        body,
    )
}

impl RetainedMailboxRuntime for LocalServiceRuntime {
    fn observe_source_files(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Vec<SourceFile>, AtmError> {
        let _mail_store = self.mail_store.as_ref();
        observe_source_files(home_dir, team, agent)
    }

    fn commit_source_files(&self, source_files: &[SourceFile]) -> Result<(), AtmError> {
        let _mail_store = self.mail_store.as_ref();
        commit_source_files(source_files)
    }

    fn read_messages(&self, path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
        let _mail_store = self.mail_store.as_ref();
        crate::mailbox::read_messages(path)
    }

    fn commit_mailbox_state(
        &self,
        path: &Path,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError> {
        let _mail_store = self.mail_store.as_ref();
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
        let _mail_store = self.mail_store.as_ref();
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

    fn upsert_visibility_state(
        &self,
        request: boundary::MailStoreUpsertVisibilityStateRequest,
    ) -> Result<boundary::MailStoreUpsertVisibilityStateResponse, AtmError> {
        Ok(boundary::MailStoreUpsertVisibilityStateResponse {
            state: request.state,
        })
    }

    fn load_visibility_state(
        &self,
        _request: boundary::MailStoreLoadVisibilityStateRequest,
    ) -> Result<boundary::MailStoreLoadVisibilityStateResponse, AtmError> {
        Ok(boundary::MailStoreLoadVisibilityStateResponse { state: None })
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
        })
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
