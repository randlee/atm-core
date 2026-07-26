use std::collections::BTreeSet;
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::error::AtmError;
use atm_core::protocol::{PeerSyncDisposition, PeerSyncOutcome, PeerSyncRequest};
use atm_storage::{PeerSyncPolicy, TrustedPeer};

use crate::https_transport::HttpsMessageTransport;

use super::{DaemonRequestDispatcher, PeerSyncProgress};

impl DaemonRequestDispatcher {
    fn reconcile_peer(
        &self,
        peer_host: &atm_core::types::HostName,
        peer: &TrustedPeer,
        transport: &dyn HttpsMessageTransport,
        policy: PeerSyncPolicy,
        deadline: RequestDeadline,
        delivered_request_json: &mut BTreeSet<String>,
    ) -> Result<u16, AtmError> {
        let not_before = atm_core::types::IsoTimestamp::from_datetime(
            chrono::Utc::now()
                - chrono::Duration::from_std(policy.max_message_age).map_err(|source| {
                    AtmError::validation(format!(
                        "peer sync maximum message age is out of range: {source}"
                    ))
                })?,
        );
        let writes = self.outbound_message_query.recent_outbound_for_peer(
            peer_host,
            not_before,
            policy.max_batch_messages,
        )?;
        let mut delivered = 0_u16;
        for stored in writes {
            if delivered_request_json.contains(&stored.request_json) {
                continue;
            }
            if deadline.remaining().is_none() {
                return Err(AtmError::remote_delivery_unconfirmed(
                    "peer reconciliation exceeded its bounded request deadline",
                ));
            }
            let request = serde_json::from_str(&stored.request_json).map_err(|source| {
                AtmError::mailbox_read(format!(
                    "stored immutable peer outbound write is invalid: {source}"
                ))
            })?;
            transport.deliver(request, peer, deadline)?;
            delivered_request_json.insert(stored.request_json);
            delivered = delivered.checked_add(1).ok_or_else(|| {
                AtmError::validation("peer sync selection exceeded its configured batch limit")
            })?;
        }
        Ok(delivered)
    }

    pub(super) fn sync_peer(
        &self,
        request: PeerSyncRequest,
        deadline: RequestDeadline,
    ) -> Result<PeerSyncOutcome, AtmError> {
        let peer = self
            .peer_config_store
            .trusted_peer(&request.peer)?
            .ok_or_else(|| AtmError::peer_config_validation("unknown trusted peer"))?;
        let policy = self.peer_config_store.peer_sync_policy(&request.peer)?;
        policy.validate()?;
        if policy.max_message_age.is_zero() {
            return Ok(PeerSyncOutcome {
                peer: request.peer,
                delivered: 0,
                disposition: PeerSyncDisposition::Disabled,
            });
        }
        let transport = self
            .https_transport
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer transport slot lock poisoned"))?
            .clone()
            .ok_or_else(|| {
                AtmError::daemon_unavailable("HTTPS peer transport is not enabled in this daemon")
            })?;
        let progress = self
            .peer_sync_progress
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer sync progress map lock poisoned"))?
            .entry(request.peer.clone())
            .or_insert_with(|| {
                std::sync::Arc::new(std::sync::Mutex::new(PeerSyncProgress::default()))
            })
            .clone();
        let mut state = progress
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer sync progress lock poisoned"))?;
        let now = std::time::Instant::now();
        if state.in_flight || state.next_allowed_at.is_some_and(|next| now < next) {
            return Ok(PeerSyncOutcome {
                peer: request.peer,
                delivered: 0,
                disposition: PeerSyncDisposition::RateLimited,
            });
        }
        state.next_allowed_at = Some(now + Duration::from_secs(60));
        state.in_flight = true;
        drop(state);
        let mut delivered_request_json = BTreeSet::new();
        let result = self.reconcile_peer(
            &request.peer,
            &peer,
            transport.as_ref(),
            policy,
            deadline,
            &mut delivered_request_json,
        );
        progress
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer sync progress lock poisoned"))?
            .in_flight = false;
        let delivered = result?;
        Ok(PeerSyncOutcome {
            peer: request.peer,
            delivered,
            disposition: PeerSyncDisposition::Completed,
        })
    }
}
