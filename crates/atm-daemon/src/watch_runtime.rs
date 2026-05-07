use atm_core::boundary::{WatchEventBatch, WatchSubscriptionRequest};
use atm_core::error::AtmError;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAULT_WATCH_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone)]
pub(crate) struct WatchRuntime {
    inner: Arc<WatchRuntimeInner>,
}

type WatchPoller =
    Arc<dyn Fn(&WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError> + Send + Sync>;

struct WatchRuntimeInner {
    state: Mutex<WatchState>,
    wake: Condvar,
    worker: Mutex<Option<JoinHandle<()>>>,
    poller: WatchPoller,
    poll_interval: Duration,
}

#[derive(Default)]
struct WatchState {
    started: bool,
    shutdown: bool,
    subscriptions: HashMap<WatchKey, WatchSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WatchKey {
    home_dir: PathBuf,
    team: String,
    agent: String,
}

#[derive(Clone)]
struct WatchSnapshot {
    request: WatchSubscriptionRequest,
    batch: WatchEventBatch,
    error_message: Option<String>,
}

impl WatchKey {
    fn from_request(request: &WatchSubscriptionRequest) -> Self {
        Self {
            home_dir: request.home_dir.clone(),
            team: request.team.to_string(),
            agent: request.agent.to_string(),
        }
    }
}

impl WatchRuntime {
    pub(crate) fn new() -> Self {
        Self::new_with_poller(
            Arc::new(|request| {
                let inbox_path = atm_core::home::inbox_path_from_home(
                    &request.home_dir,
                    request.team.as_str(),
                    request.agent.as_str(),
                )?;
                let mut paths = Vec::new();
                if inbox_path.exists() {
                    paths.push(inbox_path.clone());
                }
                let inboxes_dir = inbox_path.parent().ok_or_else(|| {
                    AtmError::daemon_unavailable("watch runtime inbox path has no parent directory")
                })?;
                let prefix = format!("{}.", request.agent.as_str());
                let primary = format!("{}.json", request.agent.as_str());
                if inboxes_dir.exists() {
                    for entry in fs::read_dir(inboxes_dir).map_err(|source| {
                        AtmError::daemon_unavailable(format!(
                            "failed to enumerate watch runtime inbox directory {}",
                            inboxes_dir.display()
                        ))
                        .with_source(source)
                    })? {
                        let path = entry
                            .map_err(|source| {
                                AtmError::daemon_unavailable(format!(
                                    "failed to read one watch runtime inbox entry in {}",
                                    inboxes_dir.display()
                                ))
                                .with_source(source)
                            })?
                            .path();
                        if path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .map(|name| {
                                name.starts_with(&prefix)
                                    && name.ends_with(".json")
                                    && name != primary
                            })
                            .unwrap_or(false)
                        {
                            paths.push(path);
                        }
                    }
                }
                paths.sort_by_key(|path| path.to_string_lossy().into_owned());
                paths.dedup();
                Ok(WatchEventBatch { paths })
            }),
            DEFAULT_WATCH_POLL_INTERVAL,
        )
    }

    fn new_with_poller(poller: WatchPoller, poll_interval: Duration) -> Self {
        Self {
            inner: Arc::new(WatchRuntimeInner {
                state: Mutex::new(WatchState::default()),
                wake: Condvar::new(),
                worker: Mutex::new(None),
                poller,
                poll_interval,
            }),
        }
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("watch runtime state lock poisoned"))?;
        if state.started {
            return Ok(());
        }
        state.started = true;
        state.shutdown = false;
        drop(state);

        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("atm-daemon-watch".to_string())
            .spawn(move || watch_worker_loop(inner))
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to spawn watch runtime worker")
                    .with_source(source)
            })?;
        *self
            .inner
            .worker
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("watch runtime worker lock poisoned"))? =
            Some(handle);
        Ok(())
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        {
            let mut state =
                self.inner.state.lock().map_err(|_| {
                    AtmError::daemon_unavailable("watch runtime state lock poisoned")
                })?;
            state.shutdown = true;
            self.inner.wake.notify_all();
        }
        if let Some(handle) = self
            .inner
            .worker
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("watch runtime worker lock poisoned"))?
            .take()
        {
            handle.join().map_err(|_| {
                AtmError::daemon_unavailable("watch runtime worker panicked during shutdown")
            })?;
        }
        Ok(())
    }

    pub(crate) fn poll(
        &self,
        request: WatchSubscriptionRequest,
    ) -> Result<WatchEventBatch, AtmError> {
        let batch = (self.inner.poller)(&request)?;
        let key = WatchKey::from_request(&request);
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("watch runtime state lock poisoned"))?;
        state.subscriptions.insert(
            key,
            WatchSnapshot {
                request,
                batch: batch.clone(),
                error_message: None,
            },
        );
        self.inner.wake.notify_one();
        Ok(batch)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(poller: WatchPoller, poll_interval: Duration) -> Self {
        Self::new_with_poller(poller, poll_interval)
    }
}

fn watch_worker_loop(inner: Arc<WatchRuntimeInner>) {
    loop {
        let subscriptions = {
            let mut state = match inner.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            while state.subscriptions.is_empty() && !state.shutdown {
                state = match inner.wake.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
            if state.shutdown {
                return;
            }
            state
                .subscriptions
                .values()
                .map(|entry| entry.request.clone())
                .collect::<Vec<_>>()
        };

        for request in subscriptions {
            let key = WatchKey::from_request(&request);
            let result = (inner.poller)(&request);
            if let Ok(mut state) = inner.state.lock() {
                if let Some(entry) = state.subscriptions.get_mut(&key) {
                    match result {
                        Ok(batch) => {
                            entry.batch = batch;
                            entry.error_message = None;
                        }
                        Err(error) => {
                            entry.error_message = Some(error.message);
                        }
                    }
                }
            } else {
                return;
            }
        }

        let state = match inner.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        let _ = inner.wake.wait_timeout(state, inner.poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::WatchRuntime;
    use atm_core::boundary::WatchSubscriptionRequest;
    use atm_core::error::AtmError;
    use atm_core::protocol::WatchEventBatch;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn request() -> WatchSubscriptionRequest {
        WatchSubscriptionRequest {
            home_dir: PathBuf::from("/tmp/atm-watch-test"),
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
        }
    }

    #[test]
    fn watch_runtime_returns_updated_batches_from_runtime_poller() {
        let batches = Arc::new(Mutex::new(vec![
            WatchEventBatch {
                paths: vec![PathBuf::from("one.jsonl")],
            },
            WatchEventBatch {
                paths: vec![PathBuf::from("two.jsonl")],
            },
        ]));
        let runtime = WatchRuntime::new_for_test(
            Arc::new({
                let batches = Arc::clone(&batches);
                move |_| {
                    let mut batches = batches.lock().expect("batches");
                    Ok(if batches.len() > 1 {
                        batches.remove(0)
                    } else {
                        batches[0].clone()
                    })
                }
            }),
            Duration::from_millis(10),
        );
        runtime.start().expect("start");

        let first = runtime.poll(request()).expect("first poll");
        assert_eq!(first.paths, vec![PathBuf::from("one.jsonl")]);

        std::thread::sleep(Duration::from_millis(30));

        let second = runtime.poll(request()).expect("second poll");
        assert_eq!(second.paths, vec![PathBuf::from("two.jsonl")]);
        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn watch_runtime_reports_degradation_from_background_failures() {
        let fail = Arc::new(Mutex::new(false));
        let runtime = WatchRuntime::new_for_test(
            Arc::new({
                let fail = Arc::clone(&fail);
                move |_| {
                    if *fail.lock().expect("flag") {
                        Err(AtmError::daemon_unavailable("watch poll failed"))
                    } else {
                        Ok(WatchEventBatch { paths: Vec::new() })
                    }
                }
            }),
            Duration::from_millis(10),
        );
        runtime.start().expect("start");
        runtime.poll(request()).expect("initial poll");
        *fail.lock().expect("flag") = true;
        std::thread::sleep(Duration::from_millis(30));
        let error = runtime.poll(request()).expect_err("degraded");
        assert!(error.message.contains("watch poll failed"));
        runtime.shutdown().expect("shutdown");
    }
}
