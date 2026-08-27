use crate::boundary::{RosterEntry, RosterHarness};
use crate::delivery_channel::{
    DeliveryChannel, GraftLeaseState, classify_delivery_channel, local_message_received_backend,
};
use crate::error::AtmError;
use crate::provenance::ValidatedWriteProvenance;
use crate::schema::{AtmMessageId, ThreadMode};
use crate::service_runtime::RetainedServiceRuntime;
use crate::types::{AgentName, PaneId, TeamName};

#[expect(
    dead_code,
    reason = "Phase Y.4 keeps the full documented event-family inventory explicit even before every family is wired into a live caller."
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryEventFamily {
    NewMessage,
    ThreadUpdate,
    AckReply,
    InboxRepair,
    RestoreInboxRebuild,
}

impl DeliveryEventFamily {
    pub(crate) fn action_name(self) -> &'static str {
        match self {
            Self::NewMessage => "new_message",
            Self::ThreadUpdate => "thread_update",
            Self::AckReply => "ack_reply",
            Self::InboxRepair => "inbox_repair",
            Self::RestoreInboxRebuild => "restore_inbox_rebuild",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryHarnessPath {
    ClaudeCode,
    NonClaude,
}

impl DeliveryHarnessPath {
    pub(crate) fn from_roster_harness(harness: RosterHarness) -> Self {
        match harness {
            RosterHarness::ClaudeCode => Self::ClaudeCode,
            RosterHarness::CodexCli
            | RosterHarness::GeminiCli
            | RosterHarness::Opencode
            | RosterHarness::Hermes
            | RosterHarness::PythonGraft => Self::NonClaude,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeliveryRecipientSnapshot {
    pub(crate) agent: AgentName,
    pub(crate) team: TeamName,
    pub(crate) harness: DeliveryHarnessPath,
    pub(crate) recipient_pane_id: Option<PaneId>,
    pub(crate) local_tmux_post_send: bool,
    pub(crate) local_herdr_post_send: bool,
    pub(crate) herdr_session: Option<String>,
    pub(crate) graft_post_send: bool,
    pub(crate) roster_backed: bool,
}

impl DeliveryRecipientSnapshot {
    pub(crate) fn remote(agent: AgentName, team: TeamName) -> Self {
        Self {
            agent,
            team,
            harness: DeliveryHarnessPath::NonClaude,
            recipient_pane_id: None,
            local_tmux_post_send: false,
            local_herdr_post_send: false,
            herdr_session: None,
            graft_post_send: false,
            roster_backed: false,
        }
    }

    /// Historical AQ0-era backend-selection flags kept for existing send/nudge
    /// callers. They are projected from the canonical AQ1 delivery-channel
    /// classifier so immediate and deferred routing cannot diverge.
    fn from_roster(member: RosterEntry) -> Self {
        let local_backend = local_message_received_backend(&member);
        let graft_lease = if matches!(
            member.harness,
            RosterHarness::CodexCli
                | RosterHarness::GeminiCli
                | RosterHarness::Opencode
                | RosterHarness::Hermes
                | RosterHarness::PythonGraft
        ) {
            GraftLeaseState::Active
        } else {
            GraftLeaseState::Absent
        };
        let delivery_channel = classify_delivery_channel(local_backend.as_ref(), graft_lease);
        let local_tmux_post_send = delivery_channel == DeliveryChannel::TmuxSteer;
        let local_herdr_post_send = delivery_channel == DeliveryChannel::HerdrSteer;
        let herdr_session = match local_backend.as_ref() {
            Some(crate::delivery_channel::LocalMessageReceivedBackend::Herdr { session }) => {
                session.as_ref().map(ToString::to_string)
            }
            _ => None,
        };
        let graft_post_send = delivery_channel == DeliveryChannel::Graft;
        Self {
            agent: member.agent_name,
            team: member.team_name,
            harness: DeliveryHarnessPath::from_roster_harness(member.harness),
            recipient_pane_id: member.recipient_pane_id,
            local_tmux_post_send,
            local_herdr_post_send,
            herdr_session,
            graft_post_send,
            roster_backed: true,
        }
    }
}

#[expect(
    dead_code,
    reason = "Phase Y.4 keeps the full documented coordinator-state inventory explicit even before every branch is exercised by runtime callers."
)]
// Harness-target compatibility is still validated at runtime by
// `validate_delivery_target(...)`; this state enum documents the coordinator
// flow, but it does not yet encode harness pairing as a typestate invariant.
// A future typestate pass can tighten that contract without widening the live
// delivery surface during Phase Y closeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewMessageCoordinatorState {
    Received,
    ResolveHarness,
    DispatchClaude,
    DispatchNonClaude,
    Completed,
    Rejected,
}

impl NewMessageCoordinatorState {
    fn transition_name(self, harness: DeliveryHarnessPath) -> &'static str {
        match (self, harness) {
            (Self::Received, _) => "delivery_policy.new_message.received",
            (Self::ResolveHarness, DeliveryHarnessPath::ClaudeCode)
            | (Self::DispatchClaude, DeliveryHarnessPath::ClaudeCode) => {
                "delivery_policy.new_message.harness_claude"
            }
            (Self::ResolveHarness, DeliveryHarnessPath::NonClaude)
            | (Self::DispatchNonClaude, DeliveryHarnessPath::NonClaude) => {
                "delivery_policy.new_message.harness_non_claude"
            }
            (Self::Completed, _) => "delivery_policy.new_message.completed",
            (Self::Rejected, _) => "delivery_policy.new_message.rejected",
            (Self::DispatchClaude, DeliveryHarnessPath::NonClaude)
            | (Self::DispatchNonClaude, DeliveryHarnessPath::ClaudeCode) => {
                "delivery_policy.new_message.rejected"
            }
        }
    }
}

#[expect(
    dead_code,
    reason = "Phase Y.4 keeps the full documented Claude-harness state inventory explicit even before every failure branch is exercised by runtime callers."
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudeHarnessNewMessageState {
    Received,
    PersistSqlite,
    SqliteCommitted,
    AppendCompatibilityMessage,
    AppendCompatibilityErrorMessage,
    RunPrimaryNudge,
    RunErrorNudge,
    RunPostSendHookFallback,
    Delivered,
    Failed,
}

impl ClaudeHarnessNewMessageState {
    fn transition_name(self) -> &'static str {
        match self {
            Self::Received => "delivery_policy.new_message.received",
            Self::PersistSqlite => "delivery_policy.new_message.persist_sqlite",
            Self::SqliteCommitted => "delivery_policy.new_message.sqlite_committed",
            Self::AppendCompatibilityMessage => {
                "delivery_policy.new_message.compat_append_original"
            }
            Self::AppendCompatibilityErrorMessage => {
                "delivery_policy.new_message.compat_append_error"
            }
            Self::RunPrimaryNudge => "delivery_policy.new_message.primary_nudge",
            Self::RunErrorNudge => "delivery_policy.new_message.error_nudge",
            Self::RunPostSendHookFallback => "delivery_policy.new_message.post_send_hook_fallback",
            Self::Delivered => "delivery_policy.new_message.delivered",
            Self::Failed => "delivery_policy.new_message.failed",
        }
    }
}

#[expect(
    dead_code,
    reason = "Phase Y.4 keeps the full documented non-Claude state inventory explicit even before every failure branch is exercised by runtime callers."
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NonClaudeHarnessNewMessageState {
    Received,
    PersistSqlite,
    SqliteCommitted,
    DeliverOriginal,
    DeliverErrorMessage,
    RunPrimaryNudge,
    RunErrorNudge,
    Delivered,
    Failed,
}

impl NonClaudeHarnessNewMessageState {
    fn transition_name(self) -> &'static str {
        match self {
            Self::Received => "delivery_policy.new_message.received",
            Self::PersistSqlite => "delivery_policy.new_message.persist_sqlite",
            Self::SqliteCommitted => "delivery_policy.new_message.sqlite_committed",
            Self::DeliverOriginal => "delivery_policy.new_message.non_claude_original",
            Self::DeliverErrorMessage => "delivery_policy.new_message.non_claude_error",
            Self::RunPrimaryNudge => "delivery_policy.new_message.primary_nudge",
            Self::RunErrorNudge => "delivery_policy.new_message.error_nudge",
            Self::Delivered => "delivery_policy.new_message.delivered",
            Self::Failed => "delivery_policy.new_message.failed",
        }
    }
}

#[expect(
    dead_code,
    reason = "Phase Y.4 keeps the full documented thread-update state inventory explicit even before every failure branch is exercised by runtime callers."
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadUpdateStateMachine {
    Received,
    ValidateParentExists,
    ValidateRootExists,
    ValidateOriginalSender,
    ValidateLinearSuccessor,
    PersistSqlite,
    DispatchByHarness,
    Delivered,
    Rejected,
    Failed,
}

impl ThreadUpdateStateMachine {
    fn transition_name(self) -> &'static str {
        match self {
            Self::Received => "delivery_policy.thread_update.received",
            Self::ValidateParentExists => "delivery_policy.thread_update.validate_parent_exists",
            Self::ValidateRootExists => "delivery_policy.thread_update.validate_root_exists",
            Self::ValidateOriginalSender => {
                "delivery_policy.thread_update.validate_original_sender"
            }
            Self::ValidateLinearSuccessor => {
                "delivery_policy.thread_update.validate_linear_successor"
            }
            Self::PersistSqlite => "delivery_policy.thread_update.persist_sqlite",
            Self::DispatchByHarness => "delivery_policy.thread_update.dispatch_by_harness",
            Self::Delivered => "delivery_policy.thread_update.delivered",
            Self::Rejected => "delivery_policy.thread_update.rejected",
            Self::Failed => "delivery_policy.thread_update.failed",
        }
    }
}

#[expect(
    dead_code,
    reason = "Phase Y.4 keeps the full documented ack-reply state inventory explicit even before every failure branch is exercised by runtime callers."
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AckReplyStateMachine {
    Received,
    ValidateAckTargetExists,
    ValidateReplyTargetAllowed,
    PersistAckTransition,
    BuildReplyDeliveryRequest,
    DispatchReplyByHarness,
    Delivered,
    Rejected,
    Failed,
}

impl AckReplyStateMachine {
    fn transition_name(self) -> &'static str {
        match self {
            Self::Received => "delivery_policy.ack_reply.received",
            Self::ValidateAckTargetExists => "delivery_policy.ack_reply.validate_ack_target_exists",
            Self::ValidateReplyTargetAllowed => {
                "delivery_policy.ack_reply.validate_reply_target_allowed"
            }
            Self::PersistAckTransition => "delivery_policy.ack_reply.persist_ack_transition",
            Self::BuildReplyDeliveryRequest => {
                "delivery_policy.ack_reply.build_reply_delivery_request"
            }
            Self::DispatchReplyByHarness => "delivery_policy.ack_reply.dispatch_reply_by_harness",
            Self::Delivered => "delivery_policy.ack_reply.delivered",
            Self::Rejected => "delivery_policy.ack_reply.rejected",
            Self::Failed => "delivery_policy.ack_reply.failed",
        }
    }
}

#[expect(
    dead_code,
    reason = "Phase Y.4 keeps the full documented inbox-repair state inventory explicit even before every branch is exercised by runtime callers."
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboxRepairStateMachine {
    Received,
    ValidateClaudeHarness,
    LoadRepairProjection,
    FilterDeletedMessages,
    StageInboxRebuild,
    PublishInboxRebuild,
    Delivered,
    Rejected,
    Failed,
}

#[expect(
    dead_code,
    reason = "Phase Y.4 keeps the full documented restore-rebuild state inventory explicit even before every branch is exercised by runtime callers."
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestoreInboxRebuildStateMachine {
    Received,
    ValidateRestoreMarker,
    ValidateClaudeHarness,
    LoadRestoreProjection,
    StageRestoreOutput,
    PublishRestoreOutput,
    Delivered,
    Rejected,
    Failed,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct DeliveryPolicyCoordinator;

impl DeliveryPolicyCoordinator {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn resolve_recipient_snapshot<R: RetainedServiceRuntime + ?Sized>(
        &self,
        runtime: &R,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<DeliveryRecipientSnapshot, AtmError> {
        runtime
            .load_roster_member(team, agent)?
            .map(DeliveryRecipientSnapshot::from_roster)
            .ok_or_else(|| AtmError::agent_not_found(agent, team))
    }

    /// Resolves the persistence-admission snapshot for the canonical writer.
    ///
    /// This is not a delivery action: `PostWriteRouter` alone selects local
    /// nudge versus peer HTTPS after persistence. A local destination must be
    /// validated against this host's roster before it is written. A
    /// host-qualified origin destination cannot be validated locally, so it
    /// retains its immutable origin record and the receiving host validates
    /// its own recipient roster after peer delivery.
    pub(crate) fn resolve_write_recipient_snapshot<R: RetainedServiceRuntime + ?Sized>(
        &self,
        runtime: &R,
        recipient: &crate::send::ResolvedRecipient,
        provenance: ValidatedWriteProvenance,
    ) -> Result<DeliveryRecipientSnapshot, AtmError> {
        if provenance.is_remote_origin() {
            return Ok(DeliveryRecipientSnapshot::remote(
                recipient.agent.clone(),
                recipient.team.clone(),
            ));
        }
        self.resolve_recipient_snapshot(runtime, &recipient.team, &recipient.agent)
    }

    #[allow(
        dead_code,
        reason = "Phase Y.4 keeps the documented event-family resolver explicit even when later branches dispatch directly from pre-resolved caller context."
    )]
    pub(crate) fn resolve_send_family(
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
    ) -> DeliveryEventFamily {
        match (parent_message_id, thread_mode) {
            (Some(_), Some(_)) => DeliveryEventFamily::ThreadUpdate,
            _ => DeliveryEventFamily::NewMessage,
        }
    }
}

#[cfg(test)]
pub(crate) fn new_message_success_transitions(
    harness: DeliveryHarnessPath,
) -> &'static [&'static str] {
    match harness {
        DeliveryHarnessPath::ClaudeCode => &[
            "delivery_policy.new_message.received",
            "delivery_policy.new_message.harness_claude",
            "delivery_policy.new_message.sqlite_committed",
            "delivery_policy.new_message.compat_append_original",
            "delivery_policy.new_message.primary_nudge",
            "delivery_policy.new_message.delivered",
        ],
        DeliveryHarnessPath::NonClaude => &[
            "delivery_policy.new_message.received",
            "delivery_policy.new_message.harness_non_claude",
            "delivery_policy.new_message.sqlite_committed",
            "delivery_policy.new_message.non_claude_original",
            "delivery_policy.new_message.primary_nudge",
            "delivery_policy.new_message.delivered",
        ],
    }
}

fn new_message_persisted_success_transitions(harness: DeliveryHarnessPath) -> Vec<&'static str> {
    match harness {
        DeliveryHarnessPath::ClaudeCode => vec![
            NewMessageCoordinatorState::Received.transition_name(harness),
            NewMessageCoordinatorState::ResolveHarness.transition_name(harness),
            ClaudeHarnessNewMessageState::SqliteCommitted.transition_name(),
            ClaudeHarnessNewMessageState::AppendCompatibilityMessage.transition_name(),
            ClaudeHarnessNewMessageState::RunPrimaryNudge.transition_name(),
            ClaudeHarnessNewMessageState::Delivered.transition_name(),
        ],
        DeliveryHarnessPath::NonClaude => vec![
            NewMessageCoordinatorState::Received.transition_name(harness),
            NewMessageCoordinatorState::ResolveHarness.transition_name(harness),
            NonClaudeHarnessNewMessageState::SqliteCommitted.transition_name(),
            NonClaudeHarnessNewMessageState::DeliverOriginal.transition_name(),
            NonClaudeHarnessNewMessageState::RunPrimaryNudge.transition_name(),
            NonClaudeHarnessNewMessageState::Delivered.transition_name(),
        ],
    }
}

fn inbox_repair_transition_name(state: InboxRepairStateMachine) -> &'static str {
    match state {
        InboxRepairStateMachine::Received => "delivery_policy.inbox_repair.received",
        InboxRepairStateMachine::ValidateClaudeHarness => {
            "delivery_policy.inbox_repair.validate_claude_harness"
        }
        InboxRepairStateMachine::LoadRepairProjection => {
            "delivery_policy.inbox_repair.load_repair_projection"
        }
        InboxRepairStateMachine::FilterDeletedMessages => {
            "delivery_policy.inbox_repair.filter_deleted_messages"
        }
        InboxRepairStateMachine::StageInboxRebuild => {
            "delivery_policy.inbox_repair.stage_inbox_rebuild"
        }
        InboxRepairStateMachine::PublishInboxRebuild => {
            "delivery_policy.inbox_repair.publish_inbox_rebuild"
        }
        InboxRepairStateMachine::Delivered => "delivery_policy.inbox_repair.delivered",
        InboxRepairStateMachine::Rejected => "delivery_policy.inbox_repair.rejected",
        InboxRepairStateMachine::Failed => "delivery_policy.inbox_repair.failed",
    }
}

fn restore_inbox_rebuild_transition_name(state: RestoreInboxRebuildStateMachine) -> &'static str {
    match state {
        RestoreInboxRebuildStateMachine::Received => {
            "delivery_policy.restore_inbox_rebuild.received"
        }
        RestoreInboxRebuildStateMachine::ValidateRestoreMarker => {
            "delivery_policy.restore_inbox_rebuild.validate_restore_marker"
        }
        RestoreInboxRebuildStateMachine::ValidateClaudeHarness => {
            "delivery_policy.restore_inbox_rebuild.validate_claude_harness"
        }
        RestoreInboxRebuildStateMachine::LoadRestoreProjection => {
            "delivery_policy.restore_inbox_rebuild.load_restore_projection"
        }
        RestoreInboxRebuildStateMachine::StageRestoreOutput => {
            "delivery_policy.restore_inbox_rebuild.stage_restore_output"
        }
        RestoreInboxRebuildStateMachine::PublishRestoreOutput => {
            "delivery_policy.restore_inbox_rebuild.publish_restore_output"
        }
        RestoreInboxRebuildStateMachine::Delivered => {
            "delivery_policy.restore_inbox_rebuild.delivered"
        }
        RestoreInboxRebuildStateMachine::Rejected => {
            "delivery_policy.restore_inbox_rebuild.rejected"
        }
        RestoreInboxRebuildStateMachine::Failed => "delivery_policy.restore_inbox_rebuild.failed",
    }
}

fn inbox_repair_persisted_success_transitions() -> Vec<&'static str> {
    inbox_repair_transitions()
        .iter()
        .copied()
        .map(inbox_repair_transition_name)
        .collect()
}

fn restore_inbox_rebuild_persisted_success_transitions() -> Vec<&'static str> {
    restore_inbox_rebuild_transitions()
        .iter()
        .copied()
        .map(restore_inbox_rebuild_transition_name)
        .collect()
}

pub(crate) fn persisted_success_transition_names(
    family: DeliveryEventFamily,
    harness: DeliveryHarnessPath,
) -> Vec<&'static str> {
    match family {
        DeliveryEventFamily::NewMessage => new_message_persisted_success_transitions(harness),
        DeliveryEventFamily::ThreadUpdate => thread_update_transitions()
            .iter()
            .copied()
            .map(ThreadUpdateStateMachine::transition_name)
            .collect(),
        DeliveryEventFamily::AckReply => ack_reply_transitions()
            .iter()
            .copied()
            .map(AckReplyStateMachine::transition_name)
            .collect(),
        DeliveryEventFamily::InboxRepair => inbox_repair_persisted_success_transitions(),
        DeliveryEventFamily::RestoreInboxRebuild => {
            restore_inbox_rebuild_persisted_success_transitions()
        }
    }
}

#[cfg(test)]
pub(crate) fn claude_append_failure_transition_names() -> &'static [&'static str] {
    &[
        "delivery_policy.new_message.received",
        "delivery_policy.new_message.harness_claude",
        "delivery_policy.new_message.sqlite_committed",
        "delivery_policy.new_message.compat_append_original",
        "delivery_policy.new_message.post_send_hook_fallback",
        "delivery_policy.new_message.failed",
    ]
}

#[cfg(test)]
pub(crate) fn append_failure_transitions() -> &'static [&'static str] {
    claude_append_failure_transition_names()
}

pub(crate) fn thread_update_transitions() -> &'static [ThreadUpdateStateMachine] {
    &[
        ThreadUpdateStateMachine::Received,
        ThreadUpdateStateMachine::ValidateParentExists,
        ThreadUpdateStateMachine::ValidateRootExists,
        ThreadUpdateStateMachine::ValidateOriginalSender,
        ThreadUpdateStateMachine::ValidateLinearSuccessor,
        ThreadUpdateStateMachine::PersistSqlite,
        ThreadUpdateStateMachine::DispatchByHarness,
        ThreadUpdateStateMachine::Delivered,
    ]
}

pub(crate) fn ack_reply_transitions() -> &'static [AckReplyStateMachine] {
    &[
        AckReplyStateMachine::Received,
        AckReplyStateMachine::ValidateAckTargetExists,
        AckReplyStateMachine::ValidateReplyTargetAllowed,
        AckReplyStateMachine::PersistAckTransition,
        AckReplyStateMachine::BuildReplyDeliveryRequest,
        AckReplyStateMachine::DispatchReplyByHarness,
        AckReplyStateMachine::Delivered,
    ]
}

pub(crate) fn inbox_repair_transitions() -> &'static [InboxRepairStateMachine] {
    &[
        InboxRepairStateMachine::Received,
        InboxRepairStateMachine::ValidateClaudeHarness,
        InboxRepairStateMachine::LoadRepairProjection,
        InboxRepairStateMachine::FilterDeletedMessages,
        InboxRepairStateMachine::StageInboxRebuild,
        InboxRepairStateMachine::PublishInboxRebuild,
        InboxRepairStateMachine::Delivered,
    ]
}

pub(crate) fn restore_inbox_rebuild_transitions() -> &'static [RestoreInboxRebuildStateMachine] {
    &[
        RestoreInboxRebuildStateMachine::Received,
        RestoreInboxRebuildStateMachine::ValidateRestoreMarker,
        RestoreInboxRebuildStateMachine::ValidateClaudeHarness,
        RestoreInboxRebuildStateMachine::LoadRestoreProjection,
        RestoreInboxRebuildStateMachine::StageRestoreOutput,
        RestoreInboxRebuildStateMachine::PublishRestoreOutput,
        RestoreInboxRebuildStateMachine::Delivered,
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        AckReplyStateMachine, DeliveryEventFamily, DeliveryHarnessPath, DeliveryPolicyCoordinator,
        DeliveryRecipientSnapshot, InboxRepairStateMachine, NewMessageCoordinatorState,
        RestoreInboxRebuildStateMachine, ack_reply_transitions, append_failure_transitions,
        inbox_repair_transitions, new_message_success_transitions,
        restore_inbox_rebuild_transitions, thread_update_transitions,
    };
    use crate::error::AtmError;
    use crate::schema::ThreadMode;
    use crate::service_runtime::RetainedServiceRuntime;
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use crate::{
        boundary::{RosterEntry, RosterHarness, RosterMemberKind},
        config::AtmConfig,
    };
    use atm_storage::contract::AgentType;
    use serde_json::Map;
    use std::path::{Path, PathBuf};

    struct MissingRosterRuntime;

    impl crate::boundary::sealed::Sealed for MissingRosterRuntime {}

    impl RetainedServiceRuntime for MissingRosterRuntime {
        fn load_config(&self, _current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
            Ok(None)
        }

        fn inbox_path(
            &self,
            home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            Ok(home_dir.join("inbox.jsonl"))
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &DeliveryRecipientSnapshot,
            _messages: &[crate::schema::InboxMessage],
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn load_roster_member(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<RosterEntry>, AtmError> {
            Ok(None)
        }

        fn load_team_roster(&self, _team: &TeamName) -> Result<Vec<RosterEntry>, AtmError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn send_family_defaults_to_new_message_without_thread_fields() {
        assert_eq!(
            DeliveryPolicyCoordinator::resolve_send_family(None, None),
            DeliveryEventFamily::NewMessage
        );
    }

    #[test]
    fn send_family_uses_thread_update_for_threaded_send() {
        assert_eq!(
            DeliveryPolicyCoordinator::resolve_send_family(
                Some(crate::schema::AtmMessageId::new()),
                Some(ThreadMode::AddDetails),
            ),
            DeliveryEventFamily::ThreadUpdate
        );
    }

    #[test]
    fn claude_success_path_keeps_compat_append_transition() {
        assert_eq!(
            new_message_success_transitions(DeliveryHarnessPath::ClaudeCode),
            &[
                "delivery_policy.new_message.received",
                "delivery_policy.new_message.harness_claude",
                "delivery_policy.new_message.sqlite_committed",
                "delivery_policy.new_message.compat_append_original",
                "delivery_policy.new_message.primary_nudge",
                "delivery_policy.new_message.delivered",
            ]
        );
    }

    #[test]
    fn non_claude_success_path_skips_compat_append_transition() {
        assert_eq!(
            new_message_success_transitions(DeliveryHarnessPath::NonClaude),
            &[
                "delivery_policy.new_message.received",
                "delivery_policy.new_message.harness_non_claude",
                "delivery_policy.new_message.sqlite_committed",
                "delivery_policy.new_message.non_claude_original",
                "delivery_policy.new_message.primary_nudge",
                "delivery_policy.new_message.delivered",
            ]
        );
    }

    #[test]
    fn python_graft_harnesses_use_graft_delivery_without_tmux() {
        for harness in [RosterHarness::Hermes, RosterHarness::PythonGraft] {
            let entry = RosterEntry {
                team_name: "team-a".parse().expect("team"),
                agent_name: "python-agent".parse().expect("agent"),
                member_kind: RosterMemberKind::Permanent,
                harness,
                agent_type: AgentType::Worker,
                model: crate::types::ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            };
            let snapshot = DeliveryRecipientSnapshot::from_roster(entry);
            assert_eq!(snapshot.harness, DeliveryHarnessPath::NonClaude);
            assert!(snapshot.graft_post_send);
        }
    }

    #[test]
    fn herdr_backend_wins_over_non_claude_graft_fallback() {
        let mut metadata = Map::new();
        metadata.insert(
            crate::delivery_channel::BACKEND_TYPE_METADATA_KEY.to_owned(),
            serde_json::json!("herdr"),
        );
        let entry = RosterEntry {
            team_name: "team-a".parse().expect("team"),
            agent_name: crate::test_support::TEST_SENDER.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::CodexCli,
            agent_type: AgentType::Worker,
            model: crate::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json: metadata,
        };
        let snapshot = DeliveryRecipientSnapshot::from_roster(entry);
        assert!(!snapshot.local_tmux_post_send);
        assert!(snapshot.local_herdr_post_send);
        assert!(!snapshot.graft_post_send);
    }

    #[test]
    fn append_failure_routes_to_post_send_hook_fallback_only() {
        assert_eq!(
            append_failure_transitions(),
            &[
                "delivery_policy.new_message.received",
                "delivery_policy.new_message.harness_claude",
                "delivery_policy.new_message.sqlite_committed",
                "delivery_policy.new_message.compat_append_original",
                "delivery_policy.new_message.post_send_hook_fallback",
                "delivery_policy.new_message.failed",
            ]
        );
    }

    #[test]
    fn state_machine_sequences_stay_auditable() {
        assert_eq!(
            thread_update_transitions(),
            &[
                super::ThreadUpdateStateMachine::Received,
                super::ThreadUpdateStateMachine::ValidateParentExists,
                super::ThreadUpdateStateMachine::ValidateRootExists,
                super::ThreadUpdateStateMachine::ValidateOriginalSender,
                super::ThreadUpdateStateMachine::ValidateLinearSuccessor,
                super::ThreadUpdateStateMachine::PersistSqlite,
                super::ThreadUpdateStateMachine::DispatchByHarness,
                super::ThreadUpdateStateMachine::Delivered,
            ]
        );
        assert_eq!(
            ack_reply_transitions(),
            &[
                AckReplyStateMachine::Received,
                AckReplyStateMachine::ValidateAckTargetExists,
                AckReplyStateMachine::ValidateReplyTargetAllowed,
                AckReplyStateMachine::PersistAckTransition,
                AckReplyStateMachine::BuildReplyDeliveryRequest,
                AckReplyStateMachine::DispatchReplyByHarness,
                AckReplyStateMachine::Delivered,
            ]
        );
        assert_eq!(
            inbox_repair_transitions(),
            &[
                InboxRepairStateMachine::Received,
                InboxRepairStateMachine::ValidateClaudeHarness,
                InboxRepairStateMachine::LoadRepairProjection,
                InboxRepairStateMachine::FilterDeletedMessages,
                InboxRepairStateMachine::StageInboxRebuild,
                InboxRepairStateMachine::PublishInboxRebuild,
                InboxRepairStateMachine::Delivered,
            ]
        );
        assert_eq!(
            restore_inbox_rebuild_transitions(),
            &[
                RestoreInboxRebuildStateMachine::Received,
                RestoreInboxRebuildStateMachine::ValidateRestoreMarker,
                RestoreInboxRebuildStateMachine::ValidateClaudeHarness,
                RestoreInboxRebuildStateMachine::LoadRestoreProjection,
                RestoreInboxRebuildStateMachine::StageRestoreOutput,
                RestoreInboxRebuildStateMachine::PublishRestoreOutput,
                RestoreInboxRebuildStateMachine::Delivered,
            ]
        );
    }

    #[test]
    fn coordinator_state_enum_remains_explicit() {
        assert_eq!(
            NewMessageCoordinatorState::ResolveHarness,
            NewMessageCoordinatorState::ResolveHarness
        );
    }

    #[test]
    fn missing_roster_member_returns_actionable_recovery_contract() {
        let error = DeliveryPolicyCoordinator::new()
            .resolve_recipient_snapshot(
                &MissingRosterRuntime,
                &TeamName::from_validated("test-team"),
                &AgentName::from_validated("recipient"),
            )
            .expect_err("missing roster member must fail");

        assert!(error.code() == crate::error_codes::AtmErrorCode::AgentNotFound);
        assert!(
            error
                .message()
                .starts_with("agent 'recipient' was not found in team 'test-team'")
        );
        assert!(error.message().contains("Recovery:"));
    }
}
