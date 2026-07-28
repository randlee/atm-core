//! Bounded, non-durable scheduling of immutable peer writes.
//!
//! The coordinator retains only `(peer, message_id)` work identifiers while a
//! job is queued or running.  Canonical SQLite remains the source of payloads
//! and eligibility, so restart simply loses a wake-up rather than delivery
//! state.  In particular this module deliberately has no cursor, receipt,
//! retry history, FIFO promise, or stream abstraction.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
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

enum EligiblePeerWrite {
    Missing,
    Expired,
    Ready(Box<WriteRequest>),
}

enum PeerWork {
    Job(Box<PeerJob>),
    Stop,
}

#[derive(Default)]
struct JobState {
    in_flight: BTreeSet<PeerJob>,
    active_by_host: BTreeMap<HostName, usize>,
}

impl JobState {
    fn try_take(&mut self, job: &PeerJob) -> bool {
        if self.in_flight.len() >= MAX_ACTIVE_PEER_JOBS
            || self
                .active_by_host
                .get(&job.peer)
                .copied()
                .unwrap_or_default()
                >= MAX_ACTIVE_PEER_JOBS_PER_HOST
        {
            return false;
        }
        if self.in_flight.insert(job.clone()) {
            *self.active_by_host.entry(job.peer.clone()).or_default() += 1;
            true
        } else {
            false
        }
    }

    fn release(&mut self, job: &PeerJob) {
        self.in_flight.remove(job);
        let count = self.active_by_host.entry(job.peer.clone()).or_default();
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.active_by_host.remove(&job.peer);
        }
    }
}

/// The only daemon owner of peer delivery scheduling.  It has identifiers and
/// counters only; every job reloads immutable request data from storage.
pub(crate) struct PeerDrainCoordinator {
    peers: Arc<dyn PeerConfigStore + Send + Sync>,
    outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
    transport: SharedHttpsTransport,
    record: Arc<dyn Fn(PeerDeliveryEvent) + Send + Sync>,
    sender: Mutex<Option<SyncSender<PeerWork>>>,
    state: Arc<Mutex<JobState>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    job_workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
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
            record,
            sender: Mutex::new(None),
            state: Arc::new(Mutex::new(JobState::default())),
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
            job_workers: Arc::new(Mutex::new(Vec::new())),
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
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                tracing::warn!(
                    subsystem = "peer_drain",
                    action = "take_job",
                    outcome = "lock_poisoned",
                    "peer delivery job admission skipped because coordinator state is poisoned"
                );
                return false;
            }
        };
        state.try_take(job)
    }

    fn release_job(state: &Mutex<JobState>, job: &PeerJob) {
        let mut state = match state.lock() {
            Ok(state) => state,
            Err(_) => {
                tracing::warn!(subsystem = "peer_drain", action = "release_job", outcome = "lock_poisoned", peer = %job.peer, message_id = %job.message_id, "peer delivery job cleanup could not release its coordinator slot");
                return;
            }
        };
        state.release(job);
    }

    fn eligible_request(
        &self,
        job: &PeerJob,
        deadline: RequestDeadline,
    ) -> Result<EligiblePeerWrite, AtmError> {
        let policy = self.peers.peer_sync_policy(&job.peer)?.validate()?;
        if policy.max_message_age.is_zero() {
            return Ok(EligiblePeerWrite::Missing);
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
            IsoTimestamp::from_datetime(chrono::DateTime::<chrono::Utc>::UNIX_EPOCH),
            None,
            std::num::NonZeroU16::new(atm_storage::MAX_PEER_SYNC_BATCH_MESSAGES)
                .expect("hard peer sync cap is non-zero"),
            budget,
        )?;
        let Some(stored) = page
            .into_iter()
            .find(|stored| stored.message_id == job.message_id)
        else {
            return Ok(EligiblePeerWrite::Missing);
        };
        if stored.created_at < not_before {
            return Ok(EligiblePeerWrite::Expired);
        }
        Ok(EligiblePeerWrite::Ready(Box::new(decode_request(stored)?)))
    }

    fn deliver_one(&self, job: &PeerJob) -> Result<(), AtmError> {
        let deadline = RequestDeadline::after(PEER_DELIVERY_WORKER_DEADLINE);
        self.record(PeerDeliveryEventKind::PeerRecoveryAttempt, job, None);
        let request = match self.eligible_request(job, deadline)? {
            EligiblePeerWrite::Missing => return Ok(()),
            EligiblePeerWrite::Expired => {
                let error = AtmError::remote_delivery_unconfirmed("peer delivery window expired");
                self.record(
                    PeerDeliveryEventKind::PeerDeliveryExpired,
                    job,
                    Some(&error),
                );
                return Ok(());
            }
            EligiblePeerWrite::Ready(request) => *request,
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

    fn run(self: Arc<Self>, receiver: Receiver<PeerWork>) {
        while let Ok(work) = receiver.recv() {
            let PeerWork::Job(job) = work else { break };
            let coordinator = Arc::clone(&self);
            let handle = std::thread::spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| coordinator.deliver_one(&job)));
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => coordinator.record(
                        PeerDeliveryEventKind::PeerDeliveryUnconfirmed,
                        &job,
                        Some(&error),
                    ),
                    Err(_) => {
                        let error = AtmError::daemon_unavailable("peer delivery job panicked");
                        coordinator.record(
                            PeerDeliveryEventKind::PeerDeliveryUnconfirmed,
                            &job,
                            Some(&error),
                        );
                        tracing::error!(subsystem = "peer_drain", action = "deliver", peer = %job.peer, message_id = %job.message_id, "peer delivery job panicked; slot released");
                    }
                }
                Self::release_job(&coordinator.state, &job);
            });
            if let Ok(mut workers) = self.job_workers.lock() {
                workers.push(handle);
            }
        }
        while let Ok(PeerWork::Job(job)) = receiver.try_recv() {
            Self::release_job(&self.state, &job);
        }
    }
}

impl PeerDeliveryCoordinator for PeerDrainCoordinator {
    fn signal_after_persist(&self, peer: HostName, message_id: AtmMessageId) {
        let job = PeerJob { peer, message_id };
        if !self.take_job(&job) {
            return;
        }
        let sender = match self.sender.lock() {
            Ok(sender) => sender.clone(),
            Err(_) => None,
        };
        let Some(sender) = sender else {
            Self::release_job(&self.state, &job);
            tracing::warn!(
                subsystem = "peer_drain",
                action = "signal",
                outcome = "worker_unavailable",
                "peer delivery worker is not running"
            );
            return;
        };
        if let Err(error) = sender.try_send(PeerWork::Job(Box::new(job.clone()))) {
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
            let (sender, receiver) = mpsc::sync_channel(POST_COMMIT_QUEUE_DEPTH);
            *self.sender.lock().map_err(|_| {
                AtmError::daemon_unavailable("peer delivery sender lock poisoned")
            })? = Some(sender);
            self.stop.store(false, Ordering::SeqCst);
            // A small dispatcher only accepts identifiers; each worker is independently bounded by state.
            let coordinator = Arc::new(Self {
                peers: Arc::clone(&self.peers),
                outbound: Arc::clone(&self.outbound),
                transport: Arc::clone(&self.transport),
                record: Arc::clone(&self.record),
                sender: Mutex::new(None),
                state: Arc::clone(&self.state),
                stop: Arc::clone(&self.stop),
                worker: Mutex::new(None),
                job_workers: Arc::clone(&self.job_workers),
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
        let sender = self
            .sender
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("peer delivery sender lock poisoned"))?
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(PeerWork::Stop);
        }
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
        let workers =
            std::mem::take(&mut *self.job_workers.lock().map_err(|_| {
                AtmError::daemon_unavailable("peer delivery job worker lock poisoned")
            })?);
        for worker in workers {
            worker.join().map_err(|_| {
                AtmError::daemon_unavailable("peer delivery job panicked during shutdown")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn job(peer: &str) -> PeerJob {
        PeerJob {
            peer: peer.parse().expect("peer"),
            message_id: AtmMessageId::new(),
        }
    }

    #[test]
    fn job_state_coalesces_duplicates_and_enforces_per_host_capacity() {
        let mut state = JobState::default();
        let first = job("peer.example.test");
        assert!(state.try_take(&first));
        assert!(
            !state.try_take(&first),
            "same ULID is coalesced while in flight"
        );
        for _ in 1..MAX_ACTIVE_PEER_JOBS_PER_HOST {
            assert!(state.try_take(&job("peer.example.test")));
        }
        assert!(
            !state.try_take(&job("peer.example.test")),
            "per-host cap blocks only worker starts"
        );
        assert!(
            state.try_take(&job("other.example.test")),
            "a stalled host cannot consume another host's slot"
        );
        state.release(&first);
        assert!(
            state.try_take(&first),
            "completed work may be rediscovered idempotently"
        );
    }
}
