//! Shared bounded substrate for all SQLite read lanes.
//!
//! A lane owns independent defensive read-only connections. The workers are
//! deliberately OS threads because rusqlite connections are thread-affine, but
//! Tokio callers only await bounded channels and oneshots: normal read fan-out
//! never passes through a global blocking bridge.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use atm_storage::{AtmError, ReadLaneError};
use rusqlite::{Connection, InterruptHandle};

use crate::shared_db::SharedDbTarget;
use crate::shared_db_reader_lanes::open_read_connection_for_target;

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkerStatus {
    Ready = 0,
    Quarantined = 1,
}

impl WorkerStatus {
    fn load(source: &AtomicUsize) -> Self {
        match source.load(Ordering::Acquire) {
            value if value == Self::Ready as usize => Self::Ready,
            value if value == Self::Quarantined as usize => Self::Quarantined,
            value => unreachable!("invalid reader worker status {value}"),
        }
    }

    fn compare_exchange(source: &AtomicUsize, current: Self, next: Self) -> Result<usize, usize> {
        source.compare_exchange(
            current as usize,
            next as usize,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
    }
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckpointStatus {
    Unobserved = 0,
    Failed = 1,
    Succeeded = 2,
}

impl CheckpointStatus {
    fn load(source: &AtomicUsize) -> Self {
        match source.load(Ordering::Acquire) {
            value if value == Self::Unobserved as usize => Self::Unobserved,
            value if value == Self::Failed as usize => Self::Failed,
            value if value == Self::Succeeded as usize => Self::Succeeded,
            value => unreachable!("invalid reader checkpoint status {value}"),
        }
    }

    fn from_result(succeeded: bool) -> Self {
        if succeeded {
            Self::Succeeded
        } else {
            Self::Failed
        }
    }

    fn as_option(self) -> Option<bool> {
        match self {
            Self::Unobserved => None,
            Self::Failed => Some(false),
            Self::Succeeded => Some(true),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RequestId(usize);

impl RequestId {
    const NONE: usize = usize::MAX;

    fn next(source: &AtomicUsize) -> Self {
        Self(source.fetch_add(1, Ordering::Relaxed))
    }

    fn is_active(source: &AtomicUsize, expected: Self) -> bool {
        source.load(Ordering::Acquire) == expected.0
    }

    fn activate(self, source: &AtomicUsize) {
        source.store(self.0, Ordering::Release);
    }

    fn clear(source: &AtomicUsize) {
        source.store(Self::NONE, Ordering::Release);
    }
}

/// Snapshot of the per-lane counters. This is intentionally a value object so
/// observability can sample it without holding a worker or SQLite connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderLaneMetricsSnapshot {
    pub lane: &'static str,
    pub queue_depth: usize,
    pub saturated: u64,
    pub in_flight: usize,
    pub wait_nanos: u64,
    pub execution_nanos: u64,
    pub expired_in_queue: u64,
    pub interrupted_while_active: u64,
    pub quarantined: u64,
    pub current_quarantined_workers: usize,
    pub retired_replaced_workers: u64,
    pub quarantine_exhausted_rejections: u64,
    pub pool_size: usize,
    pub last_checkpoint_succeeded: Option<bool>,
    pub current_wal_frames: Option<u64>,
}

/// Backend-neutral value export for reader diagnostics. Lanes are keyed by
/// name so a future lane can join without changing this public value shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderLanesMetricsSnapshot {
    lanes: BTreeMap<&'static str, ReaderLaneMetricsSnapshot>,
}

impl ReaderLanesMetricsSnapshot {
    pub(crate) fn from_lanes(lanes: impl IntoIterator<Item = ReaderLaneMetricsSnapshot>) -> Self {
        Self {
            lanes: lanes
                .into_iter()
                .map(|snapshot| (snapshot.lane, snapshot))
                .collect(),
        }
    }

    #[must_use]
    pub fn lane(&self, name: &str) -> Option<&ReaderLaneMetricsSnapshot> {
        self.lanes.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &ReaderLaneMetricsSnapshot)> {
        self.lanes.iter().map(|(&name, snapshot)| (name, snapshot))
    }
}

#[derive(Debug)]
#[allow(
    dead_code,
    reason = "AV.1b attaches this established reader-lane seam to doctor diagnostics."
)]
struct ReaderLaneMetrics {
    lane: &'static str,
    queue_depth: AtomicUsize,
    saturated: AtomicU64,
    in_flight: AtomicUsize,
    wait_nanos: AtomicU64,
    execution_nanos: AtomicU64,
    expired_in_queue: AtomicU64,
    interrupted_while_active: AtomicU64,
    quarantined: AtomicU64,
    current_quarantined_workers: AtomicUsize,
    retired_replaced_workers: AtomicU64,
    quarantine_exhausted_rejections: AtomicU64,
    pool_size: AtomicUsize,
    last_checkpoint_succeeded: AtomicUsize,
    current_wal_frames: AtomicU64,
}

#[allow(
    dead_code,
    reason = "AV.1b attaches this established reader-lane seam to doctor diagnostics."
)]
impl ReaderLaneMetrics {
    fn new(lane: &'static str, pool_size: usize) -> Self {
        Self {
            lane,
            queue_depth: AtomicUsize::new(0),
            saturated: AtomicU64::new(0),
            in_flight: AtomicUsize::new(0),
            wait_nanos: AtomicU64::new(0),
            execution_nanos: AtomicU64::new(0),
            expired_in_queue: AtomicU64::new(0),
            interrupted_while_active: AtomicU64::new(0),
            quarantined: AtomicU64::new(0),
            current_quarantined_workers: AtomicUsize::new(0),
            retired_replaced_workers: AtomicU64::new(0),
            quarantine_exhausted_rejections: AtomicU64::new(0),
            pool_size: AtomicUsize::new(pool_size),
            last_checkpoint_succeeded: AtomicUsize::new(CheckpointStatus::Unobserved as usize),
            current_wal_frames: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> ReaderLaneMetricsSnapshot {
        let checkpoint = CheckpointStatus::load(&self.last_checkpoint_succeeded);
        ReaderLaneMetricsSnapshot {
            lane: self.lane,
            queue_depth: self.queue_depth.load(Ordering::Relaxed),
            saturated: self.saturated.load(Ordering::Relaxed),
            in_flight: self.in_flight.load(Ordering::Relaxed),
            wait_nanos: self.wait_nanos.load(Ordering::Relaxed),
            execution_nanos: self.execution_nanos.load(Ordering::Relaxed),
            expired_in_queue: self.expired_in_queue.load(Ordering::Relaxed),
            interrupted_while_active: self.interrupted_while_active.load(Ordering::Relaxed),
            quarantined: self.quarantined.load(Ordering::Relaxed),
            current_quarantined_workers: self.current_quarantined_workers.load(Ordering::Relaxed),
            retired_replaced_workers: self.retired_replaced_workers.load(Ordering::Relaxed),
            quarantine_exhausted_rejections: self
                .quarantine_exhausted_rejections
                .load(Ordering::Relaxed),
            pool_size: self.pool_size.load(Ordering::Relaxed),
            last_checkpoint_succeeded: checkpoint.as_option(),
            current_wal_frames: checkpoint
                .as_option()
                .map(|_| self.current_wal_frames.load(Ordering::Relaxed)),
        }
    }

    fn record_wal_health(&self, checkpoint_succeeded: bool, frames: u64) {
        self.current_wal_frames.store(frames, Ordering::Relaxed);
        self.last_checkpoint_succeeded.store(
            CheckpointStatus::from_result(checkpoint_succeeded) as usize,
            Ordering::Release,
        );
    }
}

#[derive(Clone)]
pub(crate) struct ReaderPool {
    inner: Arc<PoolInner>,
}

/// Bounded knobs for one independent read lane. AV.1b adds the doctor lane to
/// the same configuration surface; keeping the budget arithmetic here makes it
/// impossible for a future lane to silently oversubscribe SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReaderPoolConfig {
    pub(crate) pool_size: NonZeroUsize,
    pub(crate) queue_depth: NonZeroUsize,
    pub(crate) interrupt_grace: Duration,
    pub(crate) request_deadline: Duration,
    pub(crate) max_quarantined: NonZeroUsize,
}

impl ReaderPoolConfig {
    pub(crate) const fn mailbox_defaults() -> Self {
        Self {
            pool_size: NonZeroUsize::new(4).expect("non-zero mailbox pool size"),
            queue_depth: NonZeroUsize::new(16).expect("non-zero mailbox queue depth"),
            interrupt_grace: Duration::from_millis(250),
            request_deadline: Duration::from_secs(10),
            max_quarantined: NonZeroUsize::new(4).expect("non-zero mailbox quarantine budget"),
        }
    }

    pub(crate) const fn search_defaults() -> Self {
        Self {
            pool_size: NonZeroUsize::new(2).expect("non-zero search pool size"),
            queue_depth: NonZeroUsize::new(8).expect("non-zero search queue depth"),
            interrupt_grace: Duration::from_millis(250),
            request_deadline: Duration::from_secs(10),
            max_quarantined: NonZeroUsize::new(2).expect("non-zero search quarantine budget"),
        }
    }
}

/// AV.1b's doctor lane is included now so the connection cap remains stable as
/// the stacked handler-cutover branch lands.
pub(crate) const DEFAULT_DOCTOR_READER_CONFIG: ReaderPoolConfig = ReaderPoolConfig {
    pool_size: NonZeroUsize::new(4).expect("non-zero doctor pool size"),
    queue_depth: NonZeroUsize::new(16).expect("non-zero doctor queue depth"),
    interrupt_grace: Duration::from_millis(250),
    request_deadline: Duration::from_secs(10),
    max_quarantined: NonZeroUsize::new(4).expect("non-zero doctor quarantine budget"),
};

pub(crate) const DEFAULT_MAX_READER_CONNECTIONS: NonZeroUsize =
    NonZeroUsize::new(32).expect("non-zero maximum reader connections");

/// The one composition-owned `[reader_lanes]` configuration surface.
///
/// AV.1a keeps it backend-local because no HTTP handler reads it; the storage
/// factory accepts one value and validates the whole connection budget before
/// opening any worker. This prevents mailbox/search/doctor knobs from drifting
/// into per-handler constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReaderLanesConfig {
    pub(crate) mailbox: ReaderPoolConfig,
    pub(crate) search: ReaderPoolConfig,
    pub(crate) doctor: ReaderPoolConfig,
    pub(crate) max_connections: NonZeroUsize,
}

impl Default for ReaderLanesConfig {
    fn default() -> Self {
        Self {
            mailbox: ReaderPoolConfig::mailbox_defaults(),
            search: ReaderPoolConfig::search_defaults(),
            doctor: DEFAULT_DOCTOR_READER_CONFIG,
            max_connections: DEFAULT_MAX_READER_CONNECTIONS,
        }
    }
}

impl ReaderLanesConfig {
    pub(crate) fn validate(self) -> Result<(), AtmError> {
        validate_connection_budget(self.mailbox, self.search, self.doctor, self.max_connections)
    }
}

pub(crate) fn validate_connection_budget(
    mailbox: ReaderPoolConfig,
    search: ReaderPoolConfig,
    doctor: ReaderPoolConfig,
    max_connections: NonZeroUsize,
) -> Result<(), AtmError> {
    let worst_case = 1usize
        .saturating_add(mailbox.pool_size.get())
        .saturating_add(search.pool_size.get())
        .saturating_add(doctor.pool_size.get())
        .saturating_add(mailbox.max_quarantined.get())
        .saturating_add(search.max_quarantined.get())
        .saturating_add(doctor.max_quarantined.get())
        .saturating_add(1); // analyst RO connection
    if mailbox.max_quarantined > mailbox.pool_size
        || search.max_quarantined > search.pool_size
        || doctor.max_quarantined > doctor.pool_size
        || worst_case > max_connections.get()
    {
        return Err(AtmError::validation(format!(
            "SQLite reader connection budget exceeds max_connections: writer=1, mailbox_pool={}, search_pool={}, doctor_pool={}, mailbox_max_quarantined={}, search_max_quarantined={}, doctor_max_quarantined={}, analyst=1, total={worst_case}, max_connections={max_connections}",
            mailbox.pool_size.get(),
            search.pool_size.get(),
            doctor.pool_size.get(),
            mailbox.max_quarantined.get(),
            search.max_quarantined.get(),
            doctor.max_quarantined.get(),
        )));
    }
    Ok(())
}

struct PoolInner {
    lane: &'static str,
    target: Arc<SharedDbTarget>,
    config: ReaderPoolConfig,
    queue_per_worker: usize,
    workers: Mutex<Vec<Worker>>,
    next_worker_index: AtomicUsize,
    next_worker_id: AtomicUsize,
    next_request: AtomicUsize,
    metrics: Arc<ReaderLaneMetrics>,
}

struct Worker {
    id: usize,
    sender: tokio::sync::mpsc::Sender<Request>,
    interrupt: Arc<InterruptHandle>,
    state: Arc<WorkerState>,
}

struct WorkerState {
    status: AtomicUsize,
    active: AtomicBool,
    active_request: AtomicUsize,
}

struct WorkerReservation {
    id: usize,
    sender: tokio::sync::mpsc::Sender<Request>,
    interrupt: Arc<InterruptHandle>,
    state: Arc<WorkerState>,
}

struct Request {
    id: RequestId,
    queued_at: Instant,
    deadline: Instant,
    run: Box<ReaderJob>,
}

type ReaderJob = dyn FnOnce(&Connection, &SharedDbTarget, RequestDisposition) + Send;

enum RequestDisposition {
    Execute,
    ExpiredInQueue,
    Rejected(ReadLaneError),
}

impl std::fmt::Debug for ReaderPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReaderPool")
            .field("lane", &self.inner.lane)
            .field("worker_count", &self.inner.worker_count())
            .finish_non_exhaustive()
    }
}

impl ReaderPool {
    pub(crate) fn start(
        lane: &'static str,
        target: Arc<SharedDbTarget>,
        config: ReaderPoolConfig,
    ) -> Result<Self, AtmError> {
        if config.max_quarantined > config.pool_size {
            return Err(AtmError::validation(format!(
                "SQLite {lane} reader pool requires non-zero pool_size/queue_depth and max_quarantined <= pool_size"
            )));
        }
        let inner = Arc::new(PoolInner {
            lane,
            target,
            queue_per_worker: config.queue_depth.get().div_ceil(config.pool_size.get()),
            config,
            workers: Mutex::new(Vec::with_capacity(config.pool_size.get())),
            next_worker_index: AtomicUsize::new(0),
            next_worker_id: AtomicUsize::new(config.pool_size.get()),
            next_request: AtomicUsize::new(0),
            metrics: Arc::new(ReaderLaneMetrics::new(lane, config.pool_size.get())),
        });
        for worker_id in 0..config.pool_size.get() {
            inner.spawn_worker(worker_id)?;
        }
        Ok(Self { inner })
    }

    pub(crate) fn metrics(&self) -> ReaderLaneMetricsSnapshot {
        self.inner.metrics.snapshot()
    }

    /// The checkpoint owner records WAL health here; reader jobs never run a
    /// checkpoint themselves because that would make an observability sample a
    /// hidden writer-side operation.
    pub(crate) fn record_wal_health(&self, checkpoint_succeeded: bool, frames: u64) {
        self.inner
            .metrics
            .record_wal_health(checkpoint_succeeded, frames);
    }

    pub(crate) async fn submit<T, F>(
        &self,
        deadline: Duration,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        let expires_at = deadline_at(deadline)?;
        let reservation = self.reserve_worker()?;
        let request_id = RequestId::next(&self.inner.next_request);
        let (reply, response) = tokio::sync::oneshot::channel();
        let request = Request {
            id: request_id,
            queued_at: Instant::now(),
            deadline: expires_at,
            run: Box::new(move |connection, target, disposition| {
                let result = match disposition {
                    RequestDisposition::Execute => operation(connection, target),
                    RequestDisposition::ExpiredInQueue => Err(ReadLaneError::DeadlineExpired {
                        stage: "waiting in queue",
                    }),
                    RequestDisposition::Rejected(error) => Err(error),
                };
                let _ = reply.send(result);
            }),
        };
        self.inner
            .metrics
            .queue_depth
            .fetch_add(1, Ordering::Relaxed);
        let remaining = expires_at.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, reservation.sender.send(request)).await {
            Err(_) => {
                self.inner
                    .metrics
                    .queue_depth
                    .fetch_sub(1, Ordering::Relaxed);
                self.inner
                    .metrics
                    .expired_in_queue
                    .fetch_add(1, Ordering::Relaxed);
                return Err(ReadLaneError::DeadlineExpired {
                    stage: "waiting in queue",
                });
            }
            Ok(Err(_)) => {
                self.inner
                    .metrics
                    .queue_depth
                    .fetch_sub(1, Ordering::Relaxed);
                return Err(ReadLaneError::Unavailable {
                    message: "reader worker stopped".to_owned(),
                });
            }
            Ok(Ok(())) => {}
        }
        let remaining = expires_at.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, response).await {
            Err(_) => Err(self.expire_waiting_request(reservation, request_id)),
            Ok(Err(_)) => Err(ReadLaneError::Unavailable {
                message: "reader worker closed its reply channel".to_owned(),
            }),
            Ok(Ok(result)) => result,
        }
    }

    pub(crate) fn submit_blocking<T, F>(
        &self,
        deadline: Duration,
        operation: F,
    ) -> Result<T, ReadLaneError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, &SharedDbTarget) -> Result<T, ReadLaneError> + Send + 'static,
    {
        let expires_at = deadline_at(deadline)?;
        let reservation = self.reserve_worker()?;
        let request_id = RequestId::next(&self.inner.next_request);
        let (reply, response) = mpsc::sync_channel(1);
        let mut request = Request {
            id: request_id,
            queued_at: Instant::now(),
            deadline: expires_at,
            run: Box::new(move |connection, target, disposition| {
                let result = match disposition {
                    RequestDisposition::Execute => operation(connection, target),
                    RequestDisposition::ExpiredInQueue => Err(ReadLaneError::DeadlineExpired {
                        stage: "waiting in queue",
                    }),
                    RequestDisposition::Rejected(error) => Err(error),
                };
                let _ = reply.send(result);
            }),
        };
        loop {
            self.inner
                .metrics
                .queue_depth
                .fetch_add(1, Ordering::Relaxed);
            match reservation.sender.try_send(request) {
                Ok(()) => break,
                Err(tokio::sync::mpsc::error::TrySendError::Full(returned)) => {
                    self.inner
                        .metrics
                        .queue_depth
                        .fetch_sub(1, Ordering::Relaxed);
                    if Instant::now() >= expires_at {
                        self.inner
                            .metrics
                            .expired_in_queue
                            .fetch_add(1, Ordering::Relaxed);
                        return Err(ReadLaneError::DeadlineExpired {
                            stage: "waiting in queue",
                        });
                    }
                    request = returned;
                    thread::park_timeout(Duration::from_millis(2));
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    self.inner
                        .metrics
                        .queue_depth
                        .fetch_sub(1, Ordering::Relaxed);
                    return Err(ReadLaneError::Unavailable {
                        message: "reader worker stopped".to_owned(),
                    });
                }
            }
        }
        response
            .recv_timeout(expires_at.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    self.expire_waiting_request(reservation, request_id)
                }
                mpsc::RecvTimeoutError::Disconnected => ReadLaneError::Unavailable {
                    message: "reader worker closed its reply channel".to_owned(),
                },
            })?
    }

    fn reserve_worker(&self) -> Result<WorkerReservation, ReadLaneError> {
        self.inner.reserve_worker()
    }

    fn schedule_quarantine(&self, reservation: WorkerReservation, request_id: RequestId) {
        let grace = self.inner.config.interrupt_grace;
        let pool = Arc::clone(&self.inner);
        thread::spawn(move || {
            // A watchdog is exceptional cleanup, not a Tokio request task.
            // Park gives the thread no runnable work during its grace delay.
            thread::park_timeout(grace);
            if reservation.state.active.load(Ordering::Acquire)
                && RequestId::is_active(&reservation.state.active_request, request_id)
            {
                pool.quarantine_if_still_active(reservation.id, request_id);
            }
        });
    }

    fn expire_waiting_request(
        &self,
        reservation: WorkerReservation,
        request_id: RequestId,
    ) -> ReadLaneError {
        if reservation.state.active.load(Ordering::Acquire)
            && RequestId::is_active(&reservation.state.active_request, request_id)
        {
            reservation.interrupt.interrupt();
            self.inner
                .metrics
                .interrupted_while_active
                .fetch_add(1, Ordering::Relaxed);
            self.schedule_quarantine(reservation, request_id);
            ReadLaneError::DeadlineExpired {
                stage: "executing active query",
            }
        } else {
            self.inner
                .metrics
                .expired_in_queue
                .fetch_add(1, Ordering::Relaxed);
            ReadLaneError::DeadlineExpired {
                stage: "waiting in queue",
            }
        }
    }
}

fn deadline_at(deadline: Duration) -> Result<Instant, ReadLaneError> {
    Instant::now()
        .checked_add(deadline)
        .ok_or(ReadLaneError::DeadlineExpired {
            stage: "computing reader deadline",
        })
}

impl PoolInner {
    fn worker_count(&self) -> usize {
        self.workers.lock().expect("reader pool lock").len()
    }

    fn spawn_worker(self: &Arc<Self>, id: usize) -> Result<(), AtmError> {
        let connection = open_read_connection_for_target(self.target.as_ref())?;
        let interrupt = Arc::new(connection.get_interrupt_handle());
        let (sender, receiver) = tokio::sync::mpsc::channel(self.queue_per_worker);
        let state = Arc::new(WorkerState {
            status: AtomicUsize::new(WorkerStatus::Ready as usize),
            active: AtomicBool::new(false),
            active_request: AtomicUsize::new(RequestId::NONE),
        });
        let weak = Arc::downgrade(self);
        let worker_state = Arc::clone(&state);
        let worker_metrics = Arc::clone(&self.metrics);
        let worker_target = Arc::clone(&self.target);
        let lane = self.lane;
        thread::Builder::new()
            .name(format!("atm-sqlite-{lane}-reader-{id}"))
            .spawn(move || {
                let retired = run_worker(
                    connection,
                    Arc::clone(&worker_target),
                    receiver,
                    worker_state,
                    worker_metrics,
                );
                // The SQLite connection has to be dropped before a replacement
                // exists; this is the quarantine lifecycle's hard resource cap.
                if retired && let Some(pool) = Weak::upgrade(&weak) {
                    pool.retire_and_replace(id);
                }
            })
            .map_err(|error| {
                AtmError::daemon_unavailable(format!(
                    "failed to start SQLite {lane} reader worker {id}: {error}"
                ))
            })?;
        self.workers.lock().expect("reader pool lock").push(Worker {
            id,
            sender,
            interrupt,
            state,
        });
        Ok(())
    }

    fn reserve_worker(&self) -> Result<WorkerReservation, ReadLaneError> {
        let workers = self.workers.lock().expect("reader pool lock");
        let quarantined = workers
            .iter()
            .filter(|worker| WorkerStatus::load(&worker.state.status) == WorkerStatus::Quarantined)
            .count();
        if quarantined >= self.config.max_quarantined.get() {
            self.metrics
                .quarantine_exhausted_rejections
                .fetch_add(1, Ordering::Relaxed);
            self.metrics.saturated.fetch_add(1, Ordering::Relaxed);
            return Err(ReadLaneError::Saturated {
                reason: "reader quarantine budget exhausted",
            });
        }
        let count = workers.len();
        if count == 0 {
            return Err(ReadLaneError::Unavailable {
                message: "reader pool has no workers".to_owned(),
            });
        }
        let start = self.next_worker_index.fetch_add(1, Ordering::Relaxed);
        for offset in 0..count {
            let worker = &workers[(start + offset) % count];
            if WorkerStatus::load(&worker.state.status) == WorkerStatus::Ready
                && !worker.sender.is_closed()
                && worker.sender.capacity() > 0
            {
                return Ok(WorkerReservation {
                    id: worker.id,
                    sender: worker.sender.clone(),
                    interrupt: Arc::clone(&worker.interrupt),
                    state: Arc::clone(&worker.state),
                });
            }
        }
        self.metrics.saturated.fetch_add(1, Ordering::Relaxed);
        Err(ReadLaneError::Saturated {
            reason: "all bounded reader queues are full",
        })
    }

    fn quarantine_if_still_active(&self, worker_id: usize, request_id: RequestId) {
        let workers = self.workers.lock().expect("reader pool lock");
        let Some(worker) = workers.iter().find(|worker| worker.id == worker_id) else {
            return;
        };
        if !worker.state.active.load(Ordering::Acquire)
            || !RequestId::is_active(&worker.state.active_request, request_id)
        {
            return;
        }
        let existing = workers
            .iter()
            .filter(|candidate| {
                WorkerStatus::load(&candidate.state.status) == WorkerStatus::Quarantined
            })
            .count();
        if existing >= self.config.max_quarantined.get() {
            return;
        }
        if WorkerStatus::compare_exchange(
            &worker.state.status,
            WorkerStatus::Ready,
            WorkerStatus::Quarantined,
        )
        .is_ok()
        {
            self.metrics.quarantined.fetch_add(1, Ordering::Relaxed);
            self.metrics
                .current_quarantined_workers
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn retire_and_replace(self: &Arc<Self>, worker_id: usize) {
        let was_quarantined = {
            let mut workers = self.workers.lock().expect("reader pool lock");
            let Some(position) = workers.iter().position(|worker| worker.id == worker_id) else {
                return;
            };
            let worker = workers.remove(position);
            WorkerStatus::load(&worker.state.status) == WorkerStatus::Quarantined
        };
        if !was_quarantined {
            return;
        }
        self.metrics
            .current_quarantined_workers
            .fetch_sub(1, Ordering::Relaxed);
        self.metrics
            .retired_replaced_workers
            .fetch_add(1, Ordering::Relaxed);
        self.metrics.pool_size.fetch_sub(1, Ordering::Relaxed);
        let replacement_id = self.next_worker_id.fetch_add(1, Ordering::Relaxed);
        if self.spawn_worker(replacement_id).is_err() {
            return;
        }
        self.metrics.pool_size.fetch_add(1, Ordering::Relaxed);
    }
}

fn run_worker(
    connection: Connection,
    target: Arc<SharedDbTarget>,
    mut receiver: tokio::sync::mpsc::Receiver<Request>,
    state: Arc<WorkerState>,
    metrics: Arc<ReaderLaneMetrics>,
) -> bool {
    while let Some(request) = receiver.blocking_recv() {
        let now = Instant::now();
        metrics
            .queue_depth
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |depth| {
                depth.checked_sub(1)
            })
            .ok();
        metrics.wait_nanos.fetch_add(
            u64::try_from(now.saturating_duration_since(request.queued_at).as_nanos())
                .unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        if WorkerStatus::load(&state.status) != WorkerStatus::Ready {
            (request.run)(
                &connection,
                target.as_ref(),
                RequestDisposition::Rejected(ReadLaneError::Saturated {
                    reason: "reader worker is quarantined",
                }),
            );
            continue;
        }
        if now >= request.deadline {
            metrics.expired_in_queue.fetch_add(1, Ordering::Relaxed);
            (request.run)(
                &connection,
                target.as_ref(),
                RequestDisposition::ExpiredInQueue,
            );
            continue;
        }
        request.id.activate(&state.active_request);
        state.active.store(true, Ordering::Release);
        metrics.in_flight.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();
        (request.run)(&connection, target.as_ref(), RequestDisposition::Execute);
        metrics.execution_nanos.fetch_add(
            u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        metrics.in_flight.fetch_sub(1, Ordering::Relaxed);
        state.active.store(false, Ordering::Release);
        RequestId::clear(&state.active_request);
        if WorkerStatus::load(&state.status) == WorkerStatus::Quarantined {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_DOCTOR_READER_CONFIG, DEFAULT_MAX_READER_CONNECTIONS, ReaderLaneMetrics,
        ReaderPool, ReaderPoolConfig, deadline_at, validate_connection_budget,
    };
    use crate::shared_db::SharedDbTarget;
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    static NEXT_TEST_POOL_ID: AtomicUsize = AtomicUsize::new(1);

    fn test_pool(config: ReaderPoolConfig) -> ReaderPool {
        ReaderPool::start(
            "test",
            Arc::new(SharedDbTarget::InMemory {
                uri: format!(
                    "file:reader-pool-test-{}?mode=memory&cache=shared",
                    NEXT_TEST_POOL_ID.fetch_add(1, Ordering::Relaxed)
                ),
            }),
            config,
        )
        .expect("reader pool")
    }

    fn test_config(
        pool_size: usize,
        queue_depth: usize,
        interrupt_grace: Duration,
        max_quarantined: usize,
    ) -> ReaderPoolConfig {
        ReaderPoolConfig {
            pool_size: NonZeroUsize::new(pool_size).expect("non-zero test pool size"),
            queue_depth: NonZeroUsize::new(queue_depth).expect("non-zero test queue depth"),
            interrupt_grace,
            request_deadline: Duration::from_secs(10),
            max_quarantined: NonZeroUsize::new(max_quarantined)
                .expect("non-zero test quarantine budget"),
        }
    }

    #[test]
    fn documented_default_connection_budget_is_within_the_cap() {
        validate_connection_budget(
            ReaderPoolConfig::mailbox_defaults(),
            ReaderPoolConfig::search_defaults(),
            DEFAULT_DOCTOR_READER_CONFIG,
            DEFAULT_MAX_READER_CONNECTIONS,
        )
        .expect("1 writer + 4 mailbox + 2 search + 4 doctor + 10 quarantine + 1 analyst = 22");
    }

    #[test]
    fn reader_lane_default_deadline_is_config_owned() {
        let expected = Duration::from_secs(10);
        assert_eq!(
            ReaderPoolConfig::mailbox_defaults().request_deadline,
            expected
        );
        assert_eq!(
            ReaderPoolConfig::search_defaults().request_deadline,
            expected
        );
        assert_eq!(DEFAULT_DOCTOR_READER_CONFIG.request_deadline, expected);
    }

    #[test]
    fn wal_metrics_distinguish_unobserved_from_zero_frame_checkpoint() {
        let metrics = ReaderLaneMetrics::new("test", 1);
        let unobserved = metrics.snapshot();
        assert_eq!(unobserved.last_checkpoint_succeeded, None);
        assert_eq!(unobserved.current_wal_frames, None);

        metrics.record_wal_health(false, 0);
        let observed = metrics.snapshot();
        assert_eq!(observed.last_checkpoint_succeeded, Some(false));
        assert_eq!(observed.current_wal_frames, Some(0));
    }

    #[test]
    fn connection_budget_fails_closed_and_names_each_contributor() {
        let error = validate_connection_budget(
            ReaderPoolConfig::mailbox_defaults(),
            ReaderPoolConfig::search_defaults(),
            DEFAULT_DOCTOR_READER_CONFIG,
            NonZeroUsize::new(21).expect("non-zero maximum connections"),
        )
        .expect_err("22 reader connections must not fit under a cap of 21");
        let message = error.message();
        assert!(message.contains("mailbox_pool=4"));
        assert!(message.contains("search_pool=2"));
        assert!(message.contains("doctor_pool=4"));
        assert!(message.contains("max_connections=21"));
    }

    #[test]
    fn unrepresentable_reader_deadline_fails_closed() {
        assert_eq!(
            deadline_at(Duration::MAX),
            Err(atm_storage::ReadLaneError::DeadlineExpired {
                stage: "computing reader deadline",
            })
        );
    }

    #[tokio::test]
    async fn two_reader_workers_execute_independent_queries_in_parallel() {
        let pool = test_pool(test_config(2, 2, Duration::from_millis(250), 2));
        let started = Instant::now();
        let (left, right) = tokio::join!(
            pool.submit(Duration::from_secs(1), |_, _| {
                std::thread::park_timeout(Duration::from_millis(80));
                Ok::<_, atm_storage::ReadLaneError>("left")
            }),
            pool.submit(Duration::from_secs(1), |_, _| {
                std::thread::park_timeout(Duration::from_millis(80));
                Ok::<_, atm_storage::ReadLaneError>("right")
            })
        );
        assert_eq!(left.expect("left query"), "left");
        assert_eq!(right.expect("right query"), "right");
        assert!(
            started.elapsed() < Duration::from_millis(145),
            "two worker pool must not serialize independent reads"
        );
    }

    #[tokio::test]
    async fn mailbox_and_search_lanes_do_not_steal_each_others_capacity() {
        let mailbox = Arc::new(test_pool(test_config(1, 1, Duration::from_millis(50), 1)));
        let search = test_pool(test_config(1, 1, Duration::from_millis(50), 1));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let mailbox_task = {
            let mailbox = Arc::clone(&mailbox);
            tokio::spawn(async move {
                mailbox
                    .submit(Duration::from_secs(1), move |_, _| {
                        let _ = started_tx.send(());
                        std::thread::park_timeout(Duration::from_millis(80));
                        Ok::<_, atm_storage::ReadLaneError>(())
                    })
                    .await
            })
        };
        started_rx.await.expect("mailbox worker started");
        let started = Instant::now();
        search
            .submit(Duration::from_millis(100), |_, _| {
                Ok::<_, atm_storage::ReadLaneError>("search")
            })
            .await
            .expect("search lane stays available");
        assert!(
            started.elapsed() < Duration::from_millis(40),
            "search must not wait behind a mailbox worker"
        );
        mailbox_task
            .await
            .expect("mailbox join")
            .expect("mailbox result");
    }

    #[tokio::test]
    async fn bounded_queue_expires_and_rejects_saturation_without_serializing_reads() {
        let pool = Arc::new(test_pool(test_config(1, 1, Duration::from_millis(20), 1)));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let active_pool = Arc::clone(&pool);
        let active_task = tokio::spawn(async move {
            active_pool
                .submit(Duration::from_secs(1), move |_, _| {
                    let _ = started_tx.send(());
                    std::thread::park_timeout(Duration::from_millis(80));
                    Ok::<_, atm_storage::ReadLaneError>(())
                })
                .await
        });
        started_rx.await.expect("worker started");
        // The second request waits in the bounded queue and reaches its own
        // deadline without being mistaken for an active SQLite statement.
        let queued = pool
            .submit(Duration::from_millis(10), |_, _| {
                Ok::<_, atm_storage::ReadLaneError>(())
            })
            .await
            .expect_err("queued request must expire");
        assert_eq!(
            queued,
            atm_storage::ReadLaneError::DeadlineExpired {
                stage: "waiting in queue"
            }
        );
        let saturated = pool
            .submit(Duration::from_millis(20), |_, _| {
                Ok::<_, atm_storage::ReadLaneError>(())
            })
            .await
            .expect_err("full pool plus queue must fail closed");
        assert!(matches!(
            saturated,
            atm_storage::ReadLaneError::Saturated { .. }
        ));
        active_task
            .await
            .expect("join active")
            .expect("active request");
        wait_for_metrics(&pool, |metrics| metrics.queue_depth == 0)
            .await
            .expect("worker drains the expired queued request");
        let metrics = pool.metrics();
        assert!(metrics.expired_in_queue >= 1);
        assert!(metrics.saturated >= 1);
        assert_eq!(metrics.queue_depth, 0);
    }

    #[tokio::test]
    async fn nonresponsive_worker_is_quarantined_then_replaced_only_after_returning() {
        let pool = Arc::new(test_pool(test_config(1, 1, Duration::from_millis(20), 1)));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let active_pool = Arc::clone(&pool);
        let active_task = tokio::spawn(async move {
            active_pool
                .submit(Duration::from_millis(15), move |_, _| {
                    let _ = started_tx.send(());
                    // Deliberately ignores SQLite's interrupt handle: this is
                    // the adversarial worker required by A3c.
                    std::thread::park_timeout(Duration::from_millis(100));
                    Ok::<_, atm_storage::ReadLaneError>(())
                })
                .await
        });
        started_rx.await.expect("worker started");
        let timed_out = active_task
            .await
            .expect("join active")
            .expect_err("deadline");
        assert_eq!(
            timed_out,
            atm_storage::ReadLaneError::DeadlineExpired {
                stage: "executing active query"
            }
        );
        wait_for_metrics(&pool, |metrics| metrics.current_quarantined_workers == 1)
            .await
            .expect("nonresponsive worker enters quarantine after the interrupt grace");
        let quarantined = pool.metrics();
        assert_eq!(quarantined.current_quarantined_workers, 1);
        assert_eq!(quarantined.pool_size, 1, "no early replacement is allowed");
        let exhausted = pool
            .submit(Duration::from_millis(20), |_, _| {
                Ok::<_, atm_storage::ReadLaneError>(())
            })
            .await
            .expect_err("quarantine exhaustion rejects new reads");
        assert!(matches!(
            exhausted,
            atm_storage::ReadLaneError::Saturated {
                reason: "reader quarantine budget exhausted"
            }
        ));
        wait_for_metrics(&pool, |metrics| {
            metrics.current_quarantined_workers == 0
                && metrics.retired_replaced_workers == 1
                && metrics.pool_size == 1
        })
        .await
        .expect("returned worker must retire and be replaced after its connection drops");
        assert_eq!(
            pool.submit(Duration::from_millis(100), |_, _| {
                Ok::<_, atm_storage::ReadLaneError>("reclaimed")
            })
            .await
            .expect("replacement serves a new request"),
            "reclaimed"
        );
        let metrics = pool.metrics();
        assert_eq!(metrics.quarantined, 1);
        assert_eq!(metrics.interrupted_while_active, 1);
        assert_eq!(metrics.quarantine_exhausted_rejections, 1);
    }

    #[tokio::test]
    async fn active_sqlite_statement_is_interrupted_and_worker_capacity_is_reclaimed() {
        let pool = test_pool(test_config(1, 1, Duration::from_millis(100), 1));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (interrupted_tx, interrupted_rx) = std::sync::mpsc::sync_channel(1);
        let timed_out = pool
            .submit(Duration::from_millis(15), move |connection, _| {
                let _ = started_tx.send(());
                let result = connection.query_row(
                    "WITH RECURSIVE count(value) AS (
                         VALUES(0)
                         UNION ALL
                         SELECT value + 1 FROM count WHERE value < 1000000000
                     )
                     SELECT sum(value) FROM count;",
                    [],
                    |_| Ok(()),
                );
                let interrupted = matches!(
                    &result,
                    Err(rusqlite::Error::SqliteFailure(error, _))
                        if error.code == rusqlite::ErrorCode::OperationInterrupted
                );
                let _ = interrupted_tx.send(interrupted);
                result.map_err(|error| atm_storage::ReadLaneError::Unavailable {
                    message: error.to_string(),
                })
            })
            .await
            .expect_err("active recursive statement must reach its deadline");
        assert_eq!(
            timed_out,
            atm_storage::ReadLaneError::DeadlineExpired {
                stage: "executing active query"
            }
        );
        started_rx.await.expect("statement started before deadline");
        assert!(
            interrupted_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("worker reports query termination"),
            "the worker must observe SQLite's interrupt, not merely abandon the query"
        );
        assert_eq!(
            pool.submit(Duration::from_millis(100), |connection, _| {
                connection
                    .query_row("SELECT 1;", [], |row| row.get::<_, i64>(0))
                    .map_err(|error| atm_storage::ReadLaneError::Unavailable {
                        message: error.to_string(),
                    })
            })
            .await
            .expect("interrupted worker remains reusable"),
            1
        );
        let metrics = pool.metrics();
        assert_eq!(metrics.interrupted_while_active, 1);
        assert_eq!(metrics.current_quarantined_workers, 0);
        assert_eq!(metrics.pool_size, 1);
    }

    #[tokio::test]
    async fn metrics_snapshot_exposes_lane_deadline_and_wal_health_seams() {
        let pool = test_pool(test_config(1, 1, Duration::from_millis(20), 1));
        pool.record_wal_health(true, 7);
        pool.submit(Duration::from_millis(100), |_, _| {
            Ok::<_, atm_storage::ReadLaneError>(())
        })
        .await
        .expect("ordinary read");
        wait_for_metrics(&pool, |metrics| metrics.in_flight == 0)
            .await
            .expect("worker reports completion after sending its result");
        let metrics = pool.metrics();
        assert_eq!(metrics.lane, "test");
        assert_eq!(metrics.last_checkpoint_succeeded, Some(true));
        assert_eq!(metrics.current_wal_frames, Some(7));
        assert_eq!(metrics.pool_size, 1);
        assert_eq!(metrics.in_flight, 0);
    }

    async fn wait_for_metrics(
        pool: &ReaderPool,
        predicate: impl Fn(&super::ReaderLaneMetricsSnapshot) -> bool,
    ) -> Result<(), tokio::time::error::Elapsed> {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let metrics = pool.metrics();
                if predicate(&metrics) {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
    }
}
