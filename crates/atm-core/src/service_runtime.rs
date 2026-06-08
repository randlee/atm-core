use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::boundary::{MessageKey, ProjectionAppendMode};
use crate::config::{self, AtmConfig};
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::protocol::NotificationEvent;
use crate::read::seen_state;
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::types::{AgentName, IsoTimestamp, TeamName};
use crate::workflow::{self, WorkflowStateFile};

const WORKFLOW_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_NON_CLAUDE_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetainedMailboxTimeoutPolicy {
    pub(crate) workflow_lock_timeout: Duration,
}

pub(crate) trait RetainedServiceRuntime:
    crate::boundary::NotificationSink + crate::boundary::sealed::Sealed
{
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError>;
    fn load_team_config_for_doctor_compare(&self, team_dir: &Path) -> Result<TeamConfig, AtmError>;
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
    #[allow(
        dead_code,
        reason = "Repair/rebuild-only seam; called from tests and explicit repair paths, not from the normal runtime delivery pipeline."
    )]
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
    fn append_compat_inbox_message_set(
        &self,
        inbox_path: &Path,
        mode: ProjectionAppendMode,
        messages: &[MessageEnvelope],
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
    fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterMemberRecord>, AtmError>;
    fn load_claude_code_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<crate::boundary::ProjectionRoster, AtmError> {
        let records = self.load_team_roster(team)?;
        Ok(crate::boundary::ProjectionRoster::from_roster_snapshot(
            team.clone(),
            &records,
        ))
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
        F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>;
}

#[derive(Clone)]
pub struct LocalServiceRuntime {
    pub(crate) mail_store: std::sync::Arc<dyn crate::boundary::MailStore + Send + Sync>,
    pub(crate) task_store: std::sync::Arc<dyn crate::boundary::TaskStore + Send + Sync>,
    pub(crate) roster_store: std::sync::Arc<dyn crate::boundary::RosterStore + Send + Sync>,
    pub(crate) non_claude_outbound:
        std::sync::Arc<dyn crate::boundary::NonClaudeOutbound + Send + Sync>,
    pub(crate) notification_sink:
        std::sync::Arc<dyn crate::boundary::NotificationSink + Send + Sync>,
}

impl LocalServiceRuntime {
    pub fn new_with_delivery_boundaries(
        mail_store: std::sync::Arc<dyn crate::boundary::MailStore + Send + Sync>,
        task_store: std::sync::Arc<dyn crate::boundary::TaskStore + Send + Sync>,
        roster_store: std::sync::Arc<dyn crate::boundary::RosterStore + Send + Sync>,
        non_claude_outbound: std::sync::Arc<dyn crate::boundary::NonClaudeOutbound + Send + Sync>,
        notification_sink: std::sync::Arc<dyn crate::boundary::NotificationSink + Send + Sync>,
    ) -> Self {
        Self {
            mail_store,
            task_store,
            roster_store,
            non_claude_outbound,
            notification_sink,
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
            .field(
                "notification_sink",
                &std::sync::Arc::as_ptr(&self.notification_sink),
            )
            .finish()
    }
}

impl crate::boundary::sealed::Sealed for LocalServiceRuntime {}

impl crate::boundary::NotificationSink for LocalServiceRuntime {
    fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError> {
        self.notification_sink.deliver(event)
    }
}

type OutputPathFactory = std::sync::Arc<dyn Fn() -> Result<PathBuf, AtmError> + Send + Sync>;

#[derive(Clone)]
/// Production fallback boundary used when the daemon runtime is not composing
/// a dedicated non-Claude outbound adapter. This is not a test double.
pub struct LocalFileNonClaudeOutbound {
    path_factory: OutputPathFactory,
}

impl LocalFileNonClaudeOutbound {
    pub fn new() -> Self {
        Self::new_with_path_factory(std::sync::Arc::new(|| {
            Ok(crate::home::host_runtime_dir()?.join("non_claude_outbound.jsonl"))
        }))
    }

    fn new_with_path_factory(path_factory: OutputPathFactory) -> Self {
        Self { path_factory }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_path(path: PathBuf) -> Self {
        Self::new_with_path_factory(std::sync::Arc::new(move || Ok(path.clone())))
    }
}

impl Default for LocalFileNonClaudeOutbound {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for LocalFileNonClaudeOutbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LocalFileNonClaudeOutbound(..)")
    }
}

impl crate::boundary::sealed::Sealed for LocalFileNonClaudeOutbound {}

impl crate::boundary::NonClaudeOutbound for LocalFileNonClaudeOutbound {
    fn deliver_payloads(
        &self,
        request: crate::boundary::NonClaudeOutboundDeliveryRequest,
    ) -> Result<crate::boundary::NonClaudeOutboundDeliveryResponse, AtmError> {
        let output_path = (self.path_factory)().map_err(|e| {
            e.with_recovery(
                "Set ATM_HOME to a writable directory or ensure the user home directory is accessible before retrying non-Claude outbound delivery.",
            )
        })?;
        let bytes = serde_json::to_vec(&request)?;
        if bytes.len() > MAX_NON_CLAUDE_PAYLOAD_BYTES {
            return Err(AtmError::mailbox_write(format!(
                "non-Claude outbound payload for {} exceeded {MAX_NON_CLAUDE_PAYLOAD_BYTES} bytes",
                output_path.display()
            ))
            .with_recovery(
                "Reduce message count or body size before retrying non-Claude delivery through the outbound payload sink.",
            ));
        }
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

#[derive(Debug, Clone)]
/// Production fallback boundary used when the daemon runtime is not composing
/// a dedicated notification sink. This is not a test double.
pub struct LocalFileNotificationSink {
    path: PathBuf,
}

impl LocalFileNotificationSink {
    /// Path validation stays lazy because this constructor is used from
    /// cross-crate runtime assembly callsites that only have a PathBuf. The
    /// actual boundary contract is enforced on first deliver() with typed I/O
    /// errors instead of panicking during assembly.
    pub fn at_path(path: PathBuf) -> Self {
        Self { path }
    }
}

impl crate::boundary::sealed::Sealed for LocalFileNotificationSink {}

impl crate::boundary::NotificationSink for LocalFileNotificationSink {
    fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError> {
        let parent = self.path.parent().ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "notification sink path {} has no parent directory",
                self.path.display()
            ))
            .with_recovery("Choose a notification output path with an existing parent directory.")
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to create notification sink directory {}: {error}",
                parent.display()
            ))
            .with_recovery(
                "Check that the notification output directory is writable before retrying notification delivery.",
            )
            .with_source(error)
        })?;
        crate::mailbox::atomic::append_jsonl_record(&self.path, &event)
    }
}

impl RetainedServiceRuntime for LocalServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
        config::load_config(current_dir)
    }

    fn load_team_config_for_doctor_compare(&self, team_dir: &Path) -> Result<TeamConfig, AtmError> {
        config::load_claude_team_config_document(team_dir)
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

    fn rebuild_compat_inbox_projection(
        &self,
        inbox_path: &Path,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<(), AtmError> {
        let messages = load_store_backed_mailbox_projection(self, team, agent)?;
        crate::mailbox::export_compat_mailbox_projection(inbox_path, &messages)
    }

    fn append_compat_inbox_message(
        &self,
        inbox_path: &Path,
        message: &MessageEnvelope,
    ) -> Result<(), AtmError> {
        crate::mailbox::store::append_compat_mailbox_message(inbox_path, message).map_err(|error| {
            if current_claude_inbox_requires_repair(inbox_path).unwrap_or(false) {
                AtmError::validation(format!(
                    "compatibility inbox {} is malformed or unsupported for the primary Claude delivery path",
                    inbox_path.display()
                ))
                .with_recovery(
                    "Run the explicit repair/rebuild inbox projection path before retrying normal Claude compatibility delivery; healthy current Claude inbox files should not require this path.",
                )
                .with_source(error)
            } else {
                error
            }
        })
    }

    fn append_compat_inbox_message_set(
        &self,
        inbox_path: &Path,
        mode: ProjectionAppendMode,
        messages: &[MessageEnvelope],
    ) -> Result<(), AtmError> {
        match mode {
            ProjectionAppendMode::RecoveredLogicalMessageSet => {
                let export_policy = crate::mailbox::store::export_policy_for_path(inbox_path)?;
                crate::mailbox::store::append_compat_mailbox_message_set(
                    inbox_path,
                    export_policy,
                    messages,
                )
            }
        }
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

    fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterMemberRecord>, AtmError> {
        self.roster_store
            .load_roster(crate::boundary::RosterStoreLoadRosterRequest { team: team.clone() })
            .map(|response| response.members)
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

#[allow(
    dead_code,
    reason = "Called only from rebuild_compat_inbox_projection, which is a repair/rebuild-only seam exercised via tests and explicit repair paths."
)]
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

#[allow(
    dead_code,
    reason = "Called only from load_store_backed_mailbox_projection, which is a repair/rebuild-only seam exercised via tests and explicit repair paths."
)]
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

fn current_claude_inbox_requires_repair(path: &Path) -> Result<bool, AtmError> {
    if !path.exists()
        || crate::mailbox::store::inbox_file_format(path)
            != crate::mailbox::store::InboxFileFormat::ClaudeJsonArray
    {
        return Ok(false);
    }

    Ok(crate::mailbox::load_compat_mailbox_messages_strict(path).is_err())
}

#[cfg(test)]
mod tests {
    use super::{
        LocalFileNonClaudeOutbound, LocalFileNotificationSink, LocalServiceRuntime,
        MAX_NON_CLAUDE_PAYLOAD_BYTES, RetainedServiceRuntime,
    };
    use crate::boundary;
    use crate::error_codes::AtmErrorCode;
    use crate::protocol::{NotificationEvent, NotificationKind};
    use crate::schema::MessageEnvelope;
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use chrono::Utc;
    use serde_json::Value;
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

    fn read_notification_events(path: &std::path::Path) -> Vec<NotificationEvent> {
        std::fs::read_to_string(path)
            .expect("notifications")
            .lines()
            .map(|line| serde_json::from_str(line).expect("notification event"))
            .collect()
    }

    #[test]
    fn append_compat_inbox_message_accepts_current_claude_json_array_mailbox() {
        let tempdir = tempdir().expect("tempdir");
        let inbox_path = tempdir.path().join("recipient.json");
        let first = message();
        let second = message();
        std::fs::write(
            &inbox_path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&vec![first.clone()]).expect("mailbox array")
            ),
        )
        .expect("write mailbox");

        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            Arc::new(NoopMailStore),
            Arc::new(NoopTaskStore),
            Arc::new(NoopRosterStore),
            Arc::new(LocalFileNonClaudeOutbound::new()),
            Arc::new(LocalFileNotificationSink::at_path(
                tempdir.path().join("notifications.jsonl"),
            )),
        );

        runtime
            .append_compat_inbox_message(&inbox_path, &second)
            .expect("current Claude array path should succeed");

        let raw = std::fs::read_to_string(&inbox_path).expect("mailbox contents");
        let encoded: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("json array");
        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[0]["text"], serde_json::Value::String(first.text));
        assert_eq!(encoded[1]["text"], serde_json::Value::String(second.text));
    }

    #[test]
    fn append_compat_inbox_message_rejects_malformed_current_claude_json_array_mailbox() {
        let tempdir = tempdir().expect("tempdir");
        let inbox_path = tempdir.path().join("recipient.json");
        std::fs::write(&inbox_path, "[{ not-json }\n").expect("write malformed mailbox");

        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            Arc::new(NoopMailStore),
            Arc::new(NoopTaskStore),
            Arc::new(NoopRosterStore),
            Arc::new(LocalFileNonClaudeOutbound::new()),
            Arc::new(LocalFileNotificationSink::at_path(
                tempdir.path().join("notifications.jsonl"),
            )),
        );

        let error = runtime
            .append_compat_inbox_message(&inbox_path, &message())
            .expect_err("malformed Claude array path must fail closed");
        assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
        assert!(
            error
                .primary_recovery()
                .unwrap_or_default()
                .contains("explicit repair/rebuild inbox projection path"),
            "unexpected recovery: {error:?}"
        );
    }

    #[test]
    fn rebuild_compat_inbox_projection_reexports_store_backed_mailbox() {
        let tempdir = tempdir().expect("tempdir");
        let inbox_path = tempdir.path().join("recipient.jsonl");
        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            Arc::new(NoopMailStore),
            Arc::new(NoopTaskStore),
            Arc::new(NoopRosterStore),
            Arc::new(LocalFileNonClaudeOutbound::new()),
            Arc::new(LocalFileNotificationSink::at_path(
                tempdir.path().join("notifications.jsonl"),
            )),
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

    #[test]
    fn local_service_runtime_delivers_notifications_through_sink_boundary() {
        let tempdir = tempdir().expect("tempdir");
        let notification_path = tempdir.path().join("notifications.jsonl");
        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            Arc::new(NoopMailStore),
            Arc::new(NoopTaskStore),
            Arc::new(NoopRosterStore),
            Arc::new(LocalFileNonClaudeOutbound::new()),
            Arc::new(LocalFileNotificationSink::at_path(
                notification_path.clone(),
            )),
        );
        let event = NotificationEvent {
            kind: NotificationKind::Delivery,
            detail: "runtime-direct".to_string(),
            team: Some("test-team".parse::<TeamName>().expect("team")),
            agent: Some("recipient".parse::<AgentName>().expect("agent")),
        };

        crate::boundary::NotificationSink::deliver(&runtime, event.clone())
            .expect("direct sink delivery");

        let direct_events = read_notification_events(&notification_path);
        assert_eq!(direct_events.len(), 1);
        assert_eq!(direct_events[0].detail, "runtime-direct");

        let mut warnings = Vec::new();
        crate::delivery_execution::deliver_notifications(
            &runtime,
            &mut warnings,
            &crate::send::ResolvedRecipient {
                agent: "recipient".parse::<AgentName>().expect("agent"),
                team: "test-team".parse::<TeamName>().expect("team"),
            },
            Some("pane-1"),
            &[crate::delivery_plan::NotificationTarget {
                sender: "sender".parse::<AgentName>().expect("sender"),
                sender_team: Some("test-team".parse::<TeamName>().expect("team")),
                message_id: crate::schema::AtmMessageId::new(),
                requires_ack: true,
                is_ack: false,
                task_id: None,
            }],
        );
        assert!(warnings.is_empty());

        let events = read_notification_events(&notification_path);
        assert_eq!(events.len(), 2);
        let detail: Value =
            serde_json::from_str(&events[1].detail).expect("structured notification detail");
        assert_eq!(
            detail.get("recipient_pane_id").and_then(Value::as_str),
            Some("pane-1")
        );
    }

    #[test]
    fn local_non_claude_outbound_rejects_oversized_payloads() {
        let tempdir = tempdir().expect("tempdir");
        let output_path = tempdir.path().join("non_claude_outbound.jsonl");
        let runtime = LocalFileNonClaudeOutbound::new_for_test_with_path(output_path.clone());
        let oversized_body = "a".repeat(MAX_NON_CLAUDE_PAYLOAD_BYTES + 1);

        let error = crate::boundary::NonClaudeOutbound::deliver_payloads(
            &runtime,
            crate::boundary::NonClaudeOutboundDeliveryRequest {
                team: TeamName::from_validated("test-team"),
                agent: AgentName::from_validated("recipient"),
                recipient_pane_id: None,
                messages: vec![MessageEnvelope {
                    text: oversized_body,
                    ..message()
                }],
            },
        )
        .expect_err("oversized non-claude payload must fail");

        assert_eq!(error.code, AtmErrorCode::MailboxWriteFailed);
        assert!(error.message.contains("exceeded"));
        assert_eq!(
            error.primary_recovery(),
            Some(
                "Reduce message count or body size before retrying non-Claude delivery through the outbound payload sink."
            )
        );
        assert!(!output_path.exists());
    }
}
