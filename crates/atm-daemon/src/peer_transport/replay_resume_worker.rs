use std::sync::mpsc;

use atm_core::error::AtmError;

use crate::peer_transport::{
    PeerTransportRuntime, REPLAY_RESUME_POLL_INTERVAL, ReplayResumeWorkerHandle,
};

pub(super) fn start_replay_resume_worker(
    runtime: &PeerTransportRuntime,
) -> Result<ReplayResumeWorkerHandle, AtmError> {
    let (stop_tx, stop_rx) = mpsc::channel();
    let runtime = runtime.clone();
    let join_handle = std::thread::Builder::new()
        .name("atm-remote-replay-worker".to_string())
        .spawn(move || {
            loop {
                match stop_rx.recv_timeout(REPLAY_RESUME_POLL_INTERVAL) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // AG.23 migration: delete the replay sweep with the
                        // deprecated deferred-receipt replay entrypoint.
                        if let Err(error) = runtime.resume_pending_replay() {
                            tracing::warn!(
                                subsystem = "peer_transport",
                                action = "resume_pending_replay_worker",
                                outcome = "failed",
                                %error,
                                "daemon remote replay worker sweep failed"
                            );
                            runtime.client.observability.emit_or_warn(
                                "resume_pending_replay_worker",
                                "degraded",
                                "daemon remote replay worker sweep failed",
                            );
                        }
                    }
                }
            }
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn the daemon remote replay background worker")
                .with_recovery(
                    "Restart atm-daemon after restoring thread spawn capacity so deferred remote delivery can resume in the background.",
                )
                .with_source(source)
        })?;
    Ok(ReplayResumeWorkerHandle {
        stop_tx,
        join_handle,
    })
}
