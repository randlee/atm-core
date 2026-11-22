use crate::boundary::{RosterEntry, RosterHarness};
use crate::delivery_channel::{
    DeliveryChannel, GraftLeaseState, classify_delivery_channel, graft_lease_state,
    local_message_received_backend,
};
use crate::error::AtmError;
use crate::provenance::ValidatedWriteProvenance;
use crate::schema::{AtmMessageId, ThreadMode};
use crate::service_runtime::{GRAFT_RECEIVER_LEASE_LOOKUP_DEADLINE, RetainedServiceRuntime};
use crate::types::{AgentName, PaneId, TeamName};
use crate::{HerdrAgentName, HerdrSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryEventFamily {
    NewMessage,
    ThreadUpdate,
}

impl DeliveryEventFamily {
    pub(crate) fn action_name(self) -> &'static str {
        match self {
            Self::NewMessage => "new_message",
            Self::ThreadUpdate => "thread_update",
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
    pub(crate) herdr_agent: Option<HerdrAgentName>,
    pub(crate) herdr_session: Option<HerdrSession>,
    pub(crate) graft_post_send: bool,
    pub(crate) bare_cli_post_send: bool,
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
            herdr_agent: None,
            herdr_session: None,
            graft_post_send: false,
            bare_cli_post_send: false,
            roster_backed: false,
        }
    }

    /// Historical AQ0-era backend-selection flags kept for existing send/nudge
    /// callers. They are projected from the canonical AQ1 delivery-channel
    /// classifier so immediate and deferred routing cannot diverge.
    fn from_roster(member: RosterEntry, graft_lease: GraftLeaseState) -> Self {
        let local_backend = local_message_received_backend(&member);
        let delivery_channel = classify_delivery_channel(local_backend.as_ref(), graft_lease);
        let local_tmux_post_send = delivery_channel == DeliveryChannel::TmuxSteer;
        let local_herdr_post_send = delivery_channel == DeliveryChannel::HerdrSteer;
        let (herdr_agent, herdr_session) = match local_backend.as_ref() {
            Some(crate::delivery_channel::LocalMessageReceivedBackend::Herdr {
                agent,
                session,
            }) => (agent.clone(), session.clone()),
            _ => (None, None),
        };
        let graft_post_send = delivery_channel == DeliveryChannel::Graft;
        let bare_cli_post_send = delivery_channel == DeliveryChannel::BareCli;
        Self {
            agent: member.agent_name,
            team: member.team_name,
            harness: DeliveryHarnessPath::from_roster_harness(member.harness),
            recipient_pane_id: member.recipient_pane_id,
            local_tmux_post_send,
            local_herdr_post_send,
            herdr_agent,
            herdr_session,
            graft_post_send,
            bare_cli_post_send,
            roster_backed: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewMessageCoordinatorState {
    Received,
    ResolveHarness,
}

impl NewMessageCoordinatorState {
    fn transition_name(self, harness: DeliveryHarnessPath) -> &'static str {
        match (self, harness) {
            (Self::Received, _) => "delivery_policy.new_message.received",
            (Self::ResolveHarness, DeliveryHarnessPath::ClaudeCode) => {
                "delivery_policy.new_message.harness_claude"
            }
            (Self::ResolveHarness, DeliveryHarnessPath::NonClaude) => {
                "delivery_policy.new_message.harness_non_claude"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeHarnessNewMessageState {
    SqliteCommitted,
    AppendCompatibilityMessage,
    RunPrimaryNudge,
    Delivered,
}

impl ClaudeHarnessNewMessageState {
    fn transition_name(self) -> &'static str {
        match self {
            Self::SqliteCommitted => "delivery_policy.new_message.sqlite_committed",
            Self::AppendCompatibilityMessage => {
                "delivery_policy.new_message.compat_append_original"
            }
            Self::RunPrimaryNudge => "delivery_policy.new_message.primary_nudge",
            Self::Delivered => "delivery_policy.new_message.delivered",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NonClaudeHarnessNewMessageState {
    SqliteCommitted,
    DeliverOriginal,
    RunPrimaryNudge,
    Delivered,
}

impl NonClaudeHarnessNewMessageState {
    fn transition_name(self) -> &'static str {
        match self {
            Self::SqliteCommitted => "delivery_policy.new_message.sqlite_committed",
            Self::DeliverOriginal => "delivery_policy.new_message.non_claude_original",
            Self::RunPrimaryNudge => "delivery_policy.new_message.primary_nudge",
            Self::Delivered => "delivery_policy.new_message.delivered",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadUpdateStateMachine {
    Received,
    ValidateParentExists,
    ValidateRootExists,
    ValidateOriginalSender,
    ValidateLinearSuccessor,
    PersistSqlite,
    DispatchByHarness,
    Delivered,
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
        }
    }
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
        let member = runtime
            .load_roster_member(team, agent)
            .ok_or_else(|| AtmError::agent_not_found(agent, team))?;
        let lease =
            runtime.graft_receiver_lease(team, agent, GRAFT_RECEIVER_LEASE_LOOKUP_DEADLINE)?;
        Ok(DeliveryRecipientSnapshot::from_roster(
            member,
            graft_lease_state(lease.as_ref()),
        ))
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

    /// Selects the event family from the write's thread metadata.
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

fn thread_update_transitions() -> &'static [ThreadUpdateStateMachine] {
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
    }
}

#[cfg(test)]
mod tests {
    use super::{DeliveryEventFamily, DeliveryHarnessPath, DeliveryPolicyCoordinator};
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
    fn persisted_new_message_transitions_follow_the_selected_harness() {
        assert_eq!(
            super::persisted_success_transition_names(
                DeliveryEventFamily::NewMessage,
                DeliveryHarnessPath::ClaudeCode,
            ),
            [
                "delivery_policy.new_message.received",
                "delivery_policy.new_message.harness_claude",
                "delivery_policy.new_message.sqlite_committed",
                "delivery_policy.new_message.compat_append_original",
                "delivery_policy.new_message.primary_nudge",
                "delivery_policy.new_message.delivered",
            ]
        );
    }
}
