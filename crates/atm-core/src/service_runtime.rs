#![allow(
    deprecated,
    reason = "the retained runtime bridge still consumes the transitional shared storage traits until the direct boundary fully replaces it"
)]

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use atm_storage::{MessageStore as SharedMessageStore, RosterStore as SharedRosterStore};

use crate::config::{self, AtmConfig};
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::protocol::NotificationEvent;
use crate::read::seen_state;
use crate::schema::{InboxMessage, TeamConfig};
use crate::types::{AgentName, IsoTimestamp, TeamName};
use crate::workflow::{self, WorkflowStateFile};

const WORKFLOW_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_NON_CLAUDE_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetainedMailboxTimeoutPolicy {
    pub(crate) workflow_lock_timeout: Duration,
}

pub(crate) trait RetainedServiceRuntime: crate::boundary::sealed::Sealed {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError>;
    fn load_nudge_template_override(
        &self,
        _team: &TeamName,
        _kind: crate::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<Option<crate::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
        Ok(None)
    }
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
    fn deliver_non_claude_payloads(
        &self,
        recipient: &DeliveryRecipientSnapshot,
        messages: &[InboxMessage],
    ) -> Result<(), AtmError>;
    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterEntry>, AtmError>;
    fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterEntry>, AtmError>;
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
    pub(crate) message_store: std::sync::Arc<dyn SharedMessageStore + Send + Sync>,
    pub(crate) roster_store: std::sync::Arc<dyn SharedRosterStore + Send + Sync>,
    pub(crate) nudge_template_override_store:
        std::sync::Arc<dyn crate::boundary::NudgeTemplateOverrideStore + Send + Sync>,
    pub(crate) non_claude_outbound:
        std::sync::Arc<dyn crate::boundary::NonClaudeOutbound + Send + Sync>,
}

impl LocalServiceRuntime {
    pub fn new_with_delivery_boundaries(
        message_store: std::sync::Arc<dyn SharedMessageStore + Send + Sync>,
        roster_store: std::sync::Arc<dyn SharedRosterStore + Send + Sync>,
        nudge_template_override_store: std::sync::Arc<
            dyn crate::boundary::NudgeTemplateOverrideStore + Send + Sync,
        >,
        non_claude_outbound: std::sync::Arc<dyn crate::boundary::NonClaudeOutbound + Send + Sync>,
    ) -> Self {
        Self {
            message_store,
            roster_store,
            nudge_template_override_store,
            non_claude_outbound,
        }
    }

    pub fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterEntry>, AtmError> {
        Ok(self
            .roster_store
            .load_roster(team)?
            .members
            .into_iter()
            .find(|member| &member.agent_name == agent))
    }

    pub fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterEntry>, AtmError> {
        self.roster_store
            .load_roster(team)
            .map(|snapshot| snapshot.members)
    }
}

impl fmt::Debug for LocalServiceRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalServiceRuntime")
            .field(
                "message_store",
                &std::sync::Arc::as_ptr(&self.message_store),
            )
            .field("roster_store", &std::sync::Arc::as_ptr(&self.roster_store))
            .field(
                "nudge_template_override_store",
                &std::sync::Arc::as_ptr(&self.nudge_template_override_store),
            )
            .field(
                "non_claude_outbound",
                &std::sync::Arc::as_ptr(&self.non_claude_outbound),
            )
            .finish()
    }
}

impl crate::boundary::sealed::Sealed for LocalServiceRuntime {}

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

pub(crate) fn append_notification_log(event: &NotificationEvent) -> Result<(), AtmError> {
    append_notification_log_at_path(
        &crate::home::host_runtime_dir()?.join("notifications.jsonl"),
        event,
    )
}

pub(crate) fn append_notification_log_at_path(
    path: &Path,
    event: &NotificationEvent,
) -> Result<(), AtmError> {
    let parent = path.parent().ok_or_else(|| {
        AtmError::mailbox_write(format!(
            "notification log path {} has no parent directory",
            path.display()
        ))
        .with_recovery("Choose a notification log path with an existing parent directory.")
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create notification log directory {}: {error}",
            parent.display()
        ))
        .with_recovery(
            "Check that the notification log directory is writable before retrying post-send logging.",
        )
        .with_source(error)
    })?;
    crate::mailbox::atomic::append_jsonl_record(path, event)
}

impl RetainedServiceRuntime for LocalServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
        config::load_config(current_dir)
    }

    fn load_nudge_template_override(
        &self,
        team: &TeamName,
        kind: crate::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<Option<crate::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
        self.nudge_template_override_store
            .load_template_override(team, kind)
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

    fn load_roster_member(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<crate::boundary::RosterEntry>, AtmError> {
        Self::load_roster_member(self, team, agent)
    }

    fn load_team_roster(
        &self,
        team: &TeamName,
    ) -> Result<Vec<crate::boundary::RosterEntry>, AtmError> {
        Self::load_team_roster(self, team)
    }

    fn deliver_non_claude_payloads(
        &self,
        recipient: &DeliveryRecipientSnapshot,
        messages: &[InboxMessage],
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
) -> Result<Vec<InboxMessage>, AtmError> {
    let mut metadata_rows =
        crate::service_runtime_store::RetainedMailboxRuntime::query_mailbox_metadata_rows(
            runtime,
            Path::new(""),
            team,
            agent,
            None,
        )?;
    metadata_rows.sort_by(|left, right| {
        left.message_at
            .cmp(&right.message_at)
            .then_with(|| left.message_key.as_ref().cmp(right.message_key.as_ref()))
    });

    let mut messages = Vec::with_capacity(metadata_rows.len());
    for row in metadata_rows {
        // Keep the repair/rebuild projection consistent with the live send
        // export path: a row deleted between metadata enumeration and reload is
        // a legal concurrent-clear race, not a fatal rebuild error.
        let Some(record) =
            crate::service_runtime_store::RetainedMailboxRuntime::load_message_record(
                runtime,
                Path::new(""),
                team,
                agent,
                &row.message_key,
            )?
        else {
            continue;
        };
        messages.push(record.envelope);
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::{
        LocalFileNonClaudeOutbound, LocalServiceRuntime, MAX_NON_CLAUDE_PAYLOAD_BYTES,
        RetainedServiceRuntime, append_notification_log_at_path,
    };
    use crate::error_codes::AtmErrorCode;
    use crate::protocol::{NotificationEvent, NotificationKind};
    use crate::schema::InboxMessage;
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use chrono::Utc;
    use std::fs::File;
    use std::io::Read;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[derive(Debug)]
    struct NoopMessageStore;

    #[allow(
        deprecated,
        reason = "service-runtime tests intentionally exercise the transitional shared storage traits"
    )]
    impl atm_storage::MessageStore for NoopMessageStore {
        fn save_message(
            &self,
            _message: &atm_storage::Message,
        ) -> Result<(), crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn load_message(
            &self,
            _key: &atm_storage::MessageKey,
        ) -> Result<Option<atm_storage::Message>, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn list_messages(
            &self,
            _query: &atm_storage::MessageQuery,
        ) -> Result<Vec<atm_storage::Message>, crate::error::AtmError> {
            Ok(Vec::new())
        }

        fn delete_message(
            &self,
            _key: &atm_storage::MessageKey,
        ) -> Result<(), crate::error::AtmError> {
            unimplemented!("test stub")
        }
    }

    #[derive(Debug)]
    struct NoopRosterStore;

    #[allow(
        deprecated,
        reason = "service-runtime tests intentionally exercise the transitional shared storage traits"
    )]
    impl atm_storage::RosterStore for NoopRosterStore {
        fn load_roster(
            &self,
            _team: &TeamName,
        ) -> Result<atm_storage::RosterSnapshot, crate::error::AtmError> {
            Ok(atm_storage::RosterSnapshot {
                team_name: _team.clone(),
                members: Vec::new(),
                refreshed_at: None,
            })
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn save_roster(
            &self,
            _roster: &atm_storage::RosterSnapshot,
        ) -> Result<(), crate::error::AtmError> {
            unimplemented!("test stub")
        }
    }

    #[derive(Debug)]
    struct NoopNudgeTemplateOverrideStore;

    impl crate::boundary::sealed::Sealed for NoopNudgeTemplateOverrideStore {}

    impl crate::boundary::NudgeTemplateOverrideStore for NoopNudgeTemplateOverrideStore {
        fn load_template_override(
            &self,
            _team: &TeamName,
            _kind: crate::boundary::BuiltInNudgeTemplateKind,
        ) -> Result<Option<crate::boundary::TeamNudgeTemplateOverrideRow>, crate::error::AtmError>
        {
            Ok(None)
        }

        fn save_template_override(
            &self,
            _team: &TeamName,
            _kind: crate::boundary::BuiltInNudgeTemplateKind,
            _template_body: &str,
        ) -> Result<crate::boundary::TeamNudgeTemplateOverrideRow, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn disable_template_override(
            &self,
            _team: &TeamName,
            _kind: crate::boundary::BuiltInNudgeTemplateKind,
        ) -> Result<crate::boundary::TeamNudgeTemplateOverrideRow, crate::error::AtmError> {
            unimplemented!("test stub")
        }

        fn clear_template_override(
            &self,
            _team: &TeamName,
            _kind: crate::boundary::BuiltInNudgeTemplateKind,
        ) -> Result<bool, crate::error::AtmError> {
            unimplemented!("test stub")
        }
    }

    fn message() -> InboxMessage {
        InboxMessage {
            from: "sender".parse::<AgentName>().expect("sender"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some("test-team".parse::<TeamName>().expect("team")),
            summary: None,
            message_id: None,
            requires_ack: false,
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
    fn rebuild_compat_inbox_projection_reexports_store_backed_mailbox() {
        let tempdir = tempdir().expect("tempdir");
        let inbox_path = tempdir.path().join("recipient.jsonl");
        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            Arc::new(NoopMessageStore),
            Arc::new(NoopRosterStore),
            Arc::new(NoopNudgeTemplateOverrideStore),
            Arc::new(LocalFileNonClaudeOutbound::new()),
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
    fn notification_logging_appends_directly_at_event_site() {
        let tempdir = tempdir().expect("tempdir");
        let notification_path = tempdir.path().join("notifications.jsonl");
        let event = NotificationEvent {
            kind: NotificationKind::Delivery,
            detail: "runtime-direct".to_string(),
            team: Some("test-team".parse::<TeamName>().expect("team")),
            agent: Some("recipient".parse::<AgentName>().expect("agent")),
        };

        append_notification_log_at_path(&notification_path, &event).expect("direct log append");

        let direct_events = read_notification_events(&notification_path);
        assert_eq!(direct_events.len(), 1);
        assert_eq!(direct_events[0].detail, "runtime-direct");
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
                messages: vec![InboxMessage {
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
                "Check that the mailbox/workflow path is writable, has free space, and was not modified concurrently before retrying the ATM command."
            )
        );
        assert!(!output_path.exists());
    }
}
