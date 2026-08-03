#![allow(
    deprecated,
    reason = "the retained runtime bridge still consumes the transitional shared storage traits until the direct boundary fully replaces it"
)]

use std::fmt;
use std::path::{Path, PathBuf};

use atm_storage::{MessageStore as SharedMessageStore, RosterStore as SharedRosterStore};

use crate::config::{self, AtmConfig};
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::protocol::NotificationEvent;
use crate::read::seen_state;
use crate::schema::InboxMessage;
use crate::types::{AgentName, IsoTimestamp, TeamName};
const MAX_NON_CLAUDE_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WorkspaceConfigAccess {
    #[default]
    Client,
    Disabled,
}

/// Invoke a closure with the installed retained local runtime.
#[doc(hidden)]
pub fn with_default_local_service_runtime<T>(
    f: impl FnOnce(&LocalServiceRuntime) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let runtime = crate::service_runtime_store::default_runtime()?;
    f(&runtime)
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
}

#[derive(Clone)]
pub struct LocalServiceRuntime {
    pub(crate) message_store: std::sync::Arc<dyn SharedMessageStore + Send + Sync>,
    pub(crate) roster_store: std::sync::Arc<dyn SharedRosterStore + Send + Sync>,
    pub(crate) nudge_template_override_store:
        std::sync::Arc<dyn crate::boundary::NudgeTemplateOverrideStore + Send + Sync>,
    pub(crate) non_claude_outbound:
        std::sync::Arc<dyn crate::boundary::NonClaudeOutbound + Send + Sync>,
    workspace_config_access: WorkspaceConfigAccess,
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
            workspace_config_access: WorkspaceConfigAccess::Client,
        }
    }

    /// Returns the daemon-owned runtime view. A system daemon must not read a
    /// caller-supplied workspace path while handling an IPC or peer request.
    pub fn without_workspace_config(mut self) -> Self {
        self.workspace_config_access = WorkspaceConfigAccess::Disabled;
        self
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

    #[doc(hidden)]
    pub fn shared_roster_store_arc(&self) -> std::sync::Arc<dyn SharedRosterStore + Send + Sync> {
        self.roster_store.clone()
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
        let output_path = (self.path_factory)()?;
        let bytes = serde_json::to_vec(&request)?;
        if bytes.len() > MAX_NON_CLAUDE_PAYLOAD_BYTES {
            return Err(AtmError::mailbox_write(format!(
                "non-Claude outbound payload for {} exceeded {MAX_NON_CLAUDE_PAYLOAD_BYTES} bytes",
                output_path.display()
            )));
        }
        let parent = output_path.parent().ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "non-Claude outbound path {} has no parent directory",
                output_path.display()
            ))
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to create non-Claude outbound directory {}: {error}",
                parent.display()
            ))
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
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create notification log directory {}: {error}",
            parent.display()
        ))
    })?;
    crate::mailbox::atomic::append_jsonl_record(path, event)
}

impl RetainedServiceRuntime for LocalServiceRuntime {
    fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
        load_workspace_config(self.workspace_config_access, current_dir)
    }

    fn load_nudge_template_override(
        &self,
        team: &TeamName,
        kind: crate::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<Option<crate::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
        self.nudge_template_override_store
            .load_template_override(team, kind)
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
}

fn load_workspace_config(
    access: WorkspaceConfigAccess,
    current_dir: &Path,
) -> Result<Option<AtmConfig>, AtmError> {
    match access {
        WorkspaceConfigAccess::Client => config::load_config(current_dir),
        WorkspaceConfigAccess::Disabled => Ok(None),
    }
}

#[cfg(test)]
mod workspace_config_tests {
    use super::{WorkspaceConfigAccess, load_workspace_config};

    #[test]
    fn disabled_runtime_never_reads_a_callers_workspace_config() {
        let workspace = tempfile::tempdir().expect("workspace");
        std::fs::write(workspace.path().join(".atm.toml"), "not valid toml = [")
            .expect("fixture config");

        let config = load_workspace_config(WorkspaceConfigAccess::Disabled, workspace.path())
            .expect("daemon config access is disabled");

        assert!(config.is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LocalFileNonClaudeOutbound, MAX_NON_CLAUDE_PAYLOAD_BYTES, append_notification_log_at_path,
    };
    use crate::error_codes::AtmErrorCode;
    use crate::protocol::{NotificationEvent, NotificationKind};
    use crate::schema::InboxMessage;
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use chrono::Utc;
    use tempfile::tempdir;

    fn message() -> InboxMessage {
        InboxMessage {
            from: "sender".parse::<AgentName>().expect("sender"),
            source_chat_id: None,
            text: "hello".to_string(),
            timestamp: IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some("test-team".parse::<TeamName>().expect("team")),
            destination_chat_id: None,
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

        assert_eq!(error.code(), AtmErrorCode::MailboxWriteFailed);
        assert!(error.message().contains("exceeded"));
        assert!(error.message().contains("Recovery:"));
        assert!(!output_path.exists());
    }
}
