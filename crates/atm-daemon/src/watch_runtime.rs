use atm_core::boundary::{WatchEventBatch, WatchSubscriptionRequest};
use atm_core::error::AtmError;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAULT_WATCH_POLL_INTERVAL: Duration = Duration::from_millis(50);
const MAX_WATCH_SUBSCRIPTIONS: usize = 256;
const WATCH_POLL_HEALTH_TIMEOUT_MULTIPLIER: u32 = 5;

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
    team: atm_core::types::TeamName,
    agent: atm_core::types::AgentName,
}

#[derive(Clone)]
struct WatchSnapshot {
    request: WatchSubscriptionRequest,
    requested_revision: u64,
    observed_revision: u64,
    batch: WatchEventBatch,
    error: Option<WatchFailureSnapshot>,
}

#[derive(Clone)]
struct WatchRefreshTarget {
    key: WatchKey,
    request: WatchSubscriptionRequest,
    revision: u64,
}

#[derive(Clone)]
struct WatchFailureSnapshot {
    message: String,
    recovery: Option<String>,
}

impl From<AtmError> for WatchFailureSnapshot {
    fn from(error: AtmError) -> Self {
        Self {
            message: error.message,
            recovery: error.recovery,
        }
    }
}

impl WatchFailureSnapshot {
    fn to_error(&self) -> AtmError {
        let error = AtmError::daemon_unavailable(self.message.clone());
        match &self.recovery {
            Some(recovery) => error.with_recovery(recovery.clone()),
            None => error,
        }
    }
}

impl WatchKey {
    fn from_request(request: &WatchSubscriptionRequest) -> Self {
        Self {
            home_dir: request.home_dir.clone(),
            team: request.team.clone(),
            agent: request.agent.clone(),
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
        let key = WatchKey::from_request(&request);
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("watch runtime state lock poisoned"))?;
        if !state.started {
            return Err(AtmError::daemon_unavailable(
                "watch runtime is unavailable before daemon startup",
            ));
        }
        if state.shutdown {
            return Err(AtmError::daemon_unavailable(
                "watch runtime is unavailable during daemon shutdown",
            ));
        }
        let requested_revision = match state.subscriptions.get_mut(&key) {
            Some(entry) => {
                entry.request = request;
                entry.requested_revision = entry.requested_revision.saturating_add(1);
                entry.requested_revision
            }
            None => {
                if state.subscriptions.len() >= MAX_WATCH_SUBSCRIPTIONS {
                    return Err(
                        AtmError::daemon_unavailable(format!(
                            "watch runtime refused a new subscription because the bounded registry capacity of {MAX_WATCH_SUBSCRIPTIONS} entries was reached"
                        ))
                        .with_recovery(
                            "Reduce concurrent watch targets or restart atm-daemon so the bounded watch registry can be rebuilt from active callers.",
                        ),
                    );
                }
                state.subscriptions.insert(
                    key.clone(),
                    WatchSnapshot {
                        request,
                        requested_revision: 1,
                        observed_revision: 0,
                        batch: WatchEventBatch { paths: Vec::new() },
                        error: None,
                    },
                );
                1
            }
        };
        self.inner.wake.notify_one();
        loop {
            if let Some(entry) = state.subscriptions.get(&key)
                && entry.observed_revision >= requested_revision
            {
                return match &entry.error {
                    Some(error) => Err(error.to_error()),
                    None => Ok(entry.batch.clone()),
                };
            }
            if state.shutdown {
                return Err(AtmError::daemon_unavailable(
                    "watch runtime shut down before delivering an updated batch",
                ));
            }
            let wait_timeout = self
                .inner
                .poll_interval
                .saturating_mul(WATCH_POLL_HEALTH_TIMEOUT_MULTIPLIER);
            let wait = self
                .inner
                .wake
                .wait_timeout(state, wait_timeout)
                .map_err(|_| AtmError::daemon_unavailable("watch runtime state lock poisoned"))?;
            state = wait.0;
            if wait.1.timed_out() {
                return Err(
                    AtmError::daemon_unavailable(
                        "watch runtime did not deliver an updated batch before the worker health timeout elapsed",
                    )
                    .with_recovery(
                        "Restart atm-daemon if the watch worker is no longer making progress.",
                    ),
                );
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(poller: WatchPoller, poll_interval: Duration) -> Self {
        Self::new_with_poller(poller, poll_interval)
    }
}

fn watch_worker_loop(inner: Arc<WatchRuntimeInner>) {
    loop {
        let refresh_targets = {
            let mut state = match inner.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            loop {
                if state.shutdown {
                    return;
                }
                if state
                    .subscriptions
                    .values()
                    .any(|entry| entry.observed_revision < entry.requested_revision)
                {
                    break;
                }
                if state.subscriptions.is_empty() {
                    state = match inner.wake.wait(state) {
                        Ok(state) => state,
                        Err(_) => return,
                    };
                    continue;
                }
                let wait = match inner.wake.wait_timeout(state, inner.poll_interval) {
                    Ok(wait) => wait,
                    Err(_) => return,
                };
                state = wait.0;
                if wait.1.timed_out() {
                    for entry in state.subscriptions.values_mut() {
                        entry.requested_revision = entry.requested_revision.saturating_add(1);
                    }
                    break;
                }
            }
            state
                .subscriptions
                .iter()
                .filter(|(_, entry)| entry.observed_revision < entry.requested_revision)
                .map(|(key, entry)| WatchRefreshTarget {
                    key: key.clone(),
                    request: entry.request.clone(),
                    revision: entry.requested_revision,
                })
                .collect::<Vec<_>>()
        };

        for target in refresh_targets {
            let result = (inner.poller)(&target.request);
            if let Ok(mut state) = inner.state.lock() {
                if let Some(entry) = state.subscriptions.get_mut(&target.key) {
                    match result {
                        Ok(batch) => {
                            entry.batch = batch;
                            entry.error = None;
                        }
                        Err(error) => {
                            entry.error = Some(error.into());
                        }
                    }
                    entry.observed_revision = target.revision;
                    inner.wake.notify_all();
                }
            } else {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_WATCH_SUBSCRIPTIONS, WatchRuntime};
    use atm_core::boundary::WatchEventBatch;
    use atm_core::boundary::WatchSubscriptionRequest;
    use atm_core::error::AtmError;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn request() -> WatchSubscriptionRequest {
        request_for("test-agent")
    }

    fn request_for(agent: &str) -> WatchSubscriptionRequest {
        WatchSubscriptionRequest {
            home_dir: std::env::temp_dir().join("atm-watch-test"),
            team: "test-team".parse().expect("team"),
            agent: agent.parse().expect("agent"),
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
        let error = runtime.poll(request()).expect_err("degraded");
        assert!(error.message.contains("watch poll failed"));
        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn watch_runtime_times_out_when_the_worker_stops_making_progress() {
        let runtime = WatchRuntime::new_for_test(
            Arc::new(|_| {
                std::thread::sleep(Duration::from_millis(200));
                Ok(WatchEventBatch { paths: Vec::new() })
            }),
            Duration::from_millis(10),
        );
        runtime.start().expect("start");

        let error = runtime.poll(request()).expect_err("health timeout");
        assert!(error.message.contains("worker health timeout"));

        std::thread::sleep(Duration::from_millis(220));
        runtime.shutdown().expect("shutdown");
    }

    #[test]
    fn watch_runtime_rejects_subscriptions_beyond_the_bounded_capacity() {
        let runtime = WatchRuntime::new_for_test(
            Arc::new(|_| Ok(WatchEventBatch { paths: Vec::new() })),
            Duration::from_millis(10),
        );
        runtime.start().expect("start");

        for index in 0..MAX_WATCH_SUBSCRIPTIONS {
            runtime
                .poll(request_for(&format!("test-agent-{index}")))
                .expect("bounded subscription");
        }

        let error = runtime
            .poll(request_for("overflow-agent"))
            .expect_err("capacity overflow");
        assert!(error.message.contains("bounded registry capacity"));

        runtime.shutdown().expect("shutdown");
    }
}
