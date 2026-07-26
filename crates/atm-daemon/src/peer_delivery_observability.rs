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
/// receipt state. Recovery scheduling belongs to AI.28 and is intentionally
/// absent until that sprint owns real scheduler facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerDeliveryEventKind {
    WritePersisted,
    PeerDeliveryConfirmed,
    PeerDeliveryUnconfirmed,
}

impl PeerDeliveryEventKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::WritePersisted => "write_persisted",
            Self::PeerDeliveryConfirmed => "peer_delivery_confirmed",
            Self::PeerDeliveryUnconfirmed => "peer_delivery_unconfirmed",
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
}

#[derive(Debug, Default)]
pub(crate) struct PeerDeliveryProjection {
    statuses: Mutex<BTreeMap<HostName, PeerLinkStatus>>,
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
                    .get(&peer.host)
                    .cloned()
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
        if statuses.len() >= MAX_PEER_LINK_STATUS_ENTRIES && !statuses.contains_key(&event.peer) {
            tracing::warn!(subsystem = "runtime_health", action = "peer_delivery_projection", outcome = "capacity_exceeded", peer = %event.peer, cap = MAX_PEER_LINK_STATUS_ENTRIES, "peer delivery health projection is bounded; retaining event without a new status row");
            return;
        }
        let status = statuses
            .entry(event.peer.clone())
            .or_insert_with(|| PeerLinkStatus::misconfigured(event.peer.clone()));
        apply_event_to_status(status, event);
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
        .with_extra_string_field("peer", event.peer.to_string());
    if let Some(message_id) = event.message_id {
        retained_event = retained_event.with_message_id(message_id);
    }
    if let Some(error_code) = event.error_code {
        retained_event =
            retained_event.with_extra_string_field("error_code", error_code.to_string());
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
            status.next_attempt_at = None;
            status.drain = PeerDrainState::Idle;
        }
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
