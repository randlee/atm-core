use atm_core::error::AtmError;
use atm_core::protocol::NotificationEvent;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(test)]
use crate::DaemonSubsystem;
use crate::SubsystemObservability;

const DEFAULT_NOTIFICATION_QUEUE_CAPACITY: usize = 64;
const DEFAULT_NOTIFICATION_IDLE_INTERVAL: Duration = Duration::from_millis(50);
const NOTIFICATION_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);
const MAX_NOTIFICATION_EVENT_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct NotificationRuntime {
    inner: Arc<NotificationRuntimeInner>,
}

type NotificationPathFactory = Arc<dyn Fn() -> Result<PathBuf, AtmError> + Send + Sync>;

struct NotificationRuntimeInner {
    // State transitions, degradation, and the bounded queue must be updated
    // atomically with respect to wakeups, so the queue and lifecycle flags live
    // behind one mutex paired with the condvar below.
    state: Mutex<NotificationState>,
    wake: Condvar,
    path_factory: NotificationPathFactory,
    queue_capacity: usize,
    shutdown_deadline: Duration,
    observability: SubsystemObservability,
}

#[derive(Default)]
struct NotificationState {
    started: bool,
    shutdown: bool,
    shutdown_started_at: Option<Instant>,
    degraded_message: Option<String>,
    queue: VecDeque<NotificationEvent>,
    worker: Option<JoinHandle<()>>,
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
        Self {
            inner: Arc::new(NotificationRuntimeInner {
                state: Mutex::new(NotificationState::default()),
                wake: Condvar::new(),
                path_factory,
                queue_capacity,
                shutdown_deadline: NOTIFICATION_SHUTDOWN_DEADLINE,
                observability,
            }),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        let mut state = self.inner.state.lock().map_err(|_| {
            AtmError::daemon_unavailable("notification runtime state lock poisoned").with_recovery(
                "Restart the daemon; notification lifecycle state can no longer be trusted.",
            )
        })?;
        if state.started {
            return Ok(());
        }
        state.started = true;
        state.shutdown = false;
        state.shutdown_started_at = None;
        drop(state);

        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("atm-daemon-notifier".to_string())
            .spawn(move || notification_worker_loop(inner))
            .map_err(|source| {
                self.inner.observability.emit_or_warn(
                    "start",
                    "failed",
                    "failed to spawn notification runtime worker",
                );
                AtmError::daemon_unavailable("failed to spawn notification runtime worker")
                    .with_source(source)
            })?;
        let mut state = match self.inner.state.lock() {
            Ok(state) => state,
            Err(_) => {
                let _ = handle.join();
                return Err(AtmError::daemon_unavailable(
                    "notification runtime state lock poisoned",
                )
                .with_recovery(
                    "Restart the daemon; notification lifecycle state can no longer be trusted.",
                ));
            }
        };
        state.worker = Some(handle);
        self.inner
            .observability
            .emit_or_warn("start", "ok", "notification runtime worker started");
        Ok(())
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        let Some(handle) = self.take_worker_for_shutdown()? else {
            return Ok(());
        };
        self.await_worker_shutdown(handle)
    }

    pub(crate) fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError> {
        let mut state = self.inner.state.lock().map_err(|_| {
            AtmError::daemon_unavailable("notification runtime state lock poisoned").with_recovery(
                "Restart the daemon; notification lifecycle state can no longer be trusted.",
            )
        })?;
        if !state.started {
            return Err(AtmError::daemon_unavailable(
                "notification runtime is unavailable before daemon startup",
            ));
        }
        if state.shutdown {
            return Err(AtmError::daemon_unavailable(
                "notification runtime is unavailable during daemon shutdown",
            ));
        }
        if let Some(message) = &state.degraded_message {
            self.inner.observability.emit_or_warn(
                "deliver",
                "degraded",
                "notification runtime is degraded and rejecting delivery",
            );
            return Err(AtmError::daemon_unavailable(message.as_str()));
        }
        if state.queue.len() >= self.inner.queue_capacity {
            self.inner.observability.emit_or_warn(
                "deliver",
                "rejected",
                "notification runtime queue is full",
            );
            return Err(AtmError::daemon_unavailable(
                "notification runtime queue is full; delivery is backpressured",
            ));
        }
        state.queue.push_back(event);
        self.inner.wake.notify_one();
        Ok(())
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
        Self {
            inner: Arc::new(NotificationRuntimeInner {
                state: Mutex::new(NotificationState::default()),
                wake: Condvar::new(),
                path_factory,
                queue_capacity,
                shutdown_deadline,
                observability: SubsystemObservability::disabled(
                    DaemonSubsystem::NotificationRuntime,
                ),
            }),
        }
    }
}

impl NotificationRuntime {
    fn take_worker_for_shutdown(&self) -> Result<Option<JoinHandle<()>>, AtmError> {
        let mut state = self.inner.state.lock().map_err(|_| {
            AtmError::daemon_unavailable("notification runtime state lock poisoned").with_recovery(
                "Restart the daemon; notification lifecycle state can no longer be trusted.",
            )
        })?;
        state.shutdown = true;
        state.shutdown_started_at.get_or_insert_with(Instant::now);
        self.inner.wake.notify_all();
        Ok(state.worker.take())
    }

    fn await_worker_shutdown(&self, handle: JoinHandle<()>) -> Result<(), AtmError> {
        let worker_thread_id = handle.thread().id();
        let (join_helper, result_rx) = spawn_shutdown_join_helper(handle)?;
        match result_rx.recv_timeout(self.inner.shutdown_deadline) {
            Ok(Ok(())) => {
                let _ = join_helper.join();
                self.inner.observability.emit_or_warn(
                    "shutdown",
                    "ok",
                    "notification runtime worker shut down cleanly",
                );
                Ok(())
            }
            Ok(Err(_)) => {
                let _ = join_helper.join();
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
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                self.inner.observability.emit_or_warn(
                    "shutdown",
                    "degraded",
                    "notification runtime worker exceeded its shutdown deadline",
                );
                tracing::warn!(
                    subsystem = "notification",
                    action = "shutdown_detach",
                    outcome = "deadline_exceeded",
                    thread_id = ?worker_thread_id,
                    timeout_ms = self.inner.shutdown_deadline.as_millis(),
                    "notification runtime worker exceeded shutdown deadline; detaching join helper"
                );
                drop(join_helper);
                Err(AtmError::daemon_unavailable(format!(
                    "notification runtime shutdown exceeded the {:?} deadline",
                    self.inner.shutdown_deadline
                ))
                .with_recovery(
                    "Restart atm-daemon after the notification background lane becomes responsive again.",
                ))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = join_helper.join();
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

fn notification_worker_loop(inner: Arc<NotificationRuntimeInner>) {
    loop {
        let (event, shutdown_started_at) = {
            let mut state = match inner.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            while state.queue.is_empty() && !state.shutdown {
                let wait = match inner
                    .wake
                    .wait_timeout(state, DEFAULT_NOTIFICATION_IDLE_INTERVAL)
                {
                    Ok(wait) => wait,
                    Err(_) => return,
                };
                state = wait.0;
            }
            if state.shutdown && state.queue.is_empty() {
                return;
            }
            if let Some(shutdown_started_at) = state.shutdown_started_at
                && shutdown_started_at.elapsed() >= inner.shutdown_deadline
            {
                let dropped_events = state.queue.len();
                state.queue.clear();
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
                return;
            }
            (state.queue.pop_front(), state.shutdown_started_at)
        };

        let Some(event) = event else {
            continue;
        };
        if let Err(error) = persist_notification(&inner, &event, shutdown_started_at) {
            if let Ok(mut state) = inner.state.lock() {
                state.degraded_message = Some(error.message);
                state.queue.clear();
            }
            inner.observability.emit_or_warn(
                "persist_notification",
                "degraded",
                "notification runtime persistence failed and the runtime entered a degraded state",
            );
            return;
        }
    }
}

fn persist_notification(
    inner: &NotificationRuntimeInner,
    event: &NotificationEvent,
    shutdown_started_at: Option<Instant>,
) -> Result<(), AtmError> {
    ensure_shutdown_budget_remaining(inner, shutdown_started_at)?;
    let path = (inner.path_factory)()?;
    ensure_shutdown_budget_remaining(inner, shutdown_started_at)?;
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
    if encoded.len() > MAX_NOTIFICATION_EVENT_BYTES {
        return Err(AtmError::daemon_unavailable(format!(
            "notification event payload exceeded {} bytes",
            MAX_NOTIFICATION_EVENT_BYTES
        ))
        .with_recovery(
            "Reduce notification payload size before retrying retained-runtime delivery.",
        ));
    }
    ensure_shutdown_budget_remaining(inner, shutdown_started_at)?;
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
    // TODO(Y.14): std::fs append writes are still synchronous once they start.
    // Y.13 bounds queue drain before each persistence step, but it cannot
    // preempt a single blocked write_all/flush call mid-flight.
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

fn ensure_shutdown_budget_remaining(
    inner: &NotificationRuntimeInner,
    shutdown_started_at: Option<Instant>,
) -> Result<(), AtmError> {
    if let Some(shutdown_started_at) = shutdown_started_at
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
mod tests {
    use super::NotificationRuntime;
    use atm_core::protocol::{NotificationEvent, NotificationKind};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn notification_runtime_persists_events_to_runtime_output() {
        let tempdir = TempDir::new().expect("tempdir");
        let output_path = tempdir.path().join("notifications.jsonl");
        let runtime = NotificationRuntime::new_for_test_with_path(output_path.clone(), 8);
        runtime.start().expect("start");
        runtime
            .deliver(NotificationEvent {
                kind: NotificationKind::Delivery,
                detail: "message delivered".to_string(),
                team: None,
                agent: None,
            })
            .expect("deliver");
        runtime
            .shutdown()
            .unwrap_or_else(|error| panic!("shutdown failed: {error}"));

        let output = std::fs::read_to_string(output_path).expect("output");
        assert!(output.contains("\"kind\":\"delivery\""));
        assert!(output.contains("\"detail\":\"message delivered\""));
    }

    #[test]
    fn notification_runtime_reports_degraded_output_failures() {
        let tempdir = TempDir::new().expect("tempdir");
        let blocking_path = tempdir.path().join("blocking-file");
        std::fs::write(&blocking_path, "not-a-dir").expect("blocking file");
        let output_path = blocking_path.join("notifications.jsonl");
        let runtime = NotificationRuntime::new_for_test_with_path(output_path, 8);
        runtime.start().expect("start");
        runtime
            .deliver(NotificationEvent {
                kind: NotificationKind::Delivery,
                detail: "message delivered".to_string(),
                team: None,
                agent: None,
            })
            .expect("first deliver queues");
        runtime
            .shutdown()
            .unwrap_or_else(|error| panic!("shutdown failed: {error}"));

        let error = runtime
            .deliver(NotificationEvent {
                kind: NotificationKind::Delivery,
                detail: "message delivered".to_string(),
                team: None,
                agent: None,
            })
            .expect_err("degraded");
        assert!(
            error
                .message
                .contains("notification runtime is unavailable")
        );
    }

    #[test]
    fn notification_runtime_shutdown_times_out_when_persistence_stalls() {
        let tempdir = TempDir::new().expect("tempdir");
        let output_path = tempdir.path().join("notifications.jsonl");
        let entered_gate = Arc::new((Mutex::new(false), Condvar::new()));
        let release_gate = Arc::new((Mutex::new(false), Condvar::new()));
        let runtime = NotificationRuntime::new_for_test_with_path_factory_and_deadline(
            Arc::new({
                let entered_gate = Arc::clone(&entered_gate);
                let release_gate = Arc::clone(&release_gate);
                move || {
                    let (entered_lock, entered_wake) = &*entered_gate;
                    let mut entered = entered_lock.lock().expect("entered gate lock");
                    *entered = true;
                    entered_wake.notify_all();
                    drop(entered);

                    let (release_lock, release_wake) = &*release_gate;
                    let mut released = release_lock.lock().expect("release gate lock");
                    while !*released {
                        released = release_wake.wait(released).expect("release gate wait");
                    }
                    drop(released);

                    Ok(output_path.clone())
                }
            }),
            8,
            Duration::from_millis(25),
        );
        runtime.start().expect("start");
        runtime
            .deliver(NotificationEvent {
                kind: NotificationKind::Delivery,
                detail: "message delivered".to_string(),
                team: None,
                agent: None,
            })
            .expect("deliver");

        {
            let (entered_lock, entered_wake) = &*entered_gate;
            let entered = entered_lock.lock().expect("entered gate lock");
            let (_entered_guard, wait_result) = entered_wake
                .wait_timeout_while(entered, Duration::from_secs(1), |entered| !*entered)
                .expect("entered gate wait");
            assert!(
                !wait_result.timed_out(),
                "worker never entered path factory"
            );
        }

        let error = runtime.shutdown().expect_err("shutdown should time out");
        {
            let (release_lock, release_wake) = &*release_gate;
            let mut released = release_lock.lock().expect("release gate lock");
            *released = true;
            release_wake.notify_all();
        }
        assert!(
            error
                .message
                .contains("notification runtime shutdown exceeded")
        );
    }
}
