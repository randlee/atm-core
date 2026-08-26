//! Rebuilds a receiver-hook dispatch from durable message-store state.
//!
//! [`PreparedWrite::build_received_hook_dispatches`](crate::send::PreparedWrite::build_received_hook_dispatches)
//! is the write-time planner: it deliberately never reloads the just-persisted
//! record. This module is the one, explicitly separate, reload path used to
//! replay a durable at-most-once queue claim (`atm queue`, AQ2/AQ3) into the
//! same public [`BuiltInPostSendDispatch`] shape a write-time dispatch would
//! have produced. Living outside `send` keeps that invariant checkable by
//! construction: the write-time planner module never imports the message
//! store reload helper this module wraps.

use crate::boundary::{
    BuiltInPostSendDispatch, MemberKey, MessageKey, NudgeKind, PostSendHookEvent,
};
use crate::delivery_policy::DeliveryPolicyCoordinator;
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::send::NudgeMode;
use crate::send::hook::build_built_in_dispatch;
use crate::service_runtime::LocalServiceRuntime;

/// Rebuilds the receiver-hook dispatch for one already-persisted message.
///
/// `kind` selects the rebuilt dispatch's [`NudgeKind`] (a queue claim always
/// rebuilds `Queue`; a diagnostic replay may request `Steer`). Returns
/// `Ok(None)` when the message does not exist, is not addressed to `member`,
/// or resolves to no first-party delivery capability for the recipient —
/// the same conditions under which the write-time planner omits a dispatch.
///
/// # Errors
///
/// Returns [`AtmError`] if the message store or roster lookups fail, or if
/// the recipient is no longer present in the roster.
pub fn rebuild_received_hook_dispatch(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
    message_id: AtmMessageId,
    kind: NudgeKind,
) -> Result<Option<BuiltInPostSendDispatch>, AtmError> {
    let key = MessageKey::from(message_id);
    let Some(message) = runtime
        .message_store
        .load_message(&key)?
        .filter(|message| &message.team == member.team() && &message.agent == member.agent())
    else {
        return Ok(None);
    };

    let delivery_snapshot = DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
        runtime,
        member.team(),
        member.agent(),
    )?;

    // Mapping mirrors `send::hook::post_send_event_from_message`
    // (write-time), adapted to the persisted `Message` shape returned by a
    // reload instead of the in-memory `LogicalMessage` retained across a
    // single write.
    let event = PostSendHookEvent {
        sender: message.envelope.from.clone(),
        sender_chat_id: message.envelope.source_chat_id.clone(),
        sender_team: message
            .envelope
            .source_team
            .clone()
            .unwrap_or_else(|| member.team().clone()),
        sender_host: crate::schema::authenticated_source_host(&message.envelope)?,
        recipient: member.agent().clone(),
        recipient_team: member.team().clone(),
        message_id,
        description: message
            .envelope
            .summary
            .clone()
            .filter(|summary| !summary.trim().is_empty())
            .unwrap_or_else(|| message.envelope.text.clone()),
        requires_ack: message.envelope.requires_ack,
        is_ack: message.envelope.acknowledges_message_id.is_some(),
        task_id: message.envelope.task_id.clone(),
        recipient_pane_id: delivery_snapshot.recipient_pane_id.clone(),
    };

    let nudge_mode = match kind {
        NudgeKind::Steer => NudgeMode::Immediate,
        NudgeKind::Queue => NudgeMode::Deferred,
    };

    Ok(build_built_in_dispatch(
        runtime,
        &delivery_snapshot,
        &event,
        &message.envelope.text,
        nudge_mode,
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use atm_storage_rusqlite::SqliteStorageBackend;

    use super::*;
    use crate::boundary::{PostSendBuiltInTarget, RosterEntry, RosterHarness, RosterMemberKind};
    use crate::observability::NullObservability;
    use crate::schema::AgentType;
    use crate::send::{NudgeMode as SendNudgeMode, SendMessageSource, write_mail_with_runtime};
    use crate::service_runtime::LocalFileNonClaudeOutbound;
    use crate::types::{AgentName, ModelName, PaneId, TeamName};

    fn roster_member(team: &TeamName, agent: &str, pane_id: Option<PaneId>) -> RosterEntry {
        RosterEntry {
            team_name: team.clone(),
            agent_name: agent.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::PythonGraft,
            agent_type: AgentType::default(),
            model: ModelName::default(),
            recipient_pane_id: pane_id,
            metadata_json: serde_json::Map::new(),
        }
    }

    /// Builds a real SQLite-backed runtime (own on-disk database, not the
    /// `atm-runtime-test-support` composition root, which returns a
    /// `LocalServiceRuntime` from a second `atm_core` crate instance that
    /// `#[cfg(test)]` code in this crate cannot name) with a seeded roster:
    /// `sender` (no local backend) and `recipient` (tmux pane, so a write
    /// triggers a `LocalSteer` dispatch this test can then rebuild).
    fn setup() -> (tempfile::TempDir, LocalServiceRuntime, TeamName) {
        let root = tempfile::tempdir().expect("temp root");
        let database_path = root.path().join("mail.sqlite3");
        let backend = SqliteStorageBackend::new(&database_path).expect("sqlite backend");
        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            backend.message_store(),
            backend.roster_store(),
            backend.nudge_template_override_store(),
            std::sync::Arc::new(LocalFileNonClaudeOutbound::new()),
        )
        .with_pending_nudge_store(backend.pending_nudge_store());
        let team: TeamName = "test-team".parse().expect("team");
        runtime
            .roster_store
            .save_roster(&atm_storage::RosterSnapshot {
                team_name: team.clone(),
                members: vec![
                    roster_member(&team, "sender", None),
                    roster_member(
                        &team,
                        "recipient",
                        Some(PaneId::from_cli("%9").expect("pane")),
                    ),
                ],
                refreshed_at: None,
            })
            .expect("seed roster");
        (root, runtime, team)
    }

    fn write_request(
        home_dir: &std::path::Path,
        team: &TeamName,
        nudge_mode: SendNudgeMode,
    ) -> crate::send::WriteRequest {
        crate::send::WriteRequest::new(
            home_dir.to_path_buf(),
            home_dir.to_path_buf(),
            "sender".parse::<AgentName>().expect("sender"),
            "recipient@test-team",
            team.clone(),
            SendMessageSource::Inline("rebuild me".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("write request")
        .with_nudge_mode(nudge_mode)
    }

    #[test]
    fn rebuild_matches_write_time_dispatch_for_a_pending_tmux_recipient() {
        let (root, runtime, team) = setup();
        let home_dir = root.path().join("home");
        fs::create_dir_all(&home_dir).expect("home dir");

        let request = write_request(&home_dir, &team, SendNudgeMode::Immediate);
        let outcome =
            write_mail_with_runtime(request, &NullObservability, &runtime).expect("write succeeds");
        let message_id = outcome.persisted_message_id();

        let member = MemberKey::new(team, "recipient".parse().expect("agent"));
        let rebuilt =
            rebuild_received_hook_dispatch(&runtime, &member, message_id, NudgeKind::Queue)
                .expect("rebuild succeeds")
                .expect("dispatch rebuilt");

        assert_eq!(rebuilt.kind, NudgeKind::Queue);
        assert!(matches!(
            rebuilt.target,
            PostSendBuiltInTarget::LocalSteer(_)
        ));
        assert_eq!(rebuilt.event.message_id, message_id);
    }

    #[test]
    fn rebuild_returns_none_for_a_message_not_addressed_to_member() {
        let (root, runtime, team) = setup();
        let home_dir = root.path().join("home");
        fs::create_dir_all(&home_dir).expect("home dir");

        let request = write_request(&home_dir, &team, SendNudgeMode::Immediate);
        let outcome =
            write_mail_with_runtime(request, &NullObservability, &runtime).expect("write succeeds");
        let message_id = outcome.persisted_message_id();

        let wrong_member = MemberKey::new(team, "sender".parse().expect("agent"));
        let rebuilt =
            rebuild_received_hook_dispatch(&runtime, &wrong_member, message_id, NudgeKind::Queue)
                .expect("rebuild does not error");
        assert!(rebuilt.is_none());
    }
}
