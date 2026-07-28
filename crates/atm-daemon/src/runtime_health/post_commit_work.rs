//! Identifier-only work that runs after local SQLite admission.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::{
    LocalServiceRuntime,
    boundary::{self, GraftNudgeTarget, PostSendHookEvent},
    error::{AtmError, AtmErrorCode},
    graft::{
        GraftPostSendRequest, GraftPostSendResponse, deliver_graft_post_send,
        graft_receiver_record_path_from_home,
    },
    schema::{AtmMessageId, canonical_home_dir},
};

use crate::AtmHomeDir;
use crate::peer_drain_coordinator::PeerDeliveryCoordinator;

const GRAFT_POST_SEND_CONNECT_DEADLINE: Duration = Duration::from_millis(250);
const GRAFT_POST_SEND_IO_DEADLINE: Duration = Duration::from_secs(3);

/// Identifier-only work admitted after the canonical SQLite transaction.
///
/// This deliberately cannot carry a request body, receipt state, or a
/// prepared write. Workers reload immutable data from canonical storage after
/// the IPC response has been written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PostCommitWorkKey {
    LocalNudge(AtmMessageId),
    PeerDelivery {
        peer: atm_core::types::HostName,
        message_id: AtmMessageId,
    },
}

/// The admission path may only signal this boundary; it never waits for a
/// worker and never owns transport, hook, or graft I/O.
pub(crate) trait PostCommitWorkQueue: Send + Sync {
    fn signal(&self, work: PostCommitWorkKey);
}

/// The daemon-owned worker adapter. The bounded queue retains only work
/// identifiers; it reloads the committed record before invoking a hook.
pub(crate) struct PeerPostCommitWorkQueue {
    coordinator: Arc<dyn PeerDeliveryCoordinator>,
    sender: SyncSender<PostCommitWorkKey>,
    receiver: Mutex<Option<Receiver<PostCommitWorkKey>>>,
    local_nudge_targets: Arc<Mutex<BTreeMap<AtmMessageId, PostCommitNudgeTarget>>>,
    runtime: LocalServiceRuntime,
    home_dir: AtmHomeDir,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone)]
struct PostCommitNudgeTarget {
    team: atm_core::types::TeamName,
    agent: atm_core::types::AgentName,
}

impl PeerPostCommitWorkQueue {
    pub(crate) fn new(
        coordinator: Arc<dyn PeerDeliveryCoordinator>,
        runtime: LocalServiceRuntime,
        home_dir: AtmHomeDir,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel(256);
        Self {
            coordinator,
            sender,
            receiver: Mutex::new(Some(receiver)),
            local_nudge_targets: Arc::new(Mutex::new(BTreeMap::new())),
            runtime,
            home_dir,
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
        let receiver = self
            .receiver
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("post-commit worker receiver lock poisoned"))?
            .take()
            .ok_or_else(|| {
                AtmError::daemon_unavailable("post-commit worker cannot restart after shutdown")
            })?;
        self.stop.store(false, Ordering::SeqCst);
        let targets = Arc::clone(&self.local_nudge_targets);
        let runtime = self.runtime.clone();
        let home_dir = self.home_dir.clone();
        let stop = Arc::clone(&self.stop);
        *worker = Some(
            std::thread::Builder::new()
                .name("atm-post-commit-work".to_string())
                .spawn(move || Self::run(receiver, targets, runtime, home_dir, stop))
                .map_err(|_| AtmError::daemon_unavailable("failed to start post-commit worker"))?,
        );
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
            worker.join().map_err(|_| {
                AtmError::daemon_unavailable("post-commit worker panicked during shutdown")
            })?;
        }
        Ok(())
    }

    fn run(
        receiver: Receiver<PostCommitWorkKey>,
        targets: Arc<Mutex<BTreeMap<AtmMessageId, PostCommitNudgeTarget>>>,
        runtime: LocalServiceRuntime,
        home_dir: AtmHomeDir,
        stop: Arc<AtomicBool>,
    ) {
        while !stop.load(Ordering::SeqCst) {
            let work = match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(work) => work,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let PostCommitWorkKey::LocalNudge(message_id) = work else {
                continue;
            };
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

impl PostCommitWorkQueue for PeerPostCommitWorkQueue {
    fn signal(&self, work: PostCommitWorkKey) {
        match work {
            PostCommitWorkKey::PeerDelivery { peer, .. } => {
                self.coordinator.signal_after_persist(peer)
            }
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

impl PeerPostCommitWorkQueue {
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
        let recipient_home_dir = canonical_home_dir(&member.metadata_json).ok_or_else(|| {
            graft_recipient_unavailable_error(
                event,
                "recipient has no authoritative home_dir for graft post-send delivery",
            )
        })?;
        let record_path = graft_receiver_record_path_from_home(
            recipient_home_dir.as_path(),
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
