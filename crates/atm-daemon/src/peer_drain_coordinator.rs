//! Bounded, in-memory recovery of immutable peer-directed writes.
//!
//! This owns scheduling only. Records remain in canonical storage and the
//! transport remains a transport-only capability; no outbox, receipt, cursor,
//! payload, or per-message state is retained here.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atm_core::RequestDeadline;
use atm_core::error::AtmError;
use atm_core::protocol::{ResponseEnvelope, next_request_id};
use atm_core::send::WriteRequest;
use atm_core::types::{HostName, IsoTimestamp};
use atm_storage::{OutboundMessageQuery, PeerConfigStore, TrustedPeer};

use crate::https_transport::{HttpsMessageTransport, resolve_peer_authority};
use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};

const INITIAL_BACKOFF: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);
const MAX_PEER_DRAIN_SLOTS: usize = 256;

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

pub(crate) trait PeerDeliveryCoordinator: Send + Sync {
    fn deliver_after_persist(
        &self,
        request: &WriteRequest,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError>;

    fn sync_peer(&self, peer: &HostName, deadline: RequestDeadline) -> Result<u16, AtmError>;

    fn start(&self) -> Result<(), AtmError>;

    fn stop(&self) -> Result<(), AtmError>;
}

pub(crate) struct PeerDrainCoordinator {
    peers: Arc<dyn PeerConfigStore + Send + Sync>,
    outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
    transport: Arc<Mutex<Option<Arc<dyn HttpsMessageTransport>>>>,
    slots: Arc<Mutex<BTreeMap<HostName, PeerDrainSlot>>>,
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
        transport: Arc<Mutex<Option<Arc<dyn HttpsMessageTransport>>>>,
        record: Arc<dyn Fn(PeerDeliveryEvent) + Send + Sync>,
    ) -> Self {
        Self {
            peers,
            outbound,
            transport,
            slots: Arc::new(Mutex::new(BTreeMap::new())),
            record,
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }

    fn acquire(&self, host: &HostName, deadline: RequestDeadline) -> Result<(), AtmError> {
        loop {
            let mut slots = self.slots.lock().map_err(|_| {
                AtmError::daemon_unavailable("peer drain coordinator slot lock poisoned")
            })?;
            if slots.len() >= MAX_PEER_DRAIN_SLOTS && !slots.contains_key(host) {
                return Err(AtmError::daemon_unavailable(
                    "peer drain coordinator capacity exhausted",
                ));
            }
            let slot = slots.entry(host.clone()).or_default();
            slot.requested_generation = slot.requested_generation.saturating_add(1);
            if !slot.running {
                slot.running = true;
                return Ok(());
            }
            drop(slots);
            if deadline
                .remaining()
                .is_none_or(|remaining| remaining.is_zero())
            {
                return Err(AtmError::remote_delivery_unconfirmed(
                    "peer delivery remained behind the active drain until the request deadline",
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn release(&self, host: &HostName) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(host)
        {
            slot.running = false;
            slot.observed_generation = slot.requested_generation;
        }
    }

    fn mark_generation_observed(&self, host: &HostName) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(host)
        {
            slot.observed_generation = slot.requested_generation;
        }
    }

    fn generation_changed(&self, host: &HostName) -> bool {
        self.slots
            .lock()
            .ok()
            .and_then(|slots| {
                slots
                    .get(host)
                    .map(|slot| slot.requested_generation != slot.observed_generation)
            })
            .unwrap_or(false)
    }

    fn reset_backoff(&self, host: &HostName) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(host)
        {
            slot.backoff = INITIAL_BACKOFF;
            slot.next_attempt_at = None;
        }
    }

    fn record(
        &self,
        kind: PeerDeliveryEventKind,
        peer: HostName,
        error: Option<&AtmError>,
        candidates: Option<u32>,
        next: Option<Instant>,
    ) {
        (self.record)(PeerDeliveryEvent {
            kind,
            request_id: next_request_id(),
            message_id: None,
            peer,
            error_code: error.map(AtmError::code),
            candidate_count: candidates,
            next_attempt_at: next.map(|_| IsoTimestamp::now()),
        });
    }

    fn configured_peer(&self, host: &HostName) -> Result<TrustedPeer, AtmError> {
        resolve_peer_authority(host, &self.peers.list_trusted_peers()?)
    }

    fn drain(&self, host: &HostName, deadline: RequestDeadline) -> Result<u16, AtmError> {
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
        self.record(
            PeerDeliveryEventKind::PeerRecoveryAttempt,
            host.clone(),
            None,
            None,
            None,
        );
        let mut after = None;
        let mut delivered = 0_u16;
        self.mark_generation_observed(host);
        loop {
            let page =
                self.outbound
                    .page_for_peer(host, not_before, after, policy.max_batch_messages)?;
            if page.is_empty() {
                if self.generation_changed(host) {
                    self.mark_generation_observed(host);
                    after = None;
                    continue;
                }
                return Ok(self.finish_drain(host, delivered));
            }
            let requests = Self::decode_page_requests(&page)?;
            let responses = match transport.deliver_page(&requests, &peer, deadline) {
                Ok(responses) => responses,
                Err(error) => {
                    return self
                        .failed(host, AtmError::remote_delivery_unconfirmed(error.message()));
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
                    return self.failed(host, error);
                }
                delivered = delivered.saturating_add(1);
                after = Some((stored.created_at, stored.message_id));
            }
            if page.len() < usize::from(policy.max_batch_messages.get()) {
                return Ok(self.finish_drain(host, delivered));
            }
        }
    }

    fn decode_page_requests(
        page: &[atm_storage::StoredPeerWrite],
    ) -> Result<Vec<WriteRequest>, AtmError> {
        page.iter()
            .map(|stored| {
                serde_json::from_str(&stored.request_json).map_err(|_| {
                    AtmError::mailbox_read("stored immutable peer outbound write is invalid")
                })
            })
            .collect()
    }

    fn finish_drain(&self, host: &HostName, delivered: u16) -> u16 {
        self.record(
            PeerDeliveryEventKind::PeerRecoveryConfirmed,
            host.clone(),
            None,
            Some(u32::from(delivered)),
            None,
        );
        self.reset_backoff(host);
        delivered
    }

    fn failed<T>(&self, host: &HostName, error: AtmError) -> Result<T, AtmError> {
        let next = if let Ok(mut slots) = self.slots.lock() {
            let slot = slots.entry(host.clone()).or_default();
            let next = Instant::now() + slot.backoff;
            slot.next_attempt_at = Some(next);
            slot.backoff = slot.backoff.saturating_mul(2).min(MAX_BACKOFF);
            Some(next)
        } else {
            None
        };
        self.record(
            PeerDeliveryEventKind::PeerRecoveryUnconfirmed,
            host.clone(),
            Some(&error),
            None,
            next,
        );
        self.record(
            PeerDeliveryEventKind::PeerRecoveryScheduled,
            host.clone(),
            Some(&error),
            None,
            next,
        );
        Err(error)
    }

    fn deliver_current(
        &self,
        request: &WriteRequest,
        host: &HostName,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
        let peer = self.configured_peer(host)?;
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
        match transport.deliver(request.clone(), &peer, deadline) {
            Ok(ResponseEnvelope::Error(error)) => Err(error),
            Ok(_) => {
                self.record(
                    PeerDeliveryEventKind::PeerDeliveryConfirmed,
                    host.clone(),
                    None,
                    Some(1),
                    None,
                );
                Ok(())
            }
            Err(error) => Err(AtmError::remote_delivery_unconfirmed(error.message())),
        }
    }

    fn worker_clone(&self) -> Self {
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
                .page_for_peer(&peer.host, not_before, None, policy.max_batch_messages)?
                .is_empty()
            {
                continue;
            }
            let next = Instant::now() + INITIAL_BACKOFF;
            self.slots
                .lock()
                .map_err(|_| {
                    AtmError::daemon_unavailable("peer drain coordinator slot lock poisoned")
                })?
                .entry(peer.host.clone())
                .or_default()
                .next_attempt_at = Some(next);
            self.record(
                PeerDeliveryEventKind::PeerRecoveryScheduled,
                peer.host,
                None,
                None,
                Some(next),
            );
        }
        Ok(())
    }

    fn run_scheduled_recovery(&self) {
        while !self.stop.load(Ordering::SeqCst) {
            let due = self.slots.lock().ok().and_then(|mut slots| {
                let now = Instant::now();
                slots.iter_mut().find_map(|(host, slot)| {
                    (slot.next_attempt_at.is_some_and(|next| next <= now) && !slot.running).then(
                        || {
                            slot.running = true;
                            host.clone()
                        },
                    )
                })
            });
            if let Some(host) = due {
                let _ = self.drain(&host, RequestDeadline::after(Duration::from_secs(5)));
                self.release(&host);
                continue;
            }
            std::thread::park_timeout(Duration::from_millis(250));
        }
    }
}

impl PeerDeliveryCoordinator for PeerDrainCoordinator {
    fn deliver_after_persist(
        &self,
        request: &WriteRequest,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
        let host = request
            .to
            .as_ref()
            .and_then(|address| address.host())
            .ok_or_else(|| {
                AtmError::validation("peer delivery coordinator requires a host-qualified write")
            })?
            .clone();
        if self
            .peers
            .peer_sync_policy(&host)?
            .validate()?
            .max_message_age
            .is_zero()
        {
            return self.deliver_current(request, &host, deadline);
        }
        self.acquire(&host, deadline)?;
        let result = self.drain(&host, deadline);
        self.release(&host);
        result.map(|count| {
            self.record(
                PeerDeliveryEventKind::PeerDeliveryConfirmed,
                host,
                None,
                Some(u32::from(count)),
                None,
            );
        })
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
        self.release(peer);
        result
    }

    fn start(&self) -> Result<(), AtmError> {
        self.schedule_startup_recovery()?;
        let mut worker = self.worker.lock().map_err(|_| {
            AtmError::daemon_unavailable("peer drain coordinator worker lock poisoned")
        })?;
        if worker.is_none() {
            self.stop.store(false, Ordering::SeqCst);
            let coordinator = self.worker_clone();
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
        self.slots
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer drain coordinator slot lock poisoned"))?
            .clear();
        Ok(())
    }
}
