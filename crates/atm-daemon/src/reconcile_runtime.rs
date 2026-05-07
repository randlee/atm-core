use crate::boundary_adapters::{DaemonInboxIngress, DaemonNotificationSink, FileWatchEventSource};
use atm_core::boundary::{
    InboxIngress, NotificationSink, ReconcileRequest, ReconcileResult, WatchEventSource,
    WatchSubscriptionRequest,
};
use atm_core::error::AtmError;
use atm_core::protocol::NotificationEvent;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAULT_RECONCILE_DEBOUNCE: Duration = Duration::from_millis(25);

#[derive(Clone)]
pub(crate) struct ReconcileRuntime {
    inner: Arc<ReconcileRuntimeInner>,
}

type ReconcileExecutor =
    Arc<dyn Fn(&ReconcileRequest) -> Result<ReconcileResult, AtmError> + Send + Sync>;

struct ReconcileRuntimeInner {
    state: Mutex<ReconcileState>,
    wake: Condvar,
    worker: Mutex<Option<JoinHandle<()>>>,
    debounce: Duration,
    executor: ReconcileExecutor,
}

#[derive(Default)]
struct ReconcileState {
    started: bool,
    shutdown: bool,
    next_waiter_id: u64,
    pending: HashMap<ReconcileKey, PendingReconcile>,
    completed: HashMap<u64, ReconcileOutcome>,
}

#[derive(Clone)]
enum ReconcileOutcome {
    Success(ReconcileResult),
    Failure(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReconcileKey {
    home_dir: PathBuf,
    team: String,
    agent: String,
}

struct PendingReconcile {
    request: ReconcileRequest,
    waiters: Vec<u64>,
}

impl ReconcileKey {
    fn from_request(request: &ReconcileRequest) -> Self {
        Self {
            home_dir: request.home_dir.clone(),
            team: request.team.to_string(),
            agent: request.agent.to_string(),
        }
    }
}

impl ReconcileRuntime {
    pub(crate) fn new(
        watch_source: FileWatchEventSource,
        inbox_ingress: DaemonInboxIngress,
        notification_sink: DaemonNotificationSink,
    ) -> Self {
        Self::new_with_executor(
            Arc::new(move |request| {
                let batch = watch_source.poll(WatchSubscriptionRequest {
                    home_dir: request.home_dir.clone(),
                    team: request.team.clone(),
                    agent: request.agent.clone(),
                })?;
                let import = inbox_ingress.import_inbox_source(
                    atm_core::boundary::InboxIngressImportRequest {
                        home_dir: request.home_dir.clone(),
                        team: request.team.clone(),
                        agent: request.agent.clone(),
                    },
                )?;
                notification_sink.deliver(NotificationEvent {
                    kind: "reconcile_complete".to_string(),
                    detail: format!(
                        "observed_paths={} imported_sources={}",
                        batch.paths.len(),
                        import.source_files.len()
                    ),
                    team: Some(request.team.clone()),
                    agent: Some(request.agent.clone()),
                })?;
                Ok(ReconcileResult {
                    observed_paths: batch.paths.len(),
                    imported_sources: import.source_files.len(),
                })
            }),
            DEFAULT_RECONCILE_DEBOUNCE,
        )
    }

    fn new_with_executor(executor: ReconcileExecutor, debounce: Duration) -> Self {
        Self {
            inner: Arc::new(ReconcileRuntimeInner {
                state: Mutex::new(ReconcileState::default()),
                wake: Condvar::new(),
                worker: Mutex::new(None),
                debounce,
                executor,
            }),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        let mut state =
            self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
        if state.started {
            return Ok(());
        }
        state.started = true;
        state.shutdown = false;
        drop(state);

        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("atm-daemon-reconcile".to_string())
            .spawn(move || reconcile_worker_loop(inner))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to spawn reconcile runtime worker")
                    .with_source(source)
            })?;
        *self.inner.worker.lock().map_err(|_| {
            AtmError::daemon_unavailable("reconcile runtime worker lock poisoned")
        })? = Some(handle);
        Ok(())
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        {
            let mut state = self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
            state.shutdown = true;
            self.inner.wake.notify_all();
        }
        if let Some(handle) = self
            .inner
            .worker
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("reconcile runtime worker lock poisoned"))?
            .take()
        {
            handle.join().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime worker panicked during shutdown")
            })?;
        }
        Ok(())
    }

    pub(crate) fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileResult, AtmError> {
        let waiter_id = {
            let mut state = self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
            if !state.started {
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime is unavailable before daemon startup",
                ));
            }
            if state.shutdown {
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime is unavailable during daemon shutdown",
                ));
            }
            let waiter_id = state.next_waiter_id;
            state.next_waiter_id += 1;
            let key = ReconcileKey::from_request(&request);
            state
                .pending
                .entry(key)
                .and_modify(|pending| {
                    pending.request = request.clone();
                    pending.waiters.push(waiter_id);
                })
                .or_insert(PendingReconcile {
                    request,
                    waiters: vec![waiter_id],
                });
            self.inner.wake.notify_one();
            waiter_id
        };

        let mut state =
            self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
        loop {
            if let Some(outcome) = state.completed.remove(&waiter_id) {
                return match outcome {
                    ReconcileOutcome::Success(result) => Ok(result),
                    ReconcileOutcome::Failure(message) => {
                        Err(AtmError::daemon_unavailable(message))
                    }
                };
            }
            if state.shutdown {
                return Err(AtmError::daemon_unavailable(
                    "reconcile runtime shut down before completion",
                ));
            }
            state = self.inner.wake.wait(state).map_err(|_| {
                AtmError::daemon_unavailable("reconcile runtime state lock poisoned")
            })?;
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(executor: ReconcileExecutor, debounce: Duration) -> Self {
        Self::new_with_executor(executor, debounce)
    }
}

fn reconcile_worker_loop(inner: Arc<ReconcileRuntimeInner>) {
    loop {
        let pending = {
            let mut state = match inner.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            while state.pending.is_empty() && !state.shutdown {
                state = match inner.wake.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
            if state.shutdown {
                return;
            }
            drop(state);
            thread::sleep(inner.debounce);
            let mut state = match inner.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            std::mem::take(&mut state.pending)
        };

        for pending_request in pending.into_values() {
            let outcome = match (inner.executor)(&pending_request.request) {
                Ok(result) => ReconcileOutcome::Success(result),
                Err(error) => ReconcileOutcome::Failure(error.message),
            };
            if let Ok(mut state) = inner.state.lock() {
                for waiter in pending_request.waiters {
                    state.completed.insert(waiter, outcome.clone());
                }
                inner.wake.notify_all();
            } else {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReconcileRuntime;
    use atm_core::boundary::ReconcileRequest;
    use atm_core::protocol::ReconcileResult;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn request() -> ReconcileRequest {
        ReconcileRequest {
            home_dir: PathBuf::from("/tmp/atm-reconcile-test"),
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
        }
    }

    #[test]
    fn reconcile_runtime_coalesces_duplicate_requests() {
        let calls = Arc::new(Mutex::new(0usize));
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new({
                let calls = Arc::clone(&calls);
                move |_| {
                    *calls.lock().expect("calls") += 1;
                    Ok(ReconcileResult {
                        observed_paths: 2,
                        imported_sources: 1,
                    })
                }
            }),
            Duration::from_millis(20),
        );
        runtime.start().expect("start");

        let runtime_a = runtime.clone();
        let runtime_b = runtime.clone();
        let request_a = request();
        let request_b = request();
        let first = std::thread::spawn(move || runtime_a.reconcile(request_a).expect("first"));
        let second = std::thread::spawn(move || runtime_b.reconcile(request_b).expect("second"));
        assert_eq!(first.join().expect("join").observed_paths, 2);
        assert_eq!(second.join().expect("join").imported_sources, 1);
        assert_eq!(*calls.lock().expect("calls"), 1);
        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn reconcile_runtime_returns_executor_failures() {
        let runtime = ReconcileRuntime::new_for_test(
            Arc::new(|_| {
                Err(atm_core::error::AtmError::daemon_unavailable(
                    "reconcile failed",
                ))
            }),
            Duration::from_millis(10),
        );
        runtime.start().expect("start");
        let error = runtime.reconcile(request()).expect_err("failure");
        assert!(error.message.contains("reconcile failed"));
        runtime.shutdown().expect("shutdown");
    }
}
