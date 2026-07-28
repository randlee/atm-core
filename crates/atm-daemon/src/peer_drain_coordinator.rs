//! Bounded, non-durable scheduling of immutable peer writes.
//!
//! The coordinator retains only `(peer, message_id)` work identifiers while a
//! job is queued or running.  Canonical SQLite remains the source of payloads
//! and eligibility, so restart simply loses a wake-up rather than delivery
//! state.  In particular this module deliberately has no cursor, receipt,
//! retry history, FIFO promise, or stream abstraction.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::RequestDeadline;
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{ResponseEnvelope, next_request_id};
use atm_core::schema::AtmMessageId;
use atm_core::send::WriteRequest;
use atm_core::types::{HostName, IsoTimestamp};
use atm_storage::{OutboundMessageQuery, PeerConfigStore, StoredPeerWrite, TrustedPeer};

use crate::https_transport::SharedHttpsTransport;
use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};
use crate::runtime_health::peer_authority::resolve_peer_authority;

pub(crate) const POST_COMMIT_QUEUE_DEPTH: usize = 256;
pub(crate) const MAX_ACTIVE_PEER_JOBS: usize = 64;
pub(crate) const MAX_ACTIVE_PEER_JOBS_PER_HOST: usize = 8;
pub(crate) const PEER_DELIVERY_WORKER_DEADLINE: Duration = Duration::from_secs(10);
pub(crate) const PEER_SYNC_REQUEST_DEADLINE: Duration = PEER_DELIVERY_WORKER_DEADLINE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerSyncOutcome {
    Confirmed { delivered: u16 },
    Unconfirmed { code: AtmErrorCode },
    Expired { code: AtmErrorCode },
}

pub(crate) trait PeerDeliveryCoordinator: Send + Sync {
    fn signal_after_persist(&self, peer: HostName, message_id: AtmMessageId);
    fn sync_peer(
        &self,
        peer: &HostName,
        deadline: RequestDeadline,
    ) -> Result<PeerSyncOutcome, AtmError>;
    fn start(&self) -> Result<(), AtmError>;
    fn stop(&self) -> Result<(), AtmError>;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PeerJob {
    peer: HostName,
    message_id: AtmMessageId,
}

#[derive(Default)]
struct JobState {
    in_flight: BTreeSet<PeerJob>,
    active_by_host: BTreeMap<HostName, usize>,
}

/// The only daemon owner of peer delivery scheduling.  It has identifiers and
/// counters only; every job reloads immutable request data from storage.
pub(crate) struct PeerDrainCoordinator {
    peers: Arc<dyn PeerConfigStore + Send + Sync>,
    outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
    transport: SharedHttpsTransport,
    record: Arc<dyn Fn(PeerDeliveryEvent) + Send + Sync>,
    sender: SyncSender<PeerJob>,
    receiver: Mutex<Option<Receiver<PeerJob>>>,
    state: Arc<Mutex<JobState>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl PeerDrainCoordinator {
    pub(crate) fn new(
        peers: Arc<dyn PeerConfigStore + Send + Sync>,
        outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
        transport: SharedHttpsTransport,
        record: Arc<dyn Fn(PeerDeliveryEvent) + Send + Sync>,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel(POST_COMMIT_QUEUE_DEPTH);
        Self {
            peers,
            outbound,
            transport,
            record,
            sender,
            receiver: Mutex::new(Some(receiver)),
            state: Arc::new(Mutex::new(JobState::default())),
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }

    fn record(&self, kind: PeerDeliveryEventKind, job: &PeerJob, error: Option<&AtmError>) {
        (self.record)(PeerDeliveryEvent {
            kind,
            request_id: next_request_id(),
            message_id: Some(job.message_id),
            peer: job.peer.clone(),
            error_code: error.map(AtmError::code),
            candidate_count: None,
            next_attempt_at: None,
        });
    }

    fn take_job(&self, job: &PeerJob) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.in_flight.len() >= MAX_ACTIVE_PEER_JOBS
            || state
                .active_by_host
                .get(&job.peer)
                .copied()
                .unwrap_or_default()
                >= MAX_ACTIVE_PEER_JOBS_PER_HOST
        {
            return false;
        }
        if state.in_flight.insert(job.clone()) {
            *state.active_by_host.entry(job.peer.clone()).or_default() += 1;
            true
        } else {
            false
        }
    }

    fn release_job(state: &Mutex<JobState>, job: &PeerJob) {
        if let Ok(mut state) = state.lock() {
            state.in_flight.remove(job);
            let count = state.active_by_host.entry(job.peer.clone()).or_default();
            *count = count.saturating_sub(1);
            if *count == 0 {
                state.active_by_host.remove(&job.peer);
            }
        }
    }

    fn eligible_request(
        &self,
        job: &PeerJob,
        deadline: RequestDeadline,
    ) -> Result<Option<WriteRequest>, AtmError> {
        let policy = self.peers.peer_sync_policy(&job.peer)?.validate()?;
        if policy.max_message_age.is_zero() {
            return Ok(None);
        }
        let not_before = IsoTimestamp::from_datetime(
            chrono::Utc::now()
                - chrono::Duration::from_std(policy.max_message_age).map_err(|_| {
                    AtmError::validation("peer sync maximum message age is out of range")
                })?,
        );
        let budget = deadline.remaining().ok_or_else(|| {
            AtmError::remote_delivery_unconfirmed(
                "peer delivery deadline elapsed before storage lookup",
            )
        })?;
        let page = self.outbound.page_for_peer(
            &job.peer,
            not_before,
            None,
            policy.max_batch_messages,
            budget,
        )?;
        page.into_iter()
            .find(|stored| stored.message_id == job.message_id)
            .map(decode_request)
            .transpose()
    }

    fn deliver_one(&self, job: &PeerJob) -> Result<(), AtmError> {
        let deadline = RequestDeadline::after(PEER_DELIVERY_WORKER_DEADLINE);
        self.record(PeerDeliveryEventKind::PeerRecoveryAttempt, job, None);
        let Some(request) = self.eligible_request(job, deadline)? else {
            return Ok(());
        };
        let peer: TrustedPeer =
            resolve_peer_authority(&job.peer, &self.peers.list_trusted_peers()?)?;
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
        match transport.deliver(request, &peer, deadline)? {
            ResponseEnvelope::Error(error) => Err(error),
            _ => {
                self.record(PeerDeliveryEventKind::PeerDeliveryConfirmed, job, None);
                Ok(())
            }
        }
    }

    fn run(self: Arc<Self>, receiver: Receiver<PeerJob>) {
        while !self.stop.load(Ordering::SeqCst) {
            let Ok(job) = receiver.recv_timeout(Duration::from_millis(100)) else {
                continue;
            };
            let coordinator = Arc::clone(&self);
            std::thread::spawn(move || {
                let result = coordinator.deliver_one(&job);
                if let Err(error) = result {
                    coordinator.record(
                        PeerDeliveryEventKind::PeerDeliveryUnconfirmed,
                        &job,
                        Some(&error),
                    );
                }
                Self::release_job(&coordinator.state, &job);
            });
        }
    }
}

impl PeerDeliveryCoordinator for PeerDrainCoordinator {
    fn signal_after_persist(&self, peer: HostName, message_id: AtmMessageId) {
        let job = PeerJob { peer, message_id };
        if !self.take_job(&job) {
            return;
        }
        if let Err(error) = self.sender.try_send(job.clone()) {
            Self::release_job(&self.state, &job);
            if !matches!(error, TrySendError::Full(_)) {
                tracing::warn!(
                    subsystem = "peer_drain",
                    action = "signal",
                    "peer delivery worker unavailable"
                );
            }
        }
    }

    fn sync_peer(
        &self,
        peer: &HostName,
        deadline: RequestDeadline,
    ) -> Result<PeerSyncOutcome, AtmError> {
        if deadline.expired() {
            return Ok(PeerSyncOutcome::Expired {
                code: AtmErrorCode::RemoteDeliveryUnconfirmed,
            });
        }
        let policy = self.peers.peer_sync_policy(peer)?.validate()?;
        let not_before = IsoTimestamp::from_datetime(
            chrono::Utc::now()
                - chrono::Duration::from_std(policy.max_message_age).map_err(|_| {
                    AtmError::validation("peer sync maximum message age is out of range")
                })?,
        );
        let page = self.outbound.page_for_peer(
            peer,
            not_before,
            None,
            policy.max_batch_messages,
            deadline.remaining().ok_or_else(|| {
                AtmError::remote_delivery_unconfirmed("peer synchronization deadline elapsed")
            })?,
        )?;
        if self.state.lock().is_err() {
            return Ok(PeerSyncOutcome::Unconfirmed {
                code: AtmErrorCode::DaemonUnavailable,
            });
        }
        for stored in &page {
            self.signal_after_persist(peer.clone(), stored.message_id);
        }
        Ok(PeerSyncOutcome::Confirmed {
            delivered: page.len().try_into().unwrap_or(u16::MAX),
        })
    }

    fn start(&self) -> Result<(), AtmError> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer delivery lifecycle lock poisoned"))?;
        if worker.is_none() {
            let receiver = self
                .receiver
                .lock()
                .map_err(|_| AtmError::daemon_unavailable("peer delivery receiver lock poisoned"))?
                .take()
                .ok_or_else(|| {
                    AtmError::daemon_unavailable(
                        "peer delivery worker cannot restart after shutdown",
                    )
                })?;
            self.stop.store(false, Ordering::SeqCst);
            // A small dispatcher only accepts identifiers; each worker is independently bounded by state.
            let coordinator = Arc::new(Self {
                peers: Arc::clone(&self.peers),
                outbound: Arc::clone(&self.outbound),
                transport: Arc::clone(&self.transport),
                record: Arc::clone(&self.record),
                sender: self.sender.clone(),
                receiver: Mutex::new(None),
                state: Arc::clone(&self.state),
                stop: Arc::clone(&self.stop),
                worker: Mutex::new(None),
            });
            *worker = Some(
                std::thread::Builder::new()
                    .name("atm-peer-jobs".into())
                    .spawn(move || coordinator.run(receiver))
                    .map_err(|_| {
                        AtmError::daemon_unavailable("failed to start peer delivery worker")
                    })?,
            );
        }
        Ok(())
    }

    fn stop(&self) -> Result<(), AtmError> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self
            .worker
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer delivery lifecycle lock poisoned"))?
            .take()
        {
            worker.join().map_err(|_| {
                AtmError::daemon_unavailable("peer delivery worker panicked during shutdown")
            })?;
        }
        Ok(())
    }
}

fn decode_request(stored: StoredPeerWrite) -> Result<WriteRequest, AtmError> {
    serde_json::from_str(&stored.request_json).map_err(|source| {
        AtmError::mailbox_read("stored immutable peer outbound write is invalid").with_cause(source)
    })
}
