//! Recipient delivery-channel classification derived from durable roster
//! data.
//!
//! This module is the AQ1 classifier seam consumed by later sprints that
//! decide how a queue-claimed (or immediate) receiver nudge should reach its
//! recipient. It intentionally performs no delivery of its own and requires
//! no schema migration: every input is already-persisted roster data
//! (`recipient_pane_id`, `metadata_json`) and the registry-backed graft lease.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AtmError;
use crate::types::PaneId;

pub(crate) const BACKEND_TYPE_METADATA_KEY: &str = "backendType";

/// One non-empty Herdr session identifier from roster metadata.
///
/// Herdr sessions are roster data, like `pane_id`: the daemon never launches
/// sessions and its own process environment is irrelevant to which session a
/// member's agent lives in. A caller emitting a Herdr nudge sets
/// `HERDR_SESSION` on the emitter's child process from this value; `None`
/// selects Herdr's default server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HerdrSession(String);

impl HerdrSession {
    /// Constructs one Herdr session identifier.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] if `value` is empty (after trimming) or contains
    /// whitespace or control characters.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(AtmError::validation(
                "herdr session identifier must not be blank",
            ));
        }
        if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(AtmError::validation(
                "herdr session identifier must not contain whitespace or control characters",
            ));
        }
        Ok(Self(value))
    }

    /// Returns the session identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HerdrSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The first-party local delivery backend a recipient's roster entry
/// resolves to, if any.
///
/// `session`: the Herdr session the member's agent lives in (sets
/// `HERDR_SESSION` on the emitter's child env per invocation); `None` =
/// Herdr's default server. Roster data, like `pane_id` — the daemon never
/// launches sessions and its own env is irrelevant (Rand, 2026-08-26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalMessageReceivedBackend {
    Tmux { pane_id: PaneId },
    Herdr { session: Option<HerdrSession> },
}

/// Whether the recipient's process currently holds an active Graft receiver
/// lease. A two-variant enum owned by AQ1 (not `Option<&GraftReceiverLease>`)
/// so this crate never needs to name the AQ1.5 lease type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraftLeaseState {
    Absent,
    Active,
}

/// Map the durable registry lookup to the AQ1 classifier state.
///
/// Lease freshness and delivery reachability are advisory diagnostics. A
/// present lease remains an active graft capability for routing, even when it
/// is stale or has recorded an unreachable observation; delivery owns the
/// dial-anyway behavior.
#[must_use]
pub fn graft_lease_state(lookup: Option<&atm_storage::GraftReceiverLease>) -> GraftLeaseState {
    if lookup.is_some() {
        GraftLeaseState::Active
    } else {
        GraftLeaseState::Absent
    }
}

/// The delivery channel one committed dispatch should be routed through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryChannel {
    TmuxSteer,
    HerdrSteer,
    Graft,
    BareCli,
}

/// Classifies which channel a receiver nudge should use.
///
/// A local backend always wins over a Graft lease: an agent with a published
/// tmux pane or Herdr session receives its nudge locally even if it also
/// holds a Graft receiver lease.
#[must_use]
pub fn classify_delivery_channel(
    local_backend: Option<&LocalMessageReceivedBackend>,
    graft_lease: GraftLeaseState,
) -> DeliveryChannel {
    match (local_backend, graft_lease) {
        (Some(LocalMessageReceivedBackend::Tmux { .. }), _) => DeliveryChannel::TmuxSteer,
        (Some(LocalMessageReceivedBackend::Herdr { .. }), _) => DeliveryChannel::HerdrSteer,
        (None, GraftLeaseState::Active) => DeliveryChannel::Graft,
        (None, GraftLeaseState::Absent) => DeliveryChannel::BareCli,
    }
}

/// Derives the local backend from durable roster data. No schema migration:
/// `recipient_pane_id` selects `Tmux`;
/// `metadata_json["backendType"] == "herdr"` selects `Herdr`, reading an
/// optional `metadata_json["herdrSession"]` string. An unparsable
/// `herdrSession` value is treated as absent and logged, not rejected.
#[must_use]
pub fn local_message_received_backend(
    member: &crate::boundary::RosterEntry,
) -> Option<LocalMessageReceivedBackend> {
    if let Some(pane_id) = member.recipient_pane_id.clone() {
        return Some(LocalMessageReceivedBackend::Tmux { pane_id });
    }
    let is_herdr = member
        .metadata_json
        .get(BACKEND_TYPE_METADATA_KEY)
        .and_then(Value::as_str)
        == Some("herdr");
    if !is_herdr {
        return None;
    }
    let session = member
        .metadata_json
        .get("herdrSession")
        .and_then(Value::as_str)
        .and_then(|raw| match HerdrSession::new(raw) {
            Ok(session) => Some(session),
            Err(error) => {
                tracing::warn!(
                    subsystem = "atm_core.delivery_channel",
                    action = "herdr_session_parse",
                    outcome = "failed",
                    agent = %member.agent_name,
                    team = %member.team_name,
                    %error,
                    "ignoring invalid herdrSession roster metadata"
                );
                None
            }
        });
    Some(LocalMessageReceivedBackend::Herdr { session })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::{RosterEntry, RosterHarness, RosterMemberKind};
    use crate::test_support::{TEST_ARCH_CTM, TEST_TEAM};
    use crate::types::{AgentName, ModelName, TeamName};
    use atm_storage::AgentType;

    fn roster_entry() -> RosterEntry {
        RosterEntry {
            team_name: TEST_TEAM.parse::<TeamName>().expect("team"),
            agent_name: TEST_ARCH_CTM.parse::<AgentName>().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::CodexCli,
            agent_type: AgentType::default(),
            model: ModelName::default(),
            recipient_pane_id: None,
            metadata_json: serde_json::Map::new(),
        }
    }

    #[test]
    fn herdr_session_rejects_blank_and_whitespace() {
        assert!(HerdrSession::new("").is_err());
        assert!(HerdrSession::new("   ").is_err());
        assert!(HerdrSession::new("has space").is_err());
        assert!(HerdrSession::new("has\tcontrol").is_err());
    }

    #[test]
    fn herdr_session_accepts_and_displays_a_plain_token() {
        let session = HerdrSession::new("session-1").expect("valid session");
        assert_eq!(session.as_str(), "session-1");
        assert_eq!(session.to_string(), "session-1");
    }

    #[test]
    fn classify_delivery_channel_covers_all_four_rows() {
        let tmux = LocalMessageReceivedBackend::Tmux {
            pane_id: PaneId::from_cli("%1").expect("pane"),
        };
        let herdr = LocalMessageReceivedBackend::Herdr { session: None };
        assert_eq!(
            classify_delivery_channel(Some(&tmux), GraftLeaseState::Absent),
            DeliveryChannel::TmuxSteer
        );
        assert_eq!(
            classify_delivery_channel(Some(&tmux), GraftLeaseState::Active),
            DeliveryChannel::TmuxSteer
        );
        assert_eq!(
            classify_delivery_channel(Some(&herdr), GraftLeaseState::Absent),
            DeliveryChannel::HerdrSteer
        );
        assert_eq!(
            classify_delivery_channel(None, GraftLeaseState::Active),
            DeliveryChannel::Graft
        );
        assert_eq!(
            classify_delivery_channel(None, GraftLeaseState::Absent),
            DeliveryChannel::BareCli
        );
    }

    #[test]
    fn graft_lease_state_maps_presence_without_rederiving_liveness() {
        assert_eq!(graft_lease_state(None), GraftLeaseState::Absent);
        let lease = atm_storage::GraftReceiverLease {
            endpoint: "127.0.0.1:1".parse().expect("endpoint"),
            capability: crate::local_http::LocalCapability::generate().expect("capability"),
            owner_generation: crate::protocol::OwnerGeneration::new("01J00000000000000000000000")
                .expect("owner generation"),
            registered_at: chrono::Utc::now() - chrono::Duration::hours(1),
            last_seen_at: chrono::Utc::now() - chrono::Duration::hours(1),
            unreachable_since: Some(chrono::Utc::now()),
        };
        assert_eq!(graft_lease_state(Some(&lease)), GraftLeaseState::Active);
    }

    #[test]
    fn local_message_received_backend_prefers_tmux_pane() {
        let mut member = roster_entry();
        member.recipient_pane_id = Some(PaneId::from_cli("%2").expect("pane"));
        member.metadata_json.insert(
            BACKEND_TYPE_METADATA_KEY.to_owned(),
            Value::String("herdr".to_owned()),
        );

        let backend = local_message_received_backend(&member).expect("backend");
        assert!(matches!(backend, LocalMessageReceivedBackend::Tmux { .. }));
    }

    #[test]
    fn local_message_received_backend_reads_herdr_session() {
        let mut member = roster_entry();
        member.metadata_json.insert(
            BACKEND_TYPE_METADATA_KEY.to_owned(),
            Value::String("herdr".to_owned()),
        );
        member.metadata_json.insert(
            "herdrSession".to_owned(),
            Value::String("session-7".to_owned()),
        );

        let backend = local_message_received_backend(&member).expect("backend");
        match backend {
            LocalMessageReceivedBackend::Herdr { session } => {
                assert_eq!(session.expect("session").as_str(), "session-7");
            }
            LocalMessageReceivedBackend::Tmux { .. } => panic!("expected herdr backend"),
        }
    }

    #[test]
    fn local_message_received_backend_treats_invalid_herdr_session_as_absent() {
        let mut member = roster_entry();
        member.metadata_json.insert(
            BACKEND_TYPE_METADATA_KEY.to_owned(),
            Value::String("herdr".to_owned()),
        );
        member.metadata_json.insert(
            "herdrSession".to_owned(),
            Value::String("has space".to_owned()),
        );

        let backend = local_message_received_backend(&member).expect("backend");
        match backend {
            LocalMessageReceivedBackend::Herdr { session } => assert!(session.is_none()),
            LocalMessageReceivedBackend::Tmux { .. } => panic!("expected herdr backend"),
        }
    }

    #[test]
    fn local_message_received_backend_is_none_without_pane_or_herdr_flag() {
        let member = roster_entry();
        assert!(local_message_received_backend(&member).is_none());
    }
}
