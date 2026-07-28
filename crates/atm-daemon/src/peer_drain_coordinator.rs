//! Bounded, in-memory recovery of immutable peer-directed writes.
//!
//! This owns scheduling only. Records remain in canonical storage and the
//! transport remains a transport-only capability; no outbox, receipt, cursor,
//! payload, or per-message state is retained here.
//! The post-write router classifies a caller's immediate result; this module
//! alone owns bounded retry eligibility and backoff for unconfirmed delivery.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use atm_core::RequestDeadline;
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{ResponseEnvelope, next_request_id};
use atm_core::send::WriteRequest;
use atm_core::types::{HostName, IsoTimestamp};
use atm_storage::{OutboundMessageQuery, PeerConfigStore, TrustedPeer};

use crate::https_transport::{HttpsMessageTransport, SharedHttpsTransport};
use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};
use crate::runtime_health::peer_authority::resolve_peer_authority;

const INITIAL_BACKOFF: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);
const MAX_PEER_DRAIN_SLOTS: usize = 256;
pub(crate) const PEER_SYNC_REQUEST_DEADLINE: Duration = Duration::from_secs(5);

/// The only non-durable per-peer recovery state. In particular this never
/// holds a message, cursor, payload, receipt, or attempted-delivery history.
#[derive(Debug)]
struct PeerDrainSlot {
    running: bool,
    requested_generation: u64,
    observed_generation: u64,
    next_attempt_at: Option<Instant>,
    backoff: Duration,
}

impl Default for PeerDrainSlot {
    fn default() -> Self {
        Self {
            running: false,
            requested_generation: 0,
            observed_generation: 0,
            next_attempt_at: None,
            backoff: INITIAL_BACKOFF,
        }
    }
}

fn schedule_retry(slot: &mut PeerDrainSlot, now: Instant) -> Duration {
    let delay = slot.backoff;
    slot.next_attempt_at = Some(now + delay);
    slot.backoff = slot.backoff.saturating_mul(2).min(MAX_BACKOFF);
    delay
}

fn retry_timestamp(delay: Duration) -> IsoTimestamp {
    IsoTimestamp::from_datetime(
        chrono::Utc::now() + chrono::Duration::from_std(delay).expect("bounded backoff"),
    )
}

pub(crate) fn is_retryable_peer_error(error: &AtmError) -> bool {
    matches!(
        error.code(),
        AtmErrorCode::DaemonUnavailable | AtmErrorCode::RemoteDeliveryUnconfirmed
    )
}

fn remote_delivery_error_with_cause(error: AtmError) -> AtmError {
    let detail = error.detail().to_owned();
    let source = error
        .cause()
        .map(|cause| format!("{}; source cause: {cause}", error.message()))
        .unwrap_or_else(|| error.message().to_owned());
    AtmError::remote_delivery_unconfirmed(detail).with_cause(source)
}

pub(crate) trait PeerDeliveryCoordinator: Send + Sync {
    /// Records a best-effort, identifier-only wake-up after a durable origin
    /// write has committed.  It must never perform storage or transport work,
    /// and cannot turn a successful local admission into a failure.
    fn signal_after_persist(&self, peer: HostName);

    fn sync_peer(&self, peer: &HostName, deadline: RequestDeadline) -> Result<u16, AtmError>;

    fn start(&self) -> Result<(), AtmError>;

    fn stop(&self) -> Result<(), AtmError>;
}

pub(crate) struct PeerDrainCoordinator {
    peers: Arc<dyn PeerConfigStore + Send + Sync>,
    outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
    transport: SharedHttpsTransport,
    slots: Arc<(Mutex<BTreeMap<HostName, PeerDrainSlot>>, Condvar)>,
    record: Arc<dyn Fn(PeerDeliveryEvent) + Send + Sync>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl std::fmt::Debug for PeerDrainCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerDrainCoordinator")
            .field("peers", &"dyn PeerConfigStore")
            .field("outbound", &"dyn OutboundMessageQuery")
            .finish_non_exhaustive()
    }
}

impl PeerDrainCoordinator {
    pub(crate) fn new(
        peers: Arc<dyn PeerConfigStore + Send + Sync>,
        outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
        transport: SharedHttpsTransport,
        record: Arc<dyn Fn(PeerDeliveryEvent) + Send + Sync>,
    ) -> Self {
        Self {
            peers,
            outbound,
            transport,
            slots: Arc::new((Mutex::new(BTreeMap::new()), Condvar::new())),
            record,
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }

    fn slots(&self) -> Result<MutexGuard<'_, BTreeMap<HostName, PeerDrainSlot>>, AtmError> {
        self.slots
            .0
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer drain coordinator slot lock poisoned"))
    }

    /// Makes room only by discarding a fully idle slot. Running drains and
    /// scheduled retries remain live recovery state and are never evicted.
    fn reserve_slot(
        slots: &mut BTreeMap<HostName, PeerDrainSlot>,
        host: &HostName,
    ) -> Result<(), AtmError> {
        if slots.contains_key(host) || slots.len() < MAX_PEER_DRAIN_SLOTS {
            return Ok(());
        }
        let idle_host = slots
            .iter()
            .find(|(_, slot)| !slot.running && slot.next_attempt_at.is_none())
            .map(|(host, _)| host.clone());
        if let Some(idle_host) = idle_host {
            slots.remove(&idle_host);
            return Ok(());
        }
        Err(AtmError::daemon_unavailable(
            "peer drain coordinator capacity exhausted",
        ))
    }

    fn acquire(&self, host: &HostName, deadline: RequestDeadline) -> Result<(), AtmError> {
        let mut slots = self.slots()?;
        Self::reserve_slot(&mut slots, host)?;
        let slot = slots.entry(host.clone()).or_default();
        slot.requested_generation = slot.requested_generation.saturating_add(1);
        while slots.get(host).is_some_and(|slot| slot.running) {
            let Some(remaining) = deadline
                .remaining()
                .filter(|remaining| !remaining.is_zero())
            else {
                return Err(AtmError::remote_delivery_unconfirmed(
                    "peer delivery remained behind the active drain until the request deadline",
                ));
            };
            let (guard, timeout) = self.slots.1.wait_timeout(slots, remaining).map_err(|_| {
                AtmError::daemon_unavailable("peer drain coordinator slot lock poisoned")
            })?;
            slots = guard;
            if timeout.timed_out() && slots.get(host).is_some_and(|slot| slot.running) {
                return Err(AtmError::remote_delivery_unconfirmed(
                    "peer delivery remained behind the active drain until the request deadline",
                ));
            }
        }
        slots.entry(host.clone()).or_default().running = true;
        Ok(())
    }

    fn release(&self, host: &HostName) -> Result<(), AtmError> {
        if let Some(slot) = self.slots()?.get_mut(host) {
            slot.running = false;
            slot.observed_generation = slot.requested_generation;
        }
        self.slots.1.notify_all();
        Ok(())
    }

    fn mark_generation_observed(&self, host: &HostName) -> Result<(), AtmError> {
        if let Some(slot) = self.slots()?.get_mut(host) {
            slot.observed_generation = slot.requested_generation;
        }
        Ok(())
    }

    fn generation_changed(&self, host: &HostName) -> Result<bool, AtmError> {
        Ok(self
            .slots()?
            .get(host)
            .is_some_and(|slot| slot.requested_generation != slot.observed_generation))
    }

    fn reset_backoff(&self, host: &HostName) -> Result<(), AtmError> {
        if let Some(slot) = self.slots()?.get_mut(host) {
            slot.backoff = INITIAL_BACKOFF;
            slot.next_attempt_at = None;
        }
        Ok(())
    }

    fn record(
        &self,
        kind: PeerDeliveryEventKind,
        peer: HostName,
        error: Option<&AtmError>,
        candidates: Option<u32>,
        next_attempt_at: Option<IsoTimestamp>,
    ) {
        (self.record)(PeerDeliveryEvent {
            kind,
            request_id: next_request_id(),
            message_id: None,
            peer,
            error_code: error.map(AtmError::code),
            candidate_count: candidates,
            next_attempt_at,
        });
    }

    fn configured_peer(&self, host: &HostName) -> Result<TrustedPeer, AtmError> {
        resolve_peer_authority(host, &self.peers.list_trusted_peers()?)
    }

    fn recovery_context(
        &self,
        host: &HostName,
    ) -> Result<
        (
            TrustedPeer,
            atm_storage::PeerSyncPolicy,
            Arc<dyn HttpsMessageTransport>,
            IsoTimestamp,
        ),
        AtmError,
    > {
        let peer = self.configured_peer(host)?;
        let policy = self.peers.peer_sync_policy(host)?.validate()?;
        let transport = self
            .transport
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer transport slot lock poisoned"))?
            .clone()
            .ok_or_else(|| {
                AtmError::remote_delivery_unconfirmed(
                    "HTTPS peer transport is not enabled in this daemon",
                )
            })?;
        let not_before = IsoTimestamp::from_datetime(
            chrono::Utc::now()
                - chrono::Duration::from_std(policy.max_message_age).map_err(|_| {
                    AtmError::validation("peer sync maximum message age is out of range")
                })?,
        );
        Ok((peer, policy, transport, not_before))
    }

    fn drain(&self, host: &HostName, deadline: RequestDeadline) -> Result<u16, AtmError> {
        let (peer, policy, transport, not_before) = self.recovery_context(host)?;
        self.record(
            PeerDeliveryEventKind::PeerRecoveryAttempt,
            host.clone(),
            None,
            None,
            None,
        );
        let mut after = None;
        let mut delivered = 0_u16;
        self.mark_generation_observed(host)?;
        loop {
            if deadline.expired() {
                return self.failed(
                    host,
                    AtmError::remote_delivery_unconfirmed(
                        "peer reconciliation exceeded its bounded request deadline",
                    ),
                );
            }
            let page =
                self.page_for_peer(host, not_before, after, policy.max_batch_messages, deadline)?;
            if deadline.expired() {
                return self.failed(
                    host,
                    AtmError::remote_delivery_unconfirmed(
                        "peer reconciliation exceeded its bounded request deadline",
                    ),
                );
            }
            if page.is_empty() {
                if self.generation_changed(host)? {
                    self.mark_generation_observed(host)?;
                    after = None;
                    continue;
                }
                return self.finish_drain(host, delivered);
            }
            let requests = Self::decode_page_requests(&page)?;
            let responses = match transport.deliver_page(&requests, &peer, deadline) {
                Ok(responses) => responses,
                Err(error) => {
                    return self.failed(host, remote_delivery_error_with_cause(error));
                }
            };
            if responses.len() != page.len() {
                return self.failed(
                    host,
                    AtmError::remote_delivery_unconfirmed(
                        "HTTPS peer transport returned fewer responses than canonical writes",
                    ),
                );
            }
            for (stored, response) in page.iter().zip(responses) {
                if let ResponseEnvelope::Error(error) = response {
                    return if is_retryable_peer_error(&error) {
                        self.failed(host, error)
                    } else {
                        Err(error)
                    };
                }
                delivered = delivered.saturating_add(1);
                after = Some((stored.created_at, stored.message_id));
            }
            if page.len() < usize::from(policy.max_batch_messages.get()) {
                return self.finish_drain(host, delivered);
            }
        }
    }

    fn page_for_peer(
        &self,
        host: &HostName,
        not_before: IsoTimestamp,
        after: Option<(IsoTimestamp, atm_core::schema::AtmMessageId)>,
        limit: std::num::NonZeroU16,
        deadline: RequestDeadline,
    ) -> Result<Vec<atm_storage::StoredPeerWrite>, AtmError> {
        let budget = deadline.remaining().ok_or_else(|| {
            AtmError::remote_delivery_unconfirmed(
                "peer reconciliation exceeded its bounded request deadline",
            )
        })?;
        self.outbound
            .page_for_peer(host, not_before, after, limit, budget)
    }

    fn decode_page_requests(
        page: &[atm_storage::StoredPeerWrite],
    ) -> Result<Vec<WriteRequest>, AtmError> {
        page.iter()
            .map(|stored| {
                serde_json::from_str(&stored.request_json).map_err(|source| {
                    AtmError::mailbox_read("stored immutable peer outbound write is invalid")
                        .with_cause(source)
                })
            })
            .collect()
    }

    fn finish_drain(&self, host: &HostName, delivered: u16) -> Result<u16, AtmError> {
        self.record(
            // A worker scan is the only component that can establish a peer
            // HTTP acceptance outcome.  Keep that fact distinct from the
            // earlier `write_persisted` admission event.
            PeerDeliveryEventKind::PeerDeliveryConfirmed,
            host.clone(),
            None,
            Some(u32::from(delivered)),
            None,
        );
        self.reset_backoff(host)?;
        Ok(delivered)
    }

    fn failed<T>(&self, host: &HostName, error: AtmError) -> Result<T, AtmError> {
        let next_attempt_at = {
            let mut slots = self.slots()?;
            let slot = slots.entry(host.clone()).or_default();
            let delay = schedule_retry(slot, Instant::now());
            retry_timestamp(delay)
        };
        self.record(
            // The immutable local admission already succeeded. This event is
            // exclusively the later, uncertain peer-delivery outcome.
            PeerDeliveryEventKind::PeerDeliveryUnconfirmed,
            host.clone(),
            Some(&error),
            None,
            Some(next_attempt_at),
        );
        self.record(
            PeerDeliveryEventKind::PeerRecoveryScheduled,
            host.clone(),
            Some(&error),
            None,
            Some(next_attempt_at),
        );
        Err(error)
    }

    fn worker_coordinator(&self) -> Self {
        Self {
            peers: Arc::clone(&self.peers),
            outbound: Arc::clone(&self.outbound),
            transport: Arc::clone(&self.transport),
            slots: Arc::clone(&self.slots),
            record: Arc::clone(&self.record),
            stop: Arc::clone(&self.stop),
            worker: Mutex::new(None),
        }
    }

    fn schedule_startup_recovery(&self) -> Result<(), AtmError> {
        for peer in self
            .peers
            .list_trusted_peers()?
            .into_iter()
            .filter(|peer| peer.enabled)
        {
            let policy = self.peers.peer_sync_policy(&peer.host)?.validate()?;
            if policy.max_message_age.is_zero() {
                continue;
            }
            let not_before = IsoTimestamp::from_datetime(
                chrono::Utc::now()
                    - chrono::Duration::from_std(policy.max_message_age).map_err(|_| {
                        AtmError::validation("peer sync maximum message age is out of range")
                    })?,
            );
            if self
                .outbound
                .page_for_peer(
                    &peer.host,
                    not_before,
                    None,
                    policy.max_batch_messages,
                    PEER_SYNC_REQUEST_DEADLINE,
                )?
                .is_empty()
            {
                continue;
            }
            let next_attempt_at = retry_timestamp(INITIAL_BACKOFF);
            let mut slots = self.slots()?;
            Self::reserve_slot(&mut slots, &peer.host)?;
            slots.entry(peer.host.clone()).or_default().next_attempt_at =
                Some(Instant::now() + INITIAL_BACKOFF);
            drop(slots);
            self.record(
                PeerDeliveryEventKind::PeerRecoveryScheduled,
                peer.host,
                None,
                None,
                Some(next_attempt_at),
            );
        }
        Ok(())
    }

    fn run_scheduled_recovery(&self) {
        while !self.stop.load(Ordering::SeqCst) {
            let mut slots = match self.slots() {
                Ok(slots) => slots,
                Err(error) => {
                    tracing::error!(subsystem = "peer_drain", action = "schedule", %error, "peer recovery worker stopped after slot lock failure");
                    return;
                }
            };
            let due = {
                let now = Instant::now();
                slots.iter_mut().find_map(|(host, slot)| {
                    (slot.next_attempt_at.is_some_and(|next| next <= now) && !slot.running).then(
                        || {
                            slot.running = true;
                            host.clone()
                        },
                    )
                })
            };
            if let Some(host) = due {
                drop(slots);
                let _ = self.drain(&host, RequestDeadline::after(PEER_SYNC_REQUEST_DEADLINE));
                if let Err(error) = self.release(&host) {
                    tracing::error!(subsystem = "peer_drain", action = "release", %error, "peer drain slot release failed");
                }
                continue;
            }
            // A post-commit signal wakes this worker immediately; the bounded
            // timeout still makes scheduled retry deadlines and shutdown
            // progress without retaining any message-level work state.
            let _ = self.slots.1.wait_timeout(slots, Duration::from_millis(250));
        }
    }
}

impl PeerDeliveryCoordinator for PeerDrainCoordinator {
    fn signal_after_persist(&self, host: HostName) {
        let Ok(mut slots) = self.slots() else {
            tracing::error!(subsystem = "peer_drain", action = "signal_after_persist", peer = %host, "peer drain slot lock poisoned; durable write remains available for recovery");
            return;
        };
        if let Err(error) = Self::reserve_slot(&mut slots, &host) {
            tracing::warn!(subsystem = "peer_drain", action = "signal_after_persist", peer = %host, %error, "peer drain signal coalesced under bounded scheduler pressure; durable write remains available for recovery");
            return;
        }
        let slot = slots.entry(host).or_default();
        slot.requested_generation = slot.requested_generation.saturating_add(1);
        slot.next_attempt_at = Some(Instant::now());
        self.slots.1.notify_all();
    }

    fn sync_peer(&self, peer: &HostName, deadline: RequestDeadline) -> Result<u16, AtmError> {
        if self
            .peers
            .peer_sync_policy(peer)?
            .validate()?
            .max_message_age
            .is_zero()
        {
            return Ok(0);
        }
        self.acquire(peer, deadline)?;
        let result = self.drain(peer, deadline);
        self.release(peer)?;
        result
    }

    fn start(&self) -> Result<(), AtmError> {
        self.schedule_startup_recovery()?;
        let mut worker = self.worker.lock().map_err(|_| {
            AtmError::daemon_unavailable("peer drain coordinator worker lock poisoned")
        })?;
        if worker.is_none() {
            self.stop.store(false, Ordering::SeqCst);
            let coordinator = self.worker_coordinator();
            *worker = Some(
                std::thread::Builder::new()
                    .name("atm-peer-drain".to_string())
                    .spawn(move || coordinator.run_scheduled_recovery())
                    .map_err(|_| {
                        AtmError::daemon_unavailable("failed to start peer drain worker")
                    })?,
            );
        }
        Ok(())
    }

    fn stop(&self) -> Result<(), AtmError> {
        self.stop.store(true, Ordering::SeqCst);
        let worker = self
            .worker
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("peer drain coordinator worker lock poisoned")
            })?
            .take();
        if let Some(worker) = worker {
            worker.join().map_err(|_| {
                AtmError::daemon_unavailable("peer drain worker panicked during shutdown")
            })?;
        }
        self.slots()?.clear();
        self.slots.1.notify_all();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        INITIAL_BACKOFF, MAX_BACKOFF, PeerDeliveryCoordinator, PeerDrainCoordinator, PeerDrainSlot,
        is_retryable_peer_error, retry_timestamp, schedule_retry,
    };
    use atm_core::RequestDeadline;
    use atm_core::error::AtmError;
    use atm_core::protocol::ResponseEnvelope;
    use atm_core::send::WriteRequest;
    use atm_core::types::{HostName, IsoTimestamp};
    use atm_storage::{
        HttpsInterface, LocalCertificate, OutboundMessageQuery, PeerConfigStore, PeerSyncPolicy,
        StoredPeerWrite, TrustedPeer,
    };
    use chrono::Utc;
    use std::num::NonZeroU16;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    use crate::https_transport::HttpsMessageTransport;

    struct EmptyPeerStore;

    impl atm_storage::contract::sealed::Sealed for EmptyPeerStore {}

    impl PeerConfigStore for EmptyPeerStore {
        fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, AtmError> {
            Ok(Vec::new())
        }
        fn save_interface(&self, _: &HttpsInterface) -> Result<(), AtmError> {
            Ok(())
        }
        fn remove_interface(&self, _: std::net::SocketAddr) -> Result<bool, AtmError> {
            Ok(false)
        }
        fn local_certificate(&self) -> Result<Option<LocalCertificate>, AtmError> {
            Ok(None)
        }
        fn save_local_certificate(&self, _: &LocalCertificate) -> Result<(), AtmError> {
            Ok(())
        }
        fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, AtmError> {
            Ok(Vec::new())
        }
        fn trusted_peer(&self, _: &HostName) -> Result<Option<TrustedPeer>, AtmError> {
            Ok(None)
        }
        fn save_trusted_peer(&self, _: &TrustedPeer) -> Result<(), AtmError> {
            Ok(())
        }
        fn remove_trusted_peer(&self, _: &HostName) -> Result<bool, AtmError> {
            Ok(false)
        }
        fn peer_sync_policy(&self, _: &HostName) -> Result<PeerSyncPolicy, AtmError> {
            Ok(PeerSyncPolicy::default())
        }
    }

    struct EmptyOutbound;

    impl atm_storage::contract::sealed::Sealed for EmptyOutbound {}

    impl OutboundMessageQuery for EmptyOutbound {
        fn page_for_peer(
            &self,
            _: &HostName,
            _: IsoTimestamp,
            _: Option<(IsoTimestamp, atm_core::schema::AtmMessageId)>,
            _: std::num::NonZeroU16,
            _: Duration,
        ) -> Result<Vec<StoredPeerWrite>, AtmError> {
            Ok(Vec::new())
        }
    }

    struct ConfiguredPeerStore {
        peer: TrustedPeer,
        policy: PeerSyncPolicy,
    }

    impl atm_storage::contract::sealed::Sealed for ConfiguredPeerStore {}

    impl PeerConfigStore for ConfiguredPeerStore {
        fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, AtmError> {
            Ok(Vec::new())
        }
        fn save_interface(&self, _: &HttpsInterface) -> Result<(), AtmError> {
            Ok(())
        }
        fn remove_interface(&self, _: std::net::SocketAddr) -> Result<bool, AtmError> {
            Ok(false)
        }
        fn local_certificate(&self) -> Result<Option<LocalCertificate>, AtmError> {
            Ok(None)
        }
        fn save_local_certificate(&self, _: &LocalCertificate) -> Result<(), AtmError> {
            Ok(())
        }
        fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, AtmError> {
            Ok(vec![self.peer.clone()])
        }
        fn trusted_peer(&self, host: &HostName) -> Result<Option<TrustedPeer>, AtmError> {
            Ok((host == &self.peer.host).then(|| self.peer.clone()))
        }
        fn save_trusted_peer(&self, _: &TrustedPeer) -> Result<(), AtmError> {
            Ok(())
        }
        fn remove_trusted_peer(&self, _: &HostName) -> Result<bool, AtmError> {
            Ok(false)
        }
        fn peer_sync_policy(&self, _: &HostName) -> Result<PeerSyncPolicy, AtmError> {
            Ok(self.policy)
        }
    }

    #[derive(Default)]
    struct CapturingOutbound {
        not_before: Mutex<Vec<IsoTimestamp>>,
        limits: Mutex<Vec<NonZeroU16>>,
    }

    impl atm_storage::contract::sealed::Sealed for CapturingOutbound {}

    impl OutboundMessageQuery for CapturingOutbound {
        fn page_for_peer(
            &self,
            _: &HostName,
            not_before: IsoTimestamp,
            _: Option<(IsoTimestamp, atm_core::schema::AtmMessageId)>,
            limit: NonZeroU16,
            _: Duration,
        ) -> Result<Vec<StoredPeerWrite>, AtmError> {
            self.not_before
                .lock()
                .expect("captured cutoff lock")
                .push(not_before);
            self.limits.lock().expect("captured limit lock").push(limit);
            Ok(Vec::new())
        }
    }

    struct NeverCalledTransport;

    impl HttpsMessageTransport for NeverCalledTransport {
        fn deliver(
            &self,
            _: WriteRequest,
            _: &TrustedPeer,
            _: RequestDeadline,
        ) -> Result<ResponseEnvelope, AtmError> {
            panic!("an empty recovery page must not call the peer transport")
        }
    }

    fn coordinator_for_slots() -> Arc<PeerDrainCoordinator> {
        Arc::new(PeerDrainCoordinator::new(
            Arc::new(EmptyPeerStore),
            Arc::new(EmptyOutbound),
            Arc::new(std::sync::Mutex::new(None)),
            Arc::new(|_| {}),
        ))
    }

    #[test]
    fn retry_backoff_starts_at_one_minute_caps_and_resets() {
        let mut slot = PeerDrainSlot::default();
        let now = Instant::now();
        assert_eq!(schedule_retry(&mut slot, now), INITIAL_BACKOFF);
        assert_eq!(slot.next_attempt_at, Some(now + INITIAL_BACKOFF));
        for _ in 0..8 {
            schedule_retry(&mut slot, now);
        }
        assert_eq!(slot.backoff, MAX_BACKOFF);
        slot.backoff = INITIAL_BACKOFF;
        slot.next_attempt_at = None;
        assert_eq!(slot.backoff, Duration::from_secs(60));
        assert!(slot.next_attempt_at.is_none());
    }

    #[test]
    fn generation_change_requires_another_final_scan() {
        let mut slot = PeerDrainSlot {
            requested_generation: 2,
            observed_generation: 1,
            ..PeerDrainSlot::default()
        };
        assert_ne!(slot.requested_generation, slot.observed_generation);
        slot.observed_generation = slot.requested_generation;
        assert_eq!(slot.requested_generation, slot.observed_generation);
    }

    #[test]
    fn post_persist_signal_only_coalesces_an_immediate_memory_wakeup() {
        let coordinator = coordinator_for_slots();
        let host: HostName = "peer.example.test".parse().expect("host");

        coordinator.signal_after_persist(host.clone());
        coordinator.signal_after_persist(host.clone());

        let slots = coordinator.slots().expect("slot lock");
        let slot = slots.get(&host).expect("scheduled host");
        assert_eq!(slot.requested_generation, 2);
        assert!(slot.next_attempt_at.is_some());
        assert!(!slot.running, "admission must not run peer work inline");
    }

    #[test]
    fn only_unconfirmed_delivery_errors_schedule_recovery() {
        assert!(is_retryable_peer_error(
            &AtmError::remote_delivery_unconfirmed("offline")
        ));
        assert!(!is_retryable_peer_error(&AtmError::peer_config_validation(
            "bad peer"
        )));
    }

    #[test]
    fn malformed_stored_request_preserves_the_json_parser_cause() {
        let error = PeerDrainCoordinator::decode_page_requests(&[StoredPeerWrite {
            created_at: IsoTimestamp::now(),
            message_id: atm_core::schema::AtmMessageId::new(),
            request_json: "{malformed".to_string(),
        }])
        .expect_err("malformed retained JSON must be rejected");
        assert_eq!(
            error.code(),
            atm_core::error_codes::AtmErrorCode::MailboxReadFailed
        );
        assert!(
            error.cause().is_some_and(|cause| !cause.is_empty()),
            "the parser diagnostic must remain available as the structured cause"
        );
    }

    #[test]
    fn scheduled_timestamp_reports_the_real_backoff_not_the_current_instant() {
        let before = Utc::now();
        let scheduled = retry_timestamp(INITIAL_BACKOFF).into_inner();
        assert!(scheduled >= before + chrono::Duration::seconds(59));
    }

    #[test]
    fn failed_recovery_records_the_real_next_attempt_timestamp() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded_events = Arc::clone(&events);
        let coordinator = PeerDrainCoordinator::new(
            Arc::new(EmptyPeerStore),
            Arc::new(EmptyOutbound),
            Arc::new(std::sync::Mutex::new(None)),
            Arc::new(move |event| recorded_events.lock().expect("event lock").push(event)),
        );
        let host: HostName = "peer.example.test".parse().expect("host");
        let before = Utc::now();
        let _: Result<(), _> = coordinator.failed(
            &host,
            AtmError::remote_delivery_unconfirmed("peer unavailable"),
        );
        let scheduled = events
            .lock()
            .expect("event lock")
            .iter()
            .find_map(|event| event.next_attempt_at)
            .expect("scheduled event timestamp")
            .into_inner();
        assert!(scheduled >= before + chrono::Duration::seconds(59));
    }

    #[test]
    fn same_host_waiter_is_released_without_polling_after_the_active_lease() {
        let coordinator = coordinator_for_slots();
        let host: HostName = "peer.example.test".parse().expect("host");
        coordinator
            .acquire(&host, RequestDeadline::after(Duration::from_secs(1)))
            .expect("first lease");
        let (started, started_rx) = mpsc::sync_channel(1);
        let waiting = Arc::clone(&coordinator);
        let waiting_host = host.clone();
        let waiter = std::thread::spawn(move || {
            started.send(()).expect("report waiter");
            waiting.acquire(
                &waiting_host,
                RequestDeadline::after(Duration::from_secs(1)),
            )
        });
        started_rx.recv().expect("waiter starts");
        let wait_until = Instant::now() + Duration::from_secs(1);
        while coordinator
            .slots()
            .expect("slot lock")
            .get(&host)
            .is_some_and(|slot| slot.requested_generation < 2)
        {
            assert!(
                Instant::now() < wait_until,
                "waiter must contend for the slot"
            );
            std::thread::yield_now();
        }
        coordinator.release(&host).expect("release first lease");
        waiter.join().expect("join waiter").expect("waiter lease");
    }

    #[test]
    fn coordinator_rejects_a_new_host_when_every_slot_has_live_recovery_state() {
        let coordinator = coordinator_for_slots();
        let mut slots = coordinator.slots().expect("slot lock");
        for index in 0..super::MAX_PEER_DRAIN_SLOTS {
            slots.insert(
                format!("peer-{index}.example.test").parse().expect("host"),
                PeerDrainSlot {
                    next_attempt_at: Some(Instant::now() + Duration::from_secs(60)),
                    ..PeerDrainSlot::default()
                },
            );
        }
        drop(slots);
        let host: HostName = "overflow.example.test".parse().expect("host");
        assert!(
            coordinator
                .acquire(&host, RequestDeadline::after(Duration::from_secs(1)))
                .is_err()
        );
    }

    #[test]
    fn coordinator_evicts_an_idle_slot_when_capacity_is_reached() {
        let coordinator = coordinator_for_slots();
        let mut slots = coordinator.slots().expect("slot lock");
        let evicted: HostName = "idle.example.test".parse().expect("host");
        slots.insert(evicted.clone(), PeerDrainSlot::default());
        for index in 1..super::MAX_PEER_DRAIN_SLOTS {
            slots.insert(
                format!("scheduled-{index}.example.test")
                    .parse()
                    .expect("host"),
                PeerDrainSlot {
                    next_attempt_at: Some(Instant::now() + Duration::from_secs(60)),
                    ..PeerDrainSlot::default()
                },
            );
        }
        drop(slots);

        let new_host: HostName = "new.example.test".parse().expect("host");
        coordinator
            .acquire(&new_host, RequestDeadline::after(Duration::from_secs(1)))
            .expect("an idle slot must make room for a new peer");
        let slots = coordinator.slots().expect("slot lock");
        assert!(slots.contains_key(&new_host));
        assert!(!slots.contains_key(&evicted));
        assert_eq!(slots.len(), super::MAX_PEER_DRAIN_SLOTS);
    }

    #[test]
    fn recovery_passes_max_message_age_cutoff_to_storage() {
        let host: HostName = "peer.example.test".parse().expect("host");
        let max_message_age = Duration::from_secs(60);
        let outbound = Arc::new(CapturingOutbound::default());
        let transport: Arc<dyn HttpsMessageTransport> = Arc::new(NeverCalledTransport);
        let coordinator = PeerDrainCoordinator::new(
            Arc::new(ConfiguredPeerStore {
                peer: TrustedPeer {
                    host: host.clone(),
                    fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
                    enabled: true,
                    https_port: NonZeroU16::new(43101).expect("port"),
                },
                policy: PeerSyncPolicy {
                    max_message_age,
                    max_batch_messages: NonZeroU16::new(1).expect("batch cap"),
                },
            }),
            outbound.clone(),
            Arc::new(Mutex::new(Some(transport))),
            Arc::new(|_| {}),
        );

        let before = Utc::now();
        assert_eq!(
            coordinator
                .drain(&host, RequestDeadline::after(Duration::from_secs(1)))
                .expect("empty recovery drain"),
            0
        );
        let after = Utc::now();
        let captured = outbound.not_before.lock().expect("captured cutoff lock");
        assert_eq!(captured.len(), 1, "one empty page query is sufficient");
        let cutoff = captured[0].into_inner();
        let age = chrono::Duration::from_std(max_message_age).expect("bounded age");
        assert!(
            cutoff >= before - age && cutoff <= after - age,
            "storage must receive the actual max_message_age lower bound"
        );
        assert_eq!(
            outbound
                .limits
                .lock()
                .expect("captured limit lock")
                .as_slice(),
            &[NonZeroU16::new(1).expect("non-zero")],
            "reconciliation forwards the configured page cap to storage"
        );
    }
}
