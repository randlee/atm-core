use arc_swap::ArcSwap;
use atm_core::error::AtmError;
use atm_core::protocol::NotificationEvent;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::AtomicU8;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(test)]
use crate::DaemonSubsystem;
use crate::SubsystemObservability;
use crate::worker_support::{JoinHandleOwner, reap_retained_join_helpers, retain_join_helper};

const DEFAULT_NOTIFICATION_QUEUE_CAPACITY: usize = 64;
const DEFAULT_NOTIFICATION_IDLE_INTERVAL: Duration = Duration::from_millis(50);
const NOTIFICATION_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);
const MAX_NOTIFICATION_EVENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationWorkerLiveness {
    Live,
    Degraded,
    Stopped,
}

#[derive(Debug, Clone)]
pub(crate) struct NotificationRuntimeStatus {
    started: bool,
    shutdown_requested: bool,
    shutdown_started_at: Option<Instant>,
    degraded_message: Option<Arc<str>>,
    worker_liveness: NotificationWorkerLiveness,
}

impl Default for NotificationRuntimeStatus {
    fn default() -> Self {
        Self {
            started: false,
            shutdown_requested: false,
            shutdown_started_at: None,
            degraded_message: None,
            worker_liveness: NotificationWorkerLiveness::Stopped,
        }
    }
}

#[derive(Debug)]
pub(crate) enum NotificationCommand {
    Deliver { event: NotificationEvent },
}

#[derive(Clone)]
pub(crate) struct NotificationRuntime {
    inner: Arc<NotificationRuntimeInner>,
}

type NotificationPathFactory = Arc<dyn Fn() -> Result<PathBuf, AtmError> + Send + Sync>;

struct NotificationRuntimeInner {
    command_tx: SyncSender<NotificationCommand>,
    // The worker lane claims the receiver exactly once at startup; the mutex
    // exists only to serialize that single take across concurrent start calls.
    command_rx: Mutex<Option<Receiver<NotificationCommand>>>,
    queue_capacity: usize,
    status: Arc<ArcSwap<NotificationRuntimeStatus>>,
    worker: Arc<JoinHandleOwner>,
    path_factory: NotificationPathFactory,
    shutdown_deadline: Duration,
    observability: SubsystemObservability,
    start_claimed: AtomicBool,
    #[cfg(test)]
    liveness_override: AtomicU8,
}

impl NotificationRuntime {
    pub(crate) fn new_with_observability(observability: SubsystemObservability) -> Self {
        Self::new_with_path_factory(
            Arc::new(|| Ok(atm_core::home::host_runtime_dir()?.join("notifications.jsonl"))),
            DEFAULT_NOTIFICATION_QUEUE_CAPACITY,
            observability,
        )
    }

    fn new_with_path_factory(
        path_factory: NotificationPathFactory,
        queue_capacity: usize,
        observability: SubsystemObservability,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::sync_channel(queue_capacity);
        Self {
            inner: Arc::new(NotificationRuntimeInner {
                command_tx,
                command_rx: Mutex::new(Some(command_rx)),
                queue_capacity,
                status: Arc::new(ArcSwap::from_pointee(NotificationRuntimeStatus::default())),
                worker: Arc::new(JoinHandleOwner::default()),
                path_factory,
                shutdown_deadline: NOTIFICATION_SHUTDOWN_DEADLINE,
                observability,
                start_claimed: AtomicBool::new(false),
                #[cfg(test)]
                liveness_override: AtomicU8::new(0),
            }),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        reap_retained_join_helpers();
        if self.inner.start_claimed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        let Some(command_rx) = self.inner.take_command_rx()? else {
            self.inner.start_claimed.store(false, Ordering::Release);
            return Err(AtmError::daemon_unavailable(
                "notification runtime command receiver was unavailable during startup",
            )
            .with_recovery(
                "Restart atm-daemon; the notification worker lane could not claim its command receiver.",
            ));
        };

        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("atm-daemon-notifier".to_string())
            .spawn(move || notification_worker_loop(inner, command_rx))
            .map_err(|source| {
                self.inner.start_claimed.store(false, Ordering::Release);
                self.inner.observability.emit_or_warn(
                    "start",
                    "failed",
                    "failed to spawn notification runtime worker",
                );
                AtmError::daemon_unavailable("failed to spawn notification runtime worker")
                    .with_source(source)
            })?;

        self.inner.worker.install(handle).inspect_err(|_| {
            self.inner.start_claimed.store(false, Ordering::Release);
        })?;
        self.inner.mark_started();
        tracing::info!(
            subsystem = "notification",
            action = "worker_start",
            outcome = "configured",
            queue_capacity = self.inner.queue_capacity,
            idle_interval_ms = DEFAULT_NOTIFICATION_IDLE_INTERVAL.as_millis(),
            "notification runtime worker configuration accepted"
        );
        self.inner
            .observability
            .emit_or_warn("start", "ok", "notification runtime worker started");
        Ok(())
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        let Some(handle) = self.take_worker_for_shutdown()? else {
            self.inner.mark_shutdown_requested();
            self.inner.mark_worker_stopped();
            return Ok(());
        };
        self.await_worker_shutdown(handle)
    }

    pub(crate) fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError> {
        let status = self.inner.status_snapshot();
        if !status.started {
            return Err(AtmError::daemon_unavailable(
                "notification runtime is unavailable before daemon startup",
            )
            .with_recovery("Start or restart atm-daemon before retrying notification delivery."));
        }
        if status.shutdown_requested {
            return Err(AtmError::daemon_unavailable(
                "notification runtime is unavailable during daemon shutdown",
            )
            .with_recovery(
                "Wait for atm-daemon to finish shutting down or restart it before retrying notification delivery.",
            ));
        }
        if let Some(message) = &status.degraded_message {
            self.inner.observability.emit_or_warn(
                "deliver",
                "degraded",
                "notification runtime is degraded and rejecting delivery",
            );
            return Err(
                AtmError::daemon_unavailable(message.as_ref()).with_recovery(
                    "Restart atm-daemon; the notification persistence lane is degraded.",
                ),
            );
        }

        match self
            .inner
            .command_tx
            .try_send(NotificationCommand::Deliver { event })
        {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.inner.observability.emit_or_warn(
                    "deliver",
                    "rejected",
                    "notification runtime queue is full",
                );
                Err(AtmError::daemon_unavailable(
                    "notification runtime queue is full; delivery is backpressured",
                )
                .with_recovery(
                    "Wait for the notification worker lane to drain or restart atm-daemon if the queue remains saturated.",
                ))
            }
            Err(TrySendError::Disconnected(_)) => {
                let latest_status = self.inner.status_snapshot();
                if let Some(message) = &latest_status.degraded_message {
                    return Err(
                        AtmError::daemon_unavailable(message.as_ref()).with_recovery(
                            "Restart atm-daemon; the notification persistence lane is degraded.",
                        ),
                    );
                }
                if latest_status.shutdown_requested {
                    return Err(AtmError::daemon_unavailable(
                        "notification runtime is unavailable during daemon shutdown",
                    )
                    .with_recovery(
                        "Wait for atm-daemon to finish shutting down or restart it before retrying notification delivery.",
                    ));
                }
                Err(AtmError::daemon_unavailable(
                    "notification runtime command channel is unavailable",
                )
                .with_recovery(
                    "Restart atm-daemon; the notification worker lane is no longer receiving delivery commands.",
                ))
            }
        }
    }

    pub(crate) fn worker_liveness(&self) -> NotificationWorkerLiveness {
        #[cfg(test)]
        if let Some(override_liveness) = self.inner.liveness_override() {
            return override_liveness;
        }
        self.inner.status_snapshot().worker_liveness
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_path(path: PathBuf, queue_capacity: usize) -> Self {
        Self::new_with_path_factory(
            Arc::new(move || Ok(path.clone())),
            queue_capacity,
            SubsystemObservability::disabled(DaemonSubsystem::NotificationRuntime),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_path_factory_and_deadline(
        path_factory: NotificationPathFactory,
        queue_capacity: usize,
        shutdown_deadline: Duration,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::sync_channel(queue_capacity);
        Self {
            inner: Arc::new(NotificationRuntimeInner {
                command_tx,
                command_rx: Mutex::new(Some(command_rx)),
                queue_capacity,
                status: Arc::new(ArcSwap::from_pointee(NotificationRuntimeStatus::default())),
                worker: Arc::new(JoinHandleOwner::default()),
                path_factory,
                shutdown_deadline,
                observability: SubsystemObservability::disabled(
                    DaemonSubsystem::NotificationRuntime,
                ),
                start_claimed: AtomicBool::new(false),
                #[cfg(test)]
                liveness_override: AtomicU8::new(0),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_liveness_override_for_test(
        &self,
        liveness: Option<NotificationWorkerLiveness>,
    ) {
        self.inner
            .liveness_override
            .store(encode_liveness_override(liveness), Ordering::Release);
    }
}

impl NotificationRuntime {
    fn take_worker_for_shutdown(&self) -> Result<Option<JoinHandle<()>>, AtmError> {
        self.inner.mark_shutdown_requested();
        self.inner.worker.take()
    }

    fn await_worker_shutdown(&self, handle: JoinHandle<()>) -> Result<(), AtmError> {
        let worker_thread_id = handle.thread().id();
        let (join_helper, result_rx) = spawn_shutdown_join_helper(handle)?;
        match result_rx.recv_timeout(self.inner.shutdown_deadline) {
            Ok(Ok(())) => {
                self.handle_shutdown_joined(join_helper);
                Ok(())
            }
            Ok(Err(_)) => self.handle_shutdown_panic(join_helper),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                self.handle_shutdown_timeout(join_helper, worker_thread_id)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.handle_shutdown_disconnect(join_helper, worker_thread_id)
            }
        }
    }

    fn handle_shutdown_joined(&self, join_helper: JoinHandle<()>) {
        let _ = join_helper.join();
        self.inner.mark_worker_stopped();
        self.inner.observability.emit_or_warn(
            "shutdown",
            "ok",
            "notification runtime worker shut down cleanly",
        );
    }

    fn handle_shutdown_panic(&self, join_helper: JoinHandle<()>) -> Result<(), AtmError> {
        let _ = join_helper.join();
        self.inner.mark_worker_degraded(
            "notification runtime worker panicked during shutdown".to_string(),
        );
        self.inner.observability.emit_or_warn(
            "shutdown",
            "failed",
            "notification runtime worker panicked during shutdown",
        );
        Err(AtmError::daemon_unavailable(
            "notification runtime worker panicked during shutdown",
        )
        .with_recovery(
            "Restart atm-daemon; the notification background lane crashed while shutting down.",
        ))
    }

    fn handle_shutdown_timeout(
        &self,
        join_helper: JoinHandle<()>,
        worker_thread_id: thread::ThreadId,
    ) -> Result<(), AtmError> {
        let timeout_elapsed = self
            .inner
            .status_snapshot()
            .shutdown_started_at
            .map(|started_at| started_at.elapsed())
            .unwrap_or(self.inner.shutdown_deadline);
        self.inner.observability.emit_or_warn(
            "shutdown",
            "degraded",
            "notification runtime worker exceeded its shutdown deadline",
        );
        tracing::warn!(
            subsystem = "notification",
            action = "shutdown_retain",
            outcome = "deadline_exceeded",
            thread_id = ?worker_thread_id,
            timeout_ms = timeout_elapsed.as_millis(),
            "notification runtime worker exceeded shutdown deadline; retaining join helper for later cleanup"
        );
        retain_join_helper("notification_runtime_worker", join_helper, timeout_elapsed);
        Err(AtmError::daemon_unavailable(format!(
            "notification runtime shutdown exceeded the {:?} deadline",
            self.inner.shutdown_deadline
        ))
        .with_recovery(
            "Restart atm-daemon after the notification background lane becomes responsive again.",
        ))
    }

    fn handle_shutdown_disconnect(
        &self,
        join_helper: JoinHandle<()>,
        worker_thread_id: thread::ThreadId,
    ) -> Result<(), AtmError> {
        let _ = join_helper.join();
        self.inner.mark_worker_degraded(
            "notification runtime join helper disconnected during shutdown".to_string(),
        );
        self.inner.observability.emit_or_warn(
            "shutdown",
            "failed",
            "notification runtime join helper disconnected during shutdown",
        );
        tracing::warn!(
            subsystem = "notification",
            action = "shutdown_join_helper",
            outcome = "disconnected",
            thread_id = ?worker_thread_id,
            "notification runtime join helper exited before reporting shutdown status"
        );
        Err(AtmError::daemon_unavailable(
            "notification runtime join helper disconnected during shutdown",
        )
        .with_recovery(
            "Restart atm-daemon; the notification background lane did not report bounded shutdown status cleanly.",
        ))
    }
}

impl NotificationRuntimeInner {
    fn take_command_rx(&self) -> Result<Option<Receiver<NotificationCommand>>, AtmError> {
        let mut receiver = self.command_rx.lock().map_err(|_| {
            AtmError::daemon_unavailable("notification runtime receiver lock poisoned")
                .with_recovery(
                    "Restart atm-daemon; notification command receiver ownership can no longer be trusted.",
                )
        })?;
        Ok(receiver.take())
    }

    fn status_snapshot(&self) -> Arc<NotificationRuntimeStatus> {
        self.status.load_full()
    }

    fn publish_status(
        &self,
        mutate: impl FnOnce(&NotificationRuntimeStatus) -> NotificationRuntimeStatus,
    ) {
        let current = self.status.load_full();
        self.status.store(Arc::new(mutate(current.as_ref())));
    }

    fn mark_started(&self) {
        self.status.store(Arc::new(NotificationRuntimeStatus {
            started: true,
            shutdown_requested: false,
            shutdown_started_at: None,
            degraded_message: None,
            worker_liveness: NotificationWorkerLiveness::Live,
        }));
    }

    fn mark_shutdown_requested(&self) {
        self.publish_status(|status| NotificationRuntimeStatus {
            shutdown_requested: true,
            shutdown_started_at: status.shutdown_started_at.or(Some(Instant::now())),
            ..status.clone()
        });
    }

    fn mark_worker_stopped(&self) {
        self.publish_status(|status| NotificationRuntimeStatus {
            worker_liveness: NotificationWorkerLiveness::Stopped,
            ..status.clone()
        });
    }

    fn mark_worker_degraded(&self, message: String) {
        self.publish_status(|status| NotificationRuntimeStatus {
            degraded_message: Some(Arc::<str>::from(message)),
            worker_liveness: NotificationWorkerLiveness::Degraded,
            ..status.clone()
        });
    }

    #[cfg(test)]
    fn liveness_override(&self) -> Option<NotificationWorkerLiveness> {
        decode_liveness_override(self.liveness_override.load(Ordering::Acquire))
    }
}

fn spawn_shutdown_join_helper(
    handle: JoinHandle<()>,
) -> Result<
    (
        JoinHandle<()>,
        std::sync::mpsc::Receiver<std::thread::Result<()>>,
    ),
    AtmError,
> {
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let join_helper = std::thread::Builder::new()
        .name("atm-daemon-notifier-join".to_string())
        .spawn(move || {
            let _ = result_tx.send(handle.join());
        })
        .map_err(|source| {
            AtmError::daemon_unavailable(
                "failed to spawn notification runtime join helper during shutdown",
            )
            .with_recovery(
                "Restart atm-daemon; notification shutdown could not create its bounded join helper.",
            )
            .with_source(source)
        })?;
    Ok((join_helper, result_rx))
}

fn notification_worker_loop(
    inner: Arc<NotificationRuntimeInner>,
    command_rx: Receiver<NotificationCommand>,
) {
    loop {
        let status = inner.status_snapshot();
        if status.shutdown_requested {
            if shutdown_drain_deadline_exceeded(&inner, status.shutdown_started_at) {
                let dropped_events = drain_notification_commands(&command_rx);
                inner.observability.emit_or_warn(
                    "shutdown",
                    "degraded",
                    "notification runtime dropped queued events after drain deadline",
                );
                tracing::warn!(
                    subsystem = "notification",
                    action = "drain_shutdown",
                    outcome = "deadline_exceeded",
                    dropped_events,
                    timeout_ms = inner.shutdown_deadline.as_millis(),
                    "notification runtime exceeded its bounded drain deadline during shutdown"
                );
                inner.mark_worker_stopped();
                return;
            }

            match command_rx.try_recv() {
                Ok(NotificationCommand::Deliver { event }) => {
                    if let Err(error) = persist_notification(&inner, &event) {
                        inner.mark_worker_degraded(error.message);
                        inner.observability.emit_or_warn(
                            "persist_notification",
                            "degraded",
                            "notification runtime persistence failed and the runtime entered a degraded state",
                        );
                        return;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty)
                | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    inner.mark_worker_stopped();
                    return;
                }
            }
            continue;
        }

        match command_rx.recv_timeout(DEFAULT_NOTIFICATION_IDLE_INTERVAL) {
            Ok(NotificationCommand::Deliver { event }) => {
                if let Err(error) = persist_notification(&inner, &event) {
                    inner.mark_worker_degraded(error.message);
                    inner.observability.emit_or_warn(
                        "persist_notification",
                        "degraded",
                        "notification runtime persistence failed and the runtime entered a degraded state",
                    );
                    return;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                inner.mark_worker_stopped();
                return;
            }
        }
    }
}

fn drain_notification_commands(command_rx: &Receiver<NotificationCommand>) -> usize {
    let mut dropped_events = 0;
    while let Ok(NotificationCommand::Deliver { .. }) = command_rx.try_recv() {
        dropped_events += 1;
    }
    dropped_events
}

fn shutdown_drain_deadline_exceeded(
    inner: &NotificationRuntimeInner,
    shutdown_started_at: Option<Instant>,
) -> bool {
    shutdown_started_at
        .is_some_and(|shutdown_started_at| shutdown_started_at.elapsed() >= inner.shutdown_deadline)
}

fn persist_notification(
    inner: &NotificationRuntimeInner,
    event: &NotificationEvent,
) -> Result<(), AtmError> {
    ensure_shutdown_budget_remaining(inner)?;
    let path = (inner.path_factory)()?;
    ensure_shutdown_budget_remaining(inner)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to create notification runtime directory at {}",
                parent.display()
            ))
            .with_source(source)
        })?;
    }
    let encoded = serde_json::to_vec(event).map_err(|source| {
        AtmError::daemon_unavailable("failed to encode notification event").with_source(source)
    })?;
    // The cap applies to the bytes that actually hit disk; checking after
    // encoding avoids undercounting escaping and framing overhead.
    if encoded.len() > MAX_NOTIFICATION_EVENT_BYTES {
        return Err(AtmError::daemon_unavailable(format!(
            "notification event payload exceeded {} bytes",
            MAX_NOTIFICATION_EVENT_BYTES
        ))
        .with_recovery(
            "Reduce notification payload size before retrying retained-runtime delivery.",
        ));
    }
    ensure_shutdown_budget_remaining(inner)?;
    // Notification persistence intentionally stays on a dedicated std::thread
    // worker with blocking file I/O rather than introducing Tokio into the
    // daemon runtime.
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to open notification runtime output at {}",
                path.display()
            ))
            .with_source(source)
        })?;
    // Accepted limitation: the worker re-checks shutdown budget before each
    // persistence step, but once a blocking write_all/flush call begins it
    // cannot be interrupted mid-write on this dedicated std::thread lane.
    file.write_all(&encoded)
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to write notification runtime output at {}",
                path.display()
            ))
            .with_source(source)
        })?;
    file.flush().map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to flush notification runtime output at {}",
            path.display()
        ))
        .with_source(source)
    })?;
    Ok(())
}

fn ensure_shutdown_budget_remaining(inner: &NotificationRuntimeInner) -> Result<(), AtmError> {
    let status = inner.status_snapshot();
    if let Some(shutdown_started_at) = status.shutdown_started_at
        && shutdown_started_at.elapsed() >= inner.shutdown_deadline
    {
        return Err(AtmError::daemon_unavailable(format!(
            "notification runtime shutdown exceeded the {:?} drain deadline",
            inner.shutdown_deadline
        ))
        .with_recovery(
            "Restart atm-daemon after the notification background lane becomes responsive again.",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn encode_liveness_override(liveness: Option<NotificationWorkerLiveness>) -> u8 {
    match liveness {
        None => 0,
        Some(NotificationWorkerLiveness::Live) => 1,
        Some(NotificationWorkerLiveness::Degraded) => 2,
        Some(NotificationWorkerLiveness::Stopped) => 3,
    }
}

#[cfg(test)]
fn decode_liveness_override(value: u8) -> Option<NotificationWorkerLiveness> {
    match value {
        0 => None,
        1 => Some(NotificationWorkerLiveness::Live),
        2 => Some(NotificationWorkerLiveness::Degraded),
        3 => Some(NotificationWorkerLiveness::Stopped),
        _ => None,
    }
}

#[cfg(test)]
#[path = "notification_runtime_tests.rs"]
mod tests;
