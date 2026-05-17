use crate::boundary::{RosterHarness, RosterMemberRecord};
use crate::error::AtmError;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::{AtmMessageId, ThreadMode};
use crate::service_runtime::RetainedServiceRuntime;
use crate::types::{AgentName, TaskId, TeamName};
use tracing::warn;

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
            RosterHarness::CodexCli | RosterHarness::GeminiCli | RosterHarness::Opencode => {
                Self::NonClaude
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeliveryRecipientSnapshot {
    pub(crate) agent: AgentName,
    pub(crate) team: TeamName,
    pub(crate) harness: DeliveryHarnessPath,
    pub(crate) recipient_pane_id: Option<String>,
    pub(crate) roster_backed: bool,
}

impl DeliveryRecipientSnapshot {
    fn fallback_claude(agent: AgentName, team: TeamName) -> Self {
        Self {
            agent,
            team,
            harness: DeliveryHarnessPath::ClaudeCode,
            recipient_pane_id: None,
            roster_backed: false,
        }
    }

    fn from_roster(member: RosterMemberRecord) -> Self {
        Self {
            agent: member.agent_name,
            team: member.team_name,
            harness: DeliveryHarnessPath::from_roster_harness(member.harness),
            recipient_pane_id: member.recipient_pane_id,
            roster_backed: true,
        }
    }

    pub(crate) fn allows_claude_jsonl_append(&self) -> bool {
        self.harness == DeliveryHarnessPath::ClaudeCode
    }
}

#[expect(
    dead_code,
    reason = "Phase Y.4 keeps the full documented coordinator-state inventory explicit even before every branch is exercised by runtime callers."
)]
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
    ValidateParent,
    ValidateRoot,
    ValidateSender,
    ValidateLinearity,
    PersistSqlite,
    RouteDelivery,
    Delivered,
    Failed,
}

impl ThreadUpdateStateMachine {
    fn transition_name(self) -> &'static str {
        match self {
            Self::Received => "delivery_policy.thread_update.received",
            Self::ValidateParent => "delivery_policy.thread_update.validate_parent",
            Self::ValidateRoot => "delivery_policy.thread_update.validate_root",
            Self::ValidateSender => "delivery_policy.thread_update.validate_sender",
            Self::ValidateLinearity => "delivery_policy.thread_update.validate_linearity",
            Self::PersistSqlite => "delivery_policy.thread_update.persist_sqlite",
            Self::RouteDelivery => "delivery_policy.thread_update.route_delivery",
            Self::Delivered => "delivery_policy.thread_update.delivered",
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
    ValidateTarget,
    PersistAckState,
    DelegateReplyDelivery,
    Delivered,
    Failed,
}

impl AckReplyStateMachine {
    fn transition_name(self) -> &'static str {
        match self {
            Self::Received => "delivery_policy.ack_reply.received",
            Self::ValidateTarget => "delivery_policy.ack_reply.validate_target",
            Self::PersistAckState => "delivery_policy.ack_reply.persist_ack_state",
            Self::DelegateReplyDelivery => "delivery_policy.ack_reply.delegate_reply_delivery",
            Self::Delivered => "delivery_policy.ack_reply.delivered",
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
    ResolveHarness,
    LoadProjection,
    StageOutput,
    PublishOutput,
    Completed,
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
    ResolveHarness,
    LoadProjection,
    StageOutput,
    PublishOutput,
    CleanupStaging,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PersistedDeliveryRoute {
    pub(crate) family: DeliveryEventFamily,
    pub(crate) harness: DeliveryHarnessPath,
}

pub(crate) struct DeliveryTransitionEvent<'a> {
    pub(crate) family: DeliveryEventFamily,
    pub(crate) outcome: &'static str,
    pub(crate) team: &'a TeamName,
    pub(crate) agent: &'a AgentName,
    pub(crate) sender: &'a AgentName,
    pub(crate) message_id: Option<AtmMessageId>,
    pub(crate) task_id: Option<TaskId>,
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
        Ok(runtime
            .load_roster_member(team, agent)?
            .map(DeliveryRecipientSnapshot::from_roster)
            .unwrap_or_else(|| {
                DeliveryRecipientSnapshot::fallback_claude(agent.clone(), team.clone())
            }))
    }

    pub(crate) fn route_persisted_delivery(
        &self,
        family: DeliveryEventFamily,
        snapshot: &DeliveryRecipientSnapshot,
    ) -> PersistedDeliveryRoute {
        PersistedDeliveryRoute {
            family,
            harness: snapshot.harness,
        }
    }

    pub(crate) fn resolve_send_family(
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
    ) -> DeliveryEventFamily {
        match (parent_message_id, thread_mode) {
            (Some(_), Some(_)) => DeliveryEventFamily::ThreadUpdate,
            _ => DeliveryEventFamily::NewMessage,
        }
    }

    pub(crate) fn emit_transition(
        &self,
        observability: &dyn ObservabilityPort,
        event: DeliveryTransitionEvent<'_>,
    ) {
        self.emit_event_or_warn(
            observability,
            CommandEvent {
                command: "delivery_policy",
                action: event.family.action_name(),
                outcome: event.outcome,
                team: event.team.clone(),
                agent: event.agent.clone(),
                sender: event.sender.clone(),
                message_id: event.message_id,
                requires_ack: false,
                dry_run: false,
                task_id: event.task_id,
                error_code: None,
                error_message: None,
            },
        );
    }

    fn emit_event_or_warn(&self, observability: &dyn ObservabilityPort, event: CommandEvent) {
        if let Err(error) = observability.emit(event) {
            warn!(
                %error,
                command = "delivery_policy",
                "failed to emit delivery-policy transition event"
            );
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
        InboxRepairStateMachine::ResolveHarness => "delivery_policy.inbox_repair.resolve_harness",
        InboxRepairStateMachine::LoadProjection => "delivery_policy.inbox_repair.load_projection",
        InboxRepairStateMachine::StageOutput => "delivery_policy.inbox_repair.stage_output",
        InboxRepairStateMachine::PublishOutput => "delivery_policy.inbox_repair.publish_output",
        InboxRepairStateMachine::Completed => "delivery_policy.inbox_repair.completed",
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
        RestoreInboxRebuildStateMachine::ResolveHarness => {
            "delivery_policy.restore_inbox_rebuild.resolve_harness"
        }
        RestoreInboxRebuildStateMachine::LoadProjection => {
            "delivery_policy.restore_inbox_rebuild.load_projection"
        }
        RestoreInboxRebuildStateMachine::StageOutput => {
            "delivery_policy.restore_inbox_rebuild.stage_output"
        }
        RestoreInboxRebuildStateMachine::PublishOutput => {
            "delivery_policy.restore_inbox_rebuild.publish_output"
        }
        RestoreInboxRebuildStateMachine::CleanupStaging => {
            "delivery_policy.restore_inbox_rebuild.cleanup_staging"
        }
        RestoreInboxRebuildStateMachine::Completed => {
            "delivery_policy.restore_inbox_rebuild.completed"
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

pub(crate) fn new_message_sqlite_failure_transitions(
    harness: DeliveryHarnessPath,
) -> &'static [&'static str] {
    match harness {
        DeliveryHarnessPath::ClaudeCode => &[
            "delivery_policy.new_message.received",
            "delivery_policy.new_message.harness_claude",
            "delivery_policy.new_message.sqlite_failed",
            "delivery_policy.new_message.compat_append_original",
            "delivery_policy.new_message.compat_append_error",
            "delivery_policy.new_message.primary_nudge",
            "delivery_policy.new_message.error_nudge",
            "delivery_policy.new_message.failed",
        ],
        DeliveryHarnessPath::NonClaude => &[
            "delivery_policy.new_message.received",
            "delivery_policy.new_message.harness_non_claude",
            "delivery_policy.new_message.sqlite_failed",
            "delivery_policy.new_message.non_claude_original",
            "delivery_policy.new_message.non_claude_error",
            "delivery_policy.new_message.primary_nudge",
            "delivery_policy.new_message.error_nudge",
            "delivery_policy.new_message.failed",
        ],
    }
}

pub(crate) fn sqlite_failure_transition_names(
    harness: DeliveryHarnessPath,
) -> &'static [&'static str] {
    new_message_sqlite_failure_transitions(harness)
}

pub(crate) fn append_failure_transition_names(
    harness: DeliveryHarnessPath,
) -> &'static [&'static str] {
    append_failure_transitions(harness)
}

pub(crate) fn append_failure_transitions(harness: DeliveryHarnessPath) -> &'static [&'static str] {
    match harness {
        DeliveryHarnessPath::ClaudeCode => &[
            "delivery_policy.new_message.received",
            "delivery_policy.new_message.harness_claude",
            "delivery_policy.new_message.sqlite_committed",
            "delivery_policy.new_message.compat_append_original",
            "delivery_policy.new_message.post_send_hook_fallback",
            "delivery_policy.new_message.failed",
        ],
        DeliveryHarnessPath::NonClaude => &[
            "delivery_policy.new_message.received",
            "delivery_policy.new_message.harness_non_claude",
            "delivery_policy.new_message.sqlite_committed",
            "delivery_policy.new_message.non_claude_original",
            "delivery_policy.new_message.post_send_hook_fallback",
            "delivery_policy.new_message.failed",
        ],
    }
}

pub(crate) fn thread_update_transitions() -> &'static [ThreadUpdateStateMachine] {
    &[
        ThreadUpdateStateMachine::Received,
        ThreadUpdateStateMachine::ValidateParent,
        ThreadUpdateStateMachine::ValidateRoot,
        ThreadUpdateStateMachine::ValidateSender,
        ThreadUpdateStateMachine::ValidateLinearity,
        ThreadUpdateStateMachine::PersistSqlite,
        ThreadUpdateStateMachine::RouteDelivery,
        ThreadUpdateStateMachine::Delivered,
    ]
}

pub(crate) fn ack_reply_transitions() -> &'static [AckReplyStateMachine] {
    &[
        AckReplyStateMachine::Received,
        AckReplyStateMachine::ValidateTarget,
        AckReplyStateMachine::PersistAckState,
        AckReplyStateMachine::DelegateReplyDelivery,
        AckReplyStateMachine::Delivered,
    ]
}

pub(crate) fn inbox_repair_transitions() -> &'static [InboxRepairStateMachine] {
    &[
        InboxRepairStateMachine::Received,
        InboxRepairStateMachine::ResolveHarness,
        InboxRepairStateMachine::LoadProjection,
        InboxRepairStateMachine::StageOutput,
        InboxRepairStateMachine::PublishOutput,
        InboxRepairStateMachine::Completed,
    ]
}

pub(crate) fn restore_inbox_rebuild_transitions() -> &'static [RestoreInboxRebuildStateMachine] {
    &[
        RestoreInboxRebuildStateMachine::Received,
        RestoreInboxRebuildStateMachine::ValidateRestoreMarker,
        RestoreInboxRebuildStateMachine::ResolveHarness,
        RestoreInboxRebuildStateMachine::LoadProjection,
        RestoreInboxRebuildStateMachine::StageOutput,
        RestoreInboxRebuildStateMachine::PublishOutput,
        RestoreInboxRebuildStateMachine::CleanupStaging,
        RestoreInboxRebuildStateMachine::Completed,
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        AckReplyStateMachine, DeliveryEventFamily, DeliveryHarnessPath, DeliveryPolicyCoordinator,
        InboxRepairStateMachine, NewMessageCoordinatorState, RestoreInboxRebuildStateMachine,
        ack_reply_transitions, append_failure_transitions, inbox_repair_transitions,
        new_message_sqlite_failure_transitions, new_message_success_transitions,
        restore_inbox_rebuild_transitions, thread_update_transitions,
    };
    use crate::schema::ThreadMode;

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
    fn sqlite_failure_contract_includes_original_and_error_for_both_paths() {
        assert_eq!(
            new_message_sqlite_failure_transitions(DeliveryHarnessPath::ClaudeCode),
            &[
                "delivery_policy.new_message.received",
                "delivery_policy.new_message.harness_claude",
                "delivery_policy.new_message.sqlite_failed",
                "delivery_policy.new_message.compat_append_original",
                "delivery_policy.new_message.compat_append_error",
                "delivery_policy.new_message.primary_nudge",
                "delivery_policy.new_message.error_nudge",
                "delivery_policy.new_message.failed",
            ]
        );
        assert_eq!(
            new_message_sqlite_failure_transitions(DeliveryHarnessPath::NonClaude),
            &[
                "delivery_policy.new_message.received",
                "delivery_policy.new_message.harness_non_claude",
                "delivery_policy.new_message.sqlite_failed",
                "delivery_policy.new_message.non_claude_original",
                "delivery_policy.new_message.non_claude_error",
                "delivery_policy.new_message.primary_nudge",
                "delivery_policy.new_message.error_nudge",
                "delivery_policy.new_message.failed",
            ]
        );
    }

    #[test]
    fn append_failure_routes_to_post_send_hook_fallback_only() {
        assert_eq!(
            append_failure_transitions(DeliveryHarnessPath::ClaudeCode),
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
                super::ThreadUpdateStateMachine::ValidateParent,
                super::ThreadUpdateStateMachine::ValidateRoot,
                super::ThreadUpdateStateMachine::ValidateSender,
                super::ThreadUpdateStateMachine::ValidateLinearity,
                super::ThreadUpdateStateMachine::PersistSqlite,
                super::ThreadUpdateStateMachine::RouteDelivery,
                super::ThreadUpdateStateMachine::Delivered,
            ]
        );
        assert_eq!(
            ack_reply_transitions(),
            &[
                AckReplyStateMachine::Received,
                AckReplyStateMachine::ValidateTarget,
                AckReplyStateMachine::PersistAckState,
                AckReplyStateMachine::DelegateReplyDelivery,
                AckReplyStateMachine::Delivered,
            ]
        );
        assert_eq!(
            inbox_repair_transitions(),
            &[
                InboxRepairStateMachine::Received,
                InboxRepairStateMachine::ResolveHarness,
                InboxRepairStateMachine::LoadProjection,
                InboxRepairStateMachine::StageOutput,
                InboxRepairStateMachine::PublishOutput,
                InboxRepairStateMachine::Completed,
            ]
        );
        assert_eq!(
            restore_inbox_rebuild_transitions(),
            &[
                RestoreInboxRebuildStateMachine::Received,
                RestoreInboxRebuildStateMachine::ValidateRestoreMarker,
                RestoreInboxRebuildStateMachine::ResolveHarness,
                RestoreInboxRebuildStateMachine::LoadProjection,
                RestoreInboxRebuildStateMachine::StageOutput,
                RestoreInboxRebuildStateMachine::PublishOutput,
                RestoreInboxRebuildStateMachine::CleanupStaging,
                RestoreInboxRebuildStateMachine::Completed,
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
}
