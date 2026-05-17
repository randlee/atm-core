use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::boundary::{InboxExportReexportMessageRequest, MessageKey};
use crate::config::{self, AtmConfig};
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::read::seen_state;
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::send::{PostSendHookContext, hook::maybe_run_post_send_hook};
use crate::types::{AgentName, IsoTimestamp, TeamName};
use crate::workflow::{self, WorkflowStateFile};

const WORKFLOW_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetainedMailboxTimeoutPolicy {
    pub(crate) workflow_lock_timeout: Duration,
}

pub(crate) trait RetainedServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError>;
    fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError>;
    fn team_dir(&self, home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError>;
    fn inbox_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError>;
    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<IsoTimestamp>, AtmError>;
    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        timestamp: IsoTimestamp,
    ) -> Result<(), AtmError>;
    fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy;
    fn maybe_run_post_send_hook(
        &self,
        warnings: &mut Vec<crate::send::WarningEntry>,
        config: Option<&AtmConfig>,
        context: PostSendHookContext<'_>,
    );
    #[allow(dead_code)]
    fn rebuild_compat_inbox_projection(
        &self,
        inbox_path: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<(), AtmError>;
    fn append_compat_inbox_message(
        &self,
        inbox_path: &Path,
        message: &MessageEnvelope,
    ) -> Result<(), AtmError>;
    fn deliver_non_claude_payloads(
        &self,
        recipient: &DeliveryRecipientSnapshot,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError>;
    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterMemberRecord>, AtmError>;

    fn commit_workflow_state<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>;
}

#[derive(Clone)]
pub struct LocalServiceRuntime {
    pub(crate) mail_store: std::sync::Arc<dyn crate::boundary::MailStore + Send + Sync>,
    pub(crate) task_store: std::sync::Arc<dyn crate::boundary::TaskStore + Send + Sync>,
    pub(crate) roster_store: std::sync::Arc<dyn crate::boundary::RosterStore + Send + Sync>,
    pub(crate) non_claude_outbound:
        std::sync::Arc<dyn crate::boundary::NonClaudeOutbound + Send + Sync>,
}

impl LocalServiceRuntime {
    pub fn new(
        mail_store: std::sync::Arc<dyn crate::boundary::MailStore + Send + Sync>,
        task_store: std::sync::Arc<dyn crate::boundary::TaskStore + Send + Sync>,
        roster_store: std::sync::Arc<dyn crate::boundary::RosterStore + Send + Sync>,
    ) -> Self {
        Self::new_with_non_claude_outbound(
            mail_store,
            task_store,
            roster_store,
            std::sync::Arc::new(LocalFileNonClaudeOutbound),
        )
    }

    pub fn new_with_non_claude_outbound(
        mail_store: std::sync::Arc<dyn crate::boundary::MailStore + Send + Sync>,
        task_store: std::sync::Arc<dyn crate::boundary::TaskStore + Send + Sync>,
        roster_store: std::sync::Arc<dyn crate::boundary::RosterStore + Send + Sync>,
        non_claude_outbound: std::sync::Arc<dyn crate::boundary::NonClaudeOutbound + Send + Sync>,
    ) -> Self {
        Self {
            mail_store,
            task_store,
            roster_store,
            non_claude_outbound,
        }
    }
}

impl fmt::Debug for LocalServiceRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalServiceRuntime")
            .field("mail_store", &std::sync::Arc::as_ptr(&self.mail_store))
            .field("task_store", &std::sync::Arc::as_ptr(&self.task_store))
            .field("roster_store", &std::sync::Arc::as_ptr(&self.roster_store))
            .field(
                "non_claude_outbound",
                &std::sync::Arc::as_ptr(&self.non_claude_outbound),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default)]
/// Production fallback boundary used when the daemon runtime is not composing
/// a dedicated non-Claude outbound adapter. This is not a test double.
struct LocalFileNonClaudeOutbound;

impl crate::boundary::sealed::Sealed for LocalFileNonClaudeOutbound {}

impl crate::boundary::NonClaudeOutbound for LocalFileNonClaudeOutbound {
    fn deliver_payloads(
        &self,
        request: crate::boundary::NonClaudeOutboundDeliveryRequest,
    ) -> Result<crate::boundary::NonClaudeOutboundDeliveryResponse, AtmError> {
        let output_path = crate::home::host_runtime_dir()?.join("non_claude_outbound.jsonl");
        let parent = output_path.parent().ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "non-Claude outbound path {} has no parent directory",
                output_path.display()
            ))
            .with_recovery("Check that ATM_HOME directory is writable and the parent path exists.")
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to create non-Claude outbound directory {}: {error}",
                parent.display()
            ))
            .with_recovery("Check that ATM_HOME directory is writable and the parent path exists.")
            .with_source(error)
        })?;
        crate::mailbox::atomic::append_jsonl_record(&output_path, &request)?;
        Ok(crate::boundary::NonClaudeOutboundDeliveryResponse {
            delivered_messages: request.messages.len(),
        })
    }
}

impl RetainedServiceRuntime for LocalServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
        config::load_config(current_dir)
    }

    fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError> {
        config::load_team_config(team_dir)
    }

    fn team_dir(&self, home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
        crate::home::team_dir_from_home(home_dir, team)
    }

    fn inbox_path(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<PathBuf, AtmError> {
        crate::home::inbox_path_from_home(home_dir, team, agent)
    }

    fn load_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<IsoTimestamp>, AtmError> {
        seen_state::load_seen_watermark(home_dir, team, agent)
    }

    fn save_seen_watermark(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        timestamp: IsoTimestamp,
    ) -> Result<(), AtmError> {
        seen_state::save_seen_watermark(home_dir, team, agent, timestamp)
    }

    fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy {
        RetainedMailboxTimeoutPolicy {
            workflow_lock_timeout: WORKFLOW_LOCK_TIMEOUT,
        }
    }

    fn maybe_run_post_send_hook(
        &self,
        warnings: &mut Vec<crate::send::WarningEntry>,
        config: Option<&AtmConfig>,
        context: PostSendHookContext<'_>,
    ) {
        maybe_run_post_send_hook(warnings, config, context);
    }

    fn rebuild_compat_inbox_projection(
        &self,
        inbox_path: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<(), AtmError> {
        let messages = load_store_backed_mailbox_projection(self, team, agent)?;

        crate::direct_boundaries::reexport_messages(InboxExportReexportMessageRequest {
            path: inbox_path.to_path_buf(),
            messages,
        })
        .map(|_| ())
    }

    fn append_compat_inbox_message(
        &self,
        inbox_path: &Path,
        message: &MessageEnvelope,
    ) -> Result<(), AtmError> {
        if compat_inbox_uses_legacy_array_format(inbox_path)? {
            return Err(AtmError::validation(format!(
                "append-only compatibility delivery does not support legacy array inbox {}",
                inbox_path.display()
            ))
            .with_recovery(
                "Run the explicit repair/rebuild inbox projection path before retrying normal Claude compatibility delivery; ATM no longer rewrites legacy array inboxes from the append-only runtime path.",
            ));
        }
        crate::mailbox::store::append_compat_mailbox_message(inbox_path, message)
    }

    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterMemberRecord>, AtmError> {
        self.roster_store
            .query_membership(crate::boundary::RosterStoreQueryMembershipRequest {
                team: team.clone(),
                member: agent.clone(),
            })
            .map(|response| response.member)
    }

    fn deliver_non_claude_payloads(
        &self,
        recipient: &DeliveryRecipientSnapshot,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError> {
        self.non_claude_outbound
            .deliver_payloads(crate::boundary::NonClaudeOutboundDeliveryRequest {
                team: recipient.team.clone(),
                agent: recipient.agent.clone(),
                recipient_pane_id: recipient.recipient_pane_id.clone(),
                messages: messages.to_vec(),
            })
            .map(|_| ())
    }

    fn commit_workflow_state<T, I, F>(
        &self,
        home_dir: &Path,
        team: &TeamName,
        agent: &AgentName,
        extra_write_paths: I,
        timeout: Duration,
        body: F,
    ) -> Result<T, AtmError>
    where
        I: IntoIterator<Item = PathBuf>,
        F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>,
    {
        workflow::commit_workflow_state(home_dir, team, agent, extra_write_paths, timeout, body)
    }
}

#[allow(dead_code)]
fn load_store_backed_mailbox_projection(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<MessageEnvelope>, AtmError> {
    let mut metadata_rows = runtime
        .mail_store
        .query_mailbox_metadata(crate::boundary::MailStoreQueryMailboxMetadataRequest {
            team: team.clone(),
            agent: agent.clone(),
            limit: None,
        })?
        .rows;
    metadata_rows.sort_by(|left, right| {
        left.message_at
            .cmp(&right.message_at)
            .then_with(|| left.message_key.as_ref().cmp(right.message_key.as_ref()))
    });

    metadata_rows
        .into_iter()
        .map(|row| load_projection_message(runtime, team, agent, &row.message_key))
        .collect()
}

#[allow(dead_code)]
fn load_projection_message(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &AgentName,
    message_key: &MessageKey,
) -> Result<MessageEnvelope, AtmError> {
    runtime
        .mail_store
        .load_message(crate::boundary::MailStoreLoadMessageRequest {
            team: team.clone(),
            agent: agent.clone(),
            message_key: message_key.clone(),
        })?
        .record
        .map(|record| record.envelope)
        .ok_or_else(|| {
            AtmError::validation(format!(
                "sqlite mailbox metadata row {} could not be reloaded for compatibility inbox export",
                message_key
            ))
            .with_recovery(
                "Repair or remove the malformed sqlite mailbox row before retrying the ATM command.",
            )
        })
}

fn compat_inbox_uses_legacy_array_format(path: &Path) -> Result<bool, AtmError> {
    if !path.exists() {
        return Ok(false);
    }

    let mut file = File::open(path).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to inspect compatibility inbox {} before append: {error}",
            path.display()
        ))
        .with_recovery(
            "Retry after concurrent ATM activity completes, or verify the inbox file is readable before retrying the append-only compatibility write.",
        )
        .with_source(error)
    })?;
    let mut prefix = [0_u8; 256];
    let bytes_read = file.read(&mut prefix).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to read compatibility inbox {} before append: {error}",
            path.display()
        ))
        .with_recovery(
            "Retry after concurrent ATM activity completes, or verify the inbox file is readable before retrying the append-only compatibility write.",
        )
        .with_source(error)
    })?;
    let visible = String::from_utf8_lossy(&prefix[..bytes_read]);
    Ok(visible.trim_start().starts_with('['))
}

#[cfg(test)]
mod tests {
    use super::{LocalServiceRuntime, RetainedServiceRuntime};
    use crate::boundary;
    use crate::error_codes::AtmErrorCode;
    use crate::schema::MessageEnvelope;
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use chrono::Utc;
    use std::fs::File;
    use std::io::Read;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[derive(Debug)]
    struct NoopMailStore;

    impl boundary::sealed::Sealed for NoopMailStore {}

    impl boundary::MailStore for NoopMailStore {
        fn bootstrap(
            &self,
            _request: boundary::MailStoreBootstrapRequest,
        ) -> Result<boundary::MailStoreBootstrapResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn run_transaction(
            &self,
            _request: boundary::MailStoreTransactionRequest,
        ) -> Result<boundary::MailStoreTransactionResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn upsert_message(
            &self,
            _request: boundary::MailStoreUpsertMessageRequest,
        ) -> Result<boundary::MailStoreUpsertMessageResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn load_message(
            &self,
            _request: boundary::MailStoreLoadMessageRequest,
        ) -> Result<boundary::MailStoreLoadMessageResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn load_stored_message(
            &self,
            _request: boundary::MailStoreLoadStoredMessageRequest,
        ) -> Result<boundary::MailStoreLoadStoredMessageResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn query_mailbox_metadata(
            &self,
            _request: boundary::MailStoreQueryMailboxMetadataRequest,
        ) -> Result<boundary::MailStoreQueryMailboxMetadataResponse, crate::error::AtmError>
        {
            Ok(boundary::MailStoreQueryMailboxMetadataResponse { rows: Vec::new() })
        }

        fn query_mailbox_metadata_counts(
            &self,
            _request: boundary::MailStoreQueryMailboxMetadataCountsRequest,
        ) -> Result<boundary::MailStoreQueryMailboxMetadataCountsResponse, crate::error::AtmError>
        {
            unimplemented!("test stub")
        }

        fn upsert_message_state(
            &self,
            _request: boundary::UpsertMailMessageStateRequest,
        ) -> Result<boundary::UpsertMailMessageStateResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn load_message_state(
            &self,
            _request: boundary::LoadMailMessageStateRequest,
        ) -> Result<boundary::LoadMailMessageStateResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn record_ingest_replay_state(
            &self,
            _request: boundary::MailStoreRecordIngestReplayStateRequest,
        ) -> Result<boundary::MailStoreRecordIngestReplayStateResponse, crate::error::AtmError>
        {
            unimplemented!("test stub")
        }

        fn load_ingest_replay_state(
            &self,
            _request: boundary::MailStoreLoadIngestReplayStateRequest,
        ) -> Result<boundary::MailStoreLoadIngestReplayStateResponse, crate::error::AtmError>
        {
            unimplemented!("test stub")
        }

        fn health_snapshot(
            &self,
            _request: boundary::MailStoreHealthSnapshotRequest,
        ) -> Result<boundary::MailStoreHealthSnapshotResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }
    }

    #[derive(Debug)]
    struct NoopTaskStore;

    impl boundary::sealed::Sealed for NoopTaskStore {}

    impl boundary::TaskStore for NoopTaskStore {
        fn create_task(
            &self,
            _request: boundary::TaskStoreCreateTaskRequest,
        ) -> Result<boundary::TaskStoreCreateTaskResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn load_task(
            &self,
            _request: boundary::TaskStoreLoadTaskRequest,
        ) -> Result<boundary::TaskStoreLoadTaskResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn update_task(
            &self,
            _request: boundary::TaskStoreUpdateTaskRequest,
        ) -> Result<boundary::TaskStoreUpdateTaskResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn attach_message_link(
            &self,
            _request: boundary::TaskStoreAttachMessageLinkRequest,
        ) -> Result<boundary::TaskStoreAttachMessageLinkResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn detach_message_link(
            &self,
            _request: boundary::TaskStoreDetachMessageLinkRequest,
        ) -> Result<boundary::TaskStoreDetachMessageLinkResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn record_ack_transition(
            &self,
            _request: boundary::TaskStoreRecordAckTransitionRequest,
        ) -> Result<boundary::TaskStoreRecordAckTransitionResponse, crate::error::AtmError>
        {
            unimplemented!("test stub")
        }

        fn query_task_metadata(
            &self,
            _request: boundary::TaskStoreQueryTaskMetadataRequest,
        ) -> Result<boundary::TaskStoreQueryTaskMetadataResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }
    }

    #[derive(Debug)]
    struct NoopRosterStore;

    impl boundary::sealed::Sealed for NoopRosterStore {}

    impl boundary::RosterStore for NoopRosterStore {
        fn replace_roster(
            &self,
            _request: boundary::RosterStoreReplaceRosterRequest,
        ) -> Result<boundary::RosterStoreReplaceRosterResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn load_roster(
            &self,
            _request: boundary::RosterStoreLoadRosterRequest,
        ) -> Result<boundary::RosterStoreLoadRosterResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn query_membership(
            &self,
            _request: boundary::RosterStoreQueryMembershipRequest,
        ) -> Result<boundary::RosterStoreQueryMembershipResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn list_teams(
            &self,
            _request: boundary::RosterStoreListTeamsRequest,
        ) -> Result<boundary::RosterStoreListTeamsResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn health_snapshot(
            &self,
            _request: boundary::RosterStoreHealthSnapshotRequest,
        ) -> Result<boundary::RosterStoreHealthSnapshotResponse, crate::error::AtmError> {
            unimplemented!("test stub")
        }
    }

    fn message() -> MessageEnvelope {
        MessageEnvelope {
            from: "sender".parse::<AgentName>().expect("sender"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some("test-team".parse::<TeamName>().expect("team")),
            summary: None,
            message_id: None,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn append_compat_inbox_message_rejects_legacy_array_mailbox_from_runtime_path() {
        let tempdir = tempdir().expect("tempdir");
        let inbox_path = tempdir.path().join("recipient.json");
        std::fs::write(
            &inbox_path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&vec![message()]).expect("mailbox array")
            ),
        )
        .expect("write mailbox");

        let runtime = LocalServiceRuntime::new(
            Arc::new(NoopMailStore),
            Arc::new(NoopTaskStore),
            Arc::new(NoopRosterStore),
        );

        let error = runtime
            .append_compat_inbox_message(&inbox_path, &message())
            .expect_err("legacy array path must fail closed");
        assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
        assert!(
            error
                .recovery
                .as_deref()
                .unwrap_or_default()
                .contains("explicit repair/rebuild inbox projection path"),
            "unexpected recovery: {error:?}"
        );
    }

    #[test]
    fn rebuild_compat_inbox_projection_reexports_store_backed_mailbox() {
        let tempdir = tempdir().expect("tempdir");
        let inbox_path = tempdir.path().join("recipient.jsonl");
        let runtime = LocalServiceRuntime::new(
            Arc::new(NoopMailStore),
            Arc::new(NoopTaskStore),
            Arc::new(NoopRosterStore),
        );
        let team = "test-team".parse::<TeamName>().expect("team");
        let agent = "recipient".parse::<AgentName>().expect("agent");

        runtime
            .rebuild_compat_inbox_projection(&inbox_path, &team, &agent)
            .expect("rebuild must succeed");

        let mut file = File::open(&inbox_path).expect("rebuild should create projection file");
        let mut content = String::new();
        file.read_to_string(&mut content)
            .expect("read projection file");
        assert_eq!(content, "[]\n");
    }
}
