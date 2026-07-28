use std::collections::BTreeMap;
use std::sync::Mutex;

use atm_core::doctor::{PeerDrainState, PeerLinkQuality, PeerLinkStatus};
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::RequestId;
use atm_core::schema::AtmMessageId;
use atm_core::types::{HostName, IsoTimestamp};
use atm_storage::PeerConfigStore;

use crate::SubsystemObservability;

/// Retained, safe-to-log facts about a foreground peer-delivery attempt.
/// This deliberately carries no payload, resolved IP address, credential, or
/// receipt state. AI.28 consumes the recovery variants without adding a
/// second projection writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerDeliveryEventKind {
    WritePersisted,
    PeerDeliveryConfirmed,
    PeerDeliveryUnconfirmed,
    PeerRecoveryScheduled,
    PeerRecoveryAttempt,
}

impl PeerDeliveryEventKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::WritePersisted => "write_persisted",
            Self::PeerDeliveryConfirmed => "peer_delivery_confirmed",
            Self::PeerDeliveryUnconfirmed => "peer_delivery_unconfirmed",
            Self::PeerRecoveryScheduled => "peer_recovery_scheduled",
            Self::PeerRecoveryAttempt => "peer_recovery_attempt",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PeerDeliveryEvent {
    pub(crate) kind: PeerDeliveryEventKind,
    pub(crate) request_id: RequestId,
    pub(crate) message_id: Option<AtmMessageId>,
    pub(crate) peer: HostName,
    pub(crate) error_code: Option<AtmErrorCode>,
    pub(crate) candidate_count: Option<u32>,
    pub(crate) next_attempt_at: Option<IsoTimestamp>,
}

#[derive(Debug, Default)]
pub(crate) struct PeerDeliveryProjection {
    // This map is tiny, bounded, and updated/read as a whole snapshot. A
    // Mutex keeps projection and sequence updates atomic; lock hold time is
    // limited to the in-memory BTreeMap work. Writes must serialize the
    // sequence number, so an RwLock adds complexity without useful read width.
    statuses: Mutex<PeerDeliveryProjectionState>,
}

#[derive(Debug, Default)]
struct PeerDeliveryProjectionState {
    statuses: BTreeMap<HostName, ProjectedPeerLinkStatus>,
    next_sequence: u64,
}

#[derive(Debug)]
struct ProjectedPeerLinkStatus {
    status: PeerLinkStatus,
    sequence: u64,
}

impl PeerDeliveryProjection {
    pub(crate) fn record(&self, event: PeerDeliveryEvent, observability: &SubsystemObservability) {
        emit_retained_event(observability, &event);
        self.project(event);
    }

    pub(crate) fn statuses(&self, peer_config_store: &dyn PeerConfigStore) -> Vec<PeerLinkStatus> {
        let configured_peers = match peer_config_store.list_trusted_peers() {
            Ok(peers) => peers,
            Err(error) => {
                tracing::warn!(subsystem = "runtime_health", action = "peer_delivery_projection", outcome = "peer_config_unavailable", %error, "doctor could not load configured peers for delivery-health projection");
                return Vec::new();
            }
        };
        let Ok(statuses) = self.statuses.lock() else {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "peer_delivery_projection",
                outcome = "lock_poisoned",
                "doctor could not read peer delivery-health projection"
            );
            return configured_peers
                .into_iter()
                .map(|peer| PeerLinkStatus::misconfigured(peer.host))
                .collect();
        };
        configured_peers
            .into_iter()
            .map(|peer| {
                statuses
                    .statuses
                    .get(&peer.host)
                    .map(|tracked| tracked.status.clone())
                    .unwrap_or_else(|| PeerLinkStatus::misconfigured(peer.host))
            })
            .collect()
    }

    fn project(&self, event: PeerDeliveryEvent) {
        const MAX_PEER_LINK_STATUS_ENTRIES: usize = 256;
        let Ok(mut statuses) = self.statuses.lock() else {
            tracing::warn!(
                subsystem = "runtime_health",
                action = "peer_delivery_projection",
                outcome = "lock_poisoned",
                "peer delivery health could not be projected after its retained event was emitted"
            );
            return;
        };
        statuses.next_sequence = statuses.next_sequence.wrapping_add(1);
        let sequence = statuses.next_sequence;
        if statuses.statuses.len() >= MAX_PEER_LINK_STATUS_ENTRIES
            && !statuses.statuses.contains_key(&event.peer)
        {
            let oldest_peer = statuses
                .statuses
                .iter()
                .min_by_key(|(_, tracked)| tracked.sequence)
                .map(|(peer, _)| peer.clone());
            if let Some(oldest_peer) = oldest_peer {
                statuses.statuses.remove(&oldest_peer);
                tracing::warn!(subsystem = "runtime_health", action = "peer_delivery_projection", outcome = "capacity_evicted_oldest", evicted_peer = %oldest_peer, peer = %event.peer, cap = MAX_PEER_LINK_STATUS_ENTRIES, "peer delivery health projection evicted its oldest status row");
            }
        }
        let tracked = statuses
            .statuses
            .entry(event.peer.clone())
            .or_insert_with(|| ProjectedPeerLinkStatus {
                status: PeerLinkStatus::misconfigured(event.peer.clone()),
                sequence,
            });
        tracked.sequence = sequence;
        tracked.status.candidate_count = event.candidate_count;
        apply_event_to_status(&mut tracked.status, event);
    }
}

fn emit_retained_event(observability: &SubsystemObservability, event: &PeerDeliveryEvent) {
    let mut retained_event = observability
        .event(
            "peer_delivery",
            event.kind.as_str(),
            "peer delivery outcome recorded",
        )
        .with_extra_string_field("request_id", event.request_id.to_string())
        .with_extra_string_field("peer", event.peer.to_string())
        .with_extra_string_field(
            "candidate_count",
            event
                .candidate_count
                .map_or_else(|| "unknown".to_string(), |count| count.to_string()),
        );
    if let Some(message_id) = event.message_id {
        retained_event = retained_event.with_message_id(message_id);
    }
    if let Some(error_code) = event.error_code {
        retained_event =
            retained_event.with_extra_string_field("error_code", error_code.to_string());
    }
    if let Some(next_attempt_at) = event.next_attempt_at {
        retained_event =
            retained_event.with_extra_string_field("next_attempt_at", next_attempt_at.to_string());
    }
    observability.emit_event_or_warn(retained_event);
}

fn apply_event_to_status(status: &mut PeerLinkStatus, event: PeerDeliveryEvent) {
    match event.kind {
        PeerDeliveryEventKind::WritePersisted => {}
        PeerDeliveryEventKind::PeerDeliveryConfirmed => {
            status.quality = PeerLinkQuality::Healthy;
            status.last_success_at = Some(IsoTimestamp::now());
            status.last_error_code = None;
            status.next_attempt_at = None;
            status.drain = PeerDrainState::Idle;
        }
        PeerDeliveryEventKind::PeerDeliveryUnconfirmed => {
            status.quality = peer_link_quality_for_error(event.error_code);
            status.last_failure_at = Some(IsoTimestamp::now());
            status.last_error_code = event.error_code;
            status.next_attempt_at = event.next_attempt_at;
            status.drain = PeerDrainState::Idle;
        }
        PeerDeliveryEventKind::PeerRecoveryScheduled => {
            status.quality = PeerLinkQuality::Degraded;
            status.next_attempt_at = event.next_attempt_at;
            status.drain = PeerDrainState::Connecting;
        }
        PeerDeliveryEventKind::PeerRecoveryAttempt => status.drain = PeerDrainState::Draining,
    }
}

fn peer_link_quality_for_error(error_code: Option<AtmErrorCode>) -> PeerLinkQuality {
    match error_code {
        Some(AtmErrorCode::RemoteDeliveryUnconfirmed | AtmErrorCode::DaemonUnavailable) => {
            PeerLinkQuality::Unreachable
        }
        Some(AtmErrorCode::PeerConfigValidationFailed) => PeerLinkQuality::Misconfigured,
        Some(_) | None => PeerLinkQuality::Degraded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::protocol::next_request_id;

    #[test]
    fn projection_evicts_the_oldest_status_when_at_capacity() {
        let projection = PeerDeliveryProjection::default();
        for index in 0..=256 {
            projection.project(PeerDeliveryEvent {
                kind: PeerDeliveryEventKind::PeerDeliveryConfirmed,
                request_id: next_request_id(),
                message_id: None,
                peer: format!("peer-{index}.example.test")
                    .parse()
                    .expect("valid peer host"),
                error_code: None,
                candidate_count: None,
                next_attempt_at: None,
            });
        }

        let statuses = projection.statuses.lock().expect("projection lock");
        assert_eq!(statuses.statuses.len(), 256);
        assert!(
            !statuses.statuses.contains_key(
                &"peer-0.example.test"
                    .parse::<HostName>()
                    .expect("valid peer host")
            ),
            "the oldest status must be evicted rather than silently dropping the new peer"
        );
        assert!(
            statuses.statuses.contains_key(
                &"peer-256.example.test"
                    .parse::<HostName>()
                    .expect("valid peer host")
            )
        );
    }
}
