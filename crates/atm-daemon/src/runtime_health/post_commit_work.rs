//! Identifier-only work that runs after local SQLite admission.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::{
    LocalServiceRuntime,
    boundary::{self, GraftNudgeTarget, PostSendHookEvent},
    error::{AtmError, AtmErrorCode},
    graft::{
        GraftPostSendRequest, GraftPostSendResponse, deliver_graft_post_send,
        graft_receiver_record_path_from_root,
    },
    schema::{AtmMessageId, canonical_graft_root},
};

use crate::AtmHomeDir;
use crate::daemon_runtime_observability::DaemonRuntimeObservability;
use crate::daemon_worker_join::{
    CompletionTrackedJoinHandle, JoinTimeoutPolicy, join_with_timeout,
};

const GRAFT_POST_SEND_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
const GRAFT_POST_SEND_IO_DEADLINE: Duration = Duration::from_secs(3);
const POST_COMMIT_WORKER_JOIN_POLICY: JoinTimeoutPolicy = JoinTimeoutPolicy {
    subsystem: "runtime_health",
    worker_kind: "post-commit worker",
    panic_message: "post-commit worker panicked during shutdown",
    timeout_message: "post-commit worker exceeded the shutdown join deadline",
};

/// Identifier-only work admitted after the canonical SQLite transaction.
///
/// This deliberately cannot carry a request body, receipt state, or a
/// prepared write. Workers reload immutable data from canonical storage after
/// the IPC response has been written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PostCommitWorkKey {
    LocalNudge(AtmMessageId),
}

/// The admission path may only signal this boundary; it never waits for a
/// worker and never owns transport, hook, or graft I/O.
pub(crate) trait PostCommitWorkQueue: Send + Sync {
    fn signal(&self, work: PostCommitWorkKey);
}

/// The daemon-owned local-nudge executor. The bounded queue retains only
/// notification identifiers after canonical SQLite admission.
pub(crate) struct LocalPostCommitWorkQueue {
    sender: SyncSender<PostCommitWorkKey>,
    // The receiver remains daemon-owned for the queue lifetime.  A worker may
    // stop and later be restarted; taking ownership of it for the first worker
    // made that otherwise ordinary recovery path permanently unavailable.
    receiver: Arc<Mutex<Receiver<PostCommitWorkKey>>>,
    local_nudge_targets: Arc<Mutex<BTreeMap<AtmMessageId, PostCommitNudgeTarget>>>,
    runtime: LocalServiceRuntime,
    home_dir: AtmHomeDir,
    observability: Arc<dyn DaemonRuntimeObservability>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<CompletionTrackedJoinHandle<()>>>,
}

#[derive(Clone)]
struct PostCommitNudgeTarget {
    team: atm_core::types::TeamName,
    agent: atm_core::types::AgentName,
}

impl LocalPostCommitWorkQueue {
    pub(crate) fn new(
        runtime: LocalServiceRuntime,
        home_dir: AtmHomeDir,
        observability: Arc<dyn DaemonRuntimeObservability>,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel(256);
        Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
            local_nudge_targets: Arc::new(Mutex::new(BTreeMap::new())),
            runtime,
            home_dir,
            observability,
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }

    pub(crate) fn register_local_nudge(
        &self,
        message_id: AtmMessageId,
        team: atm_core::types::TeamName,
        agent: atm_core::types::AgentName,
    ) {
        let Ok(mut targets) = self.local_nudge_targets.lock() else {
            tracing::error!(subsystem = "runtime_health", action = "post_commit_work_register", %message_id, "local post-commit work target registry lock poisoned");
            return;
        };
        targets.insert(message_id, PostCommitNudgeTarget { team, agent });
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        let mut worker = self.worker.lock().map_err(|_| {
            AtmError::daemon_unavailable("post-commit worker lifecycle lock poisoned")
        })?;
        if worker.is_some() {
            return Ok(());
        }
        self.stop.store(false, Ordering::SeqCst);
        let receiver = Arc::clone(&self.receiver);
        let targets = Arc::clone(&self.local_nudge_targets);
        let runtime = self.runtime.clone();
        let home_dir = self.home_dir.clone();
        let observability = self.observability.clone();
        let stop = Arc::clone(&self.stop);
        let (completion_tx, completion_rx) = mpsc::sync_channel(1);
        let join_handle = std::thread::Builder::new()
            .name("atm-post-commit-work".to_string())
            .spawn(move || {
                let _completion_tx = completion_tx;
                Self::run(receiver, targets, runtime, home_dir, observability, stop);
            })
            .map_err(|_| AtmError::daemon_unavailable("failed to start post-commit worker"))?;
        *worker = Some(CompletionTrackedJoinHandle {
            completion_rx,
            join_handle,
        });
        Ok(())
    }

    pub(crate) fn stop(&self) -> Result<(), AtmError> {
        self.stop.store(true, Ordering::SeqCst);
        let worker = self
            .worker
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("post-commit worker lifecycle lock poisoned")
            })?
            .take();
        if let Some(worker) = worker {
            join_with_timeout(
                worker,
                GRAFT_POST_SEND_IO_DEADLINE,
                POST_COMMIT_WORKER_JOIN_POLICY,
            )?;
        }
        Ok(())
    }

    fn run(
        receiver: Arc<Mutex<Receiver<PostCommitWorkKey>>>,
        targets: Arc<Mutex<BTreeMap<AtmMessageId, PostCommitNudgeTarget>>>,
        runtime: LocalServiceRuntime,
        home_dir: AtmHomeDir,
        observability: Arc<dyn DaemonRuntimeObservability>,
        stop: Arc<AtomicBool>,
    ) {
        loop {
            let received = match receiver.lock() {
                Ok(receiver) => receiver.recv_timeout(Duration::from_millis(100)),
                Err(_) => {
                    tracing::error!(
                        subsystem = "runtime_health",
                        action = "post_commit_work_receive",
                        "post-commit worker receiver lock poisoned"
                    );
                    break;
                }
            };
            let work = match received {
                Ok(work) => work,
                // Once shutdown begins, drain identifiers already accepted by
                // the bounded queue before ending the worker.  These are not
                // durable retry records, so silently abandoning them would
                // make local nudges disappear at daemon shutdown.
                Err(RecvTimeoutError::Timeout) if stop.load(Ordering::SeqCst) => break,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let PostCommitWorkKey::LocalNudge(message_id) = work;
            let target = targets
                .lock()
                .ok()
                .and_then(|mut targets| targets.remove(&message_id));
            let Some(target) = target else {
                continue;
            };
            let graft_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
                Arc::new(DaemonGraftPostSendPort::new(runtime.clone()));
            let emitter = crate::post_send_emitter::DaemonPostSendHookEmitter::new(graft_port);
            match catch_unwind(AssertUnwindSafe(|| {
                atm_core::send::emit_persisted_local_post_write(
                    &runtime,
                    observability.as_ref(),
                    home_dir.as_path(),
                    &target.team,
                    &target.agent,
                    message_id,
                    &emitter,
                )
            })) {
                Ok(Err(error)) => {
                    tracing::warn!(subsystem = "runtime_health", action = "post_commit_local_nudge", %message_id, %error, "post-commit local notification failed after admission")
                }
                Err(_) => {
                    tracing::error!(subsystem = "runtime_health", action = "post_commit_local_nudge", %message_id, "post-commit local notification panicked; worker isolated the failure and remains available")
                }
                Ok(Ok(())) => {}
            }
        }
    }
}

impl PostCommitWorkQueue for LocalPostCommitWorkQueue {
    fn signal(&self, work: PostCommitWorkKey) {
        match work {
            PostCommitWorkKey::LocalNudge(message_id) => match self
                .sender
                .try_send(PostCommitWorkKey::LocalNudge(message_id))
            {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    self.remove_local_nudge_target(message_id);
                    tracing::warn!(subsystem = "runtime_health", action = "post_commit_work_signal", work = "local_nudge", %message_id, "post-commit queue is full; local nudge was not emitted and must be retried by the caller or operator")
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.remove_local_nudge_target(message_id);
                    tracing::warn!(subsystem = "runtime_health", action = "post_commit_work_signal", work = "local_nudge", %message_id, "post-commit worker is unavailable; local nudge was not emitted and must be retried by the caller or operator")
                }
            },
        }
    }
}

impl LocalPostCommitWorkQueue {
    fn remove_local_nudge_target(&self, message_id: AtmMessageId) {
        if let Ok(mut targets) = self.local_nudge_targets.lock() {
            targets.remove(&message_id);
        }
    }
}

#[derive(Debug, Clone)]
struct DaemonGraftPostSendPort {
    runtime: LocalServiceRuntime,
}

impl DaemonGraftPostSendPort {
    fn new(runtime: LocalServiceRuntime) -> Self {
        Self { runtime }
    }
}

impl boundary::sealed::Sealed for DaemonGraftPostSendPort {}

impl boundary::GraftPostSendPort for DaemonGraftPostSendPort {
    fn deliver_post_send(
        &self,
        event: &PostSendHookEvent,
        target: &GraftNudgeTarget,
    ) -> Result<(), AtmError> {
        let Some(member) = self
            .runtime
            .load_roster_member(&target.recipient_team, &target.recipient)?
        else {
            return Err(graft_recipient_unavailable_error(
                event,
                "recipient is missing from the authoritative ATM roster",
            ));
        };
        let recipient_root = canonical_graft_root(&member.metadata_json).ok_or_else(|| {
            graft_recipient_unavailable_error(
                event,
                "recipient has no authoritative graft root for post-send delivery",
            )
        })?;
        let record_path = graft_receiver_record_path_from_root(
            recipient_root.as_path(),
            &target.recipient_team,
            &target.recipient,
        );
        deliver_post_send_to_graft_receiver(&record_path, event)
    }
}

fn deliver_post_send_to_graft_receiver(
    record_path: &std::path::Path,
    event: &PostSendHookEvent,
) -> Result<(), AtmError> {
    let request = GraftPostSendRequest {
        event: event.clone(),
    };
    match deliver_graft_post_send(
        record_path,
        &request,
        GRAFT_POST_SEND_CONNECT_DEADLINE,
        GRAFT_POST_SEND_IO_DEADLINE,
    )
    .map_err(|error| graft_transport_error(event, error))?
    {
        GraftPostSendResponse::Delivered => Ok(()),
        GraftPostSendResponse::Error(error) => Err(error),
    }
}

fn graft_transport_error(event: &PostSendHookEvent, error: AtmError) -> AtmError {
    graft_recipient_unavailable_error(event, error.detail())
}

fn graft_recipient_unavailable_error(
    event: &PostSendHookEvent,
    message: impl Into<String>,
) -> AtmError {
    AtmError::new(
        AtmErrorCode::PostSendGraftUnavailable,
        format!(
            "failed to deliver graft nudge to {}: {}",
            event.recipient,
            message.into()
        ),
    )
}
