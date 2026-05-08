use atm_core::error::AtmError;
use atm_core::protocol::NotificationEvent;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAULT_NOTIFICATION_QUEUE_CAPACITY: usize = 64;
const DEFAULT_NOTIFICATION_IDLE_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone)]
pub(crate) struct NotificationRuntime {
    inner: Arc<NotificationRuntimeInner>,
}

type NotificationPathFactory = Arc<dyn Fn() -> Result<PathBuf, AtmError> + Send + Sync>;

struct NotificationRuntimeInner {
    state: Mutex<NotificationState>,
    wake: Condvar,
    worker: Mutex<Option<JoinHandle<()>>>,
    path_factory: NotificationPathFactory,
    queue_capacity: usize,
}

#[derive(Default)]
struct NotificationState {
    started: bool,
    shutdown: bool,
    degraded_message: Option<String>,
    queue: VecDeque<NotificationEvent>,
}

impl NotificationRuntime {
    pub(crate) fn new() -> Self {
        Self::new_with_path_factory(
            Arc::new(|| Ok(atm_core::home::host_runtime_dir()?.join("notifications.jsonl"))),
            DEFAULT_NOTIFICATION_QUEUE_CAPACITY,
        )
    }

    fn new_with_path_factory(path_factory: NotificationPathFactory, queue_capacity: usize) -> Self {
        Self {
            inner: Arc::new(NotificationRuntimeInner {
                state: Mutex::new(NotificationState::default()),
                wake: Condvar::new(),
                worker: Mutex::new(None),
                path_factory,
                queue_capacity,
            }),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        let mut state = self.inner.state.lock().map_err(|_| {
            AtmError::daemon_unavailable("notification runtime state lock poisoned")
        })?;
        if state.started {
            return Ok(());
        }
        state.started = true;
        state.shutdown = false;
        drop(state);

        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("atm-daemon-notifier".to_string())
            .spawn(move || notification_worker_loop(inner))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to spawn notification runtime worker")
                    .with_source(source)
            })?;
        *self.inner.worker.lock().map_err(|_| {
            AtmError::daemon_unavailable("notification runtime worker lock poisoned")
        })? = Some(handle);
        Ok(())
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        {
            let mut state = self.inner.state.lock().map_err(|_| {
                AtmError::daemon_unavailable("notification runtime state lock poisoned")
            })?;
            state.shutdown = true;
            self.inner.wake.notify_all();
        }
        if let Some(handle) = self
            .inner
            .worker
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("notification runtime worker lock poisoned"))?
            .take()
        {
            handle.join().map_err(|_| {
                AtmError::daemon_unavailable("notification runtime worker panicked during shutdown")
            })?;
        }
        Ok(())
    }

    pub(crate) fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError> {
        let mut state = self.inner.state.lock().map_err(|_| {
            AtmError::daemon_unavailable("notification runtime state lock poisoned")
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
            return Err(AtmError::daemon_unavailable(message.clone()));
        }
        if state.queue.len() >= self.inner.queue_capacity {
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
        Self::new_with_path_factory(Arc::new(move || Ok(path.clone())), queue_capacity)
    }
}

fn notification_worker_loop(inner: Arc<NotificationRuntimeInner>) {
    loop {
        let event = {
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
            state.queue.pop_front()
        };

        let Some(event) = event else {
            continue;
        };
        if let Err(error) = persist_notification(&inner, &event) {
            if let Ok(mut state) = inner.state.lock() {
                state.degraded_message = Some(error.message);
                state.queue.clear();
            }
            return;
        }
    }
}

fn persist_notification(
    inner: &NotificationRuntimeInner,
    event: &NotificationEvent,
) -> Result<(), AtmError> {
    let path = (inner.path_factory)()?;
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

#[cfg(test)]
mod tests {
    use super::NotificationRuntime;
    use atm_core::protocol::NotificationEvent;
    use tempfile::TempDir;

    #[test]
    fn notification_runtime_persists_events_to_runtime_output() {
        let tempdir = TempDir::new().expect("tempdir");
        let output_path = tempdir.path().join("notifications.jsonl");
        let runtime = NotificationRuntime::new_for_test_with_path(output_path.clone(), 8);
        runtime.start().expect("start");
        runtime
            .deliver(NotificationEvent {
                kind: "delivery".to_string(),
                detail: "message delivered".to_string(),
                team: None,
                agent: None,
            })
            .expect("deliver");
        runtime.shutdown().expect("shutdown");

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
                kind: "delivery".to_string(),
                detail: "message delivered".to_string(),
                team: None,
                agent: None,
            })
            .expect("first deliver queues");
        runtime.shutdown().expect("shutdown");

        let error = runtime
            .deliver(NotificationEvent {
                kind: "delivery".to_string(),
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
}
