use std::io;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::graft::{
    GraftAdvisoryStreamRequest, GraftBatchLimit, GraftNudgeDrainRequest, GraftNudgeFetchRequest,
    GraftSessionRegistrationRequest, GraftSessionState, GraftSessionUnregistrationRequest,
    MAX_GRAFT_BATCH_LIMIT,
};
use atm_core::protocol::ResponseEnvelope;

use crate::local_ipc::ActiveAdvisoryStream;
use crate::{
    GraftObservability, GraftSessionClient, HostNudgeInjector, SessionSnapshot, read_snapshot,
    set_session_state, validate_batch_limit_against_capacity,
};

const RECEIVE_LOOP_JOIN_DEADLINE: Duration = Duration::from_secs(5);
const MAX_LIVE_RECONNECT_BACKOFF: Duration = Duration::from_secs(5);
const MAX_LIVE_RECONNECT_ATTEMPTS: usize = 16;

pub(crate) struct ReceiveLoopContext {
    pub(crate) client: Arc<dyn GraftSessionClient>,
    pub(crate) registration_request: GraftSessionRegistrationRequest,
    pub(crate) drain_request: GraftNudgeDrainRequest,
    pub(crate) poll_interval: Duration,
    pub(crate) snapshot: Arc<RwLock<SessionSnapshot>>,
    pub(crate) injector: Arc<dyn HostNudgeInjector>,
    pub(crate) observability: Arc<dyn GraftObservability>,
    pub(crate) stop_rx: mpsc::Receiver<()>,
}

pub(crate) struct LiveReceiveLoopContext {
    pub(crate) client: Arc<dyn GraftSessionClient>,
    pub(crate) registration_request: GraftSessionRegistrationRequest,
    pub(crate) advisory_stream: ActiveAdvisoryStream,
    pub(crate) limit: GraftBatchLimit,
    pub(crate) reconnect_backoff: Duration,
    pub(crate) snapshot: Arc<RwLock<SessionSnapshot>>,
    pub(crate) injector: Arc<dyn HostNudgeInjector>,
    pub(crate) observability: Arc<dyn GraftObservability>,
    pub(crate) stop_rx: mpsc::Receiver<()>,
}

pub(crate) fn join_receive_loop_with_deadline(
    join_handle: JoinHandle<Result<(), AtmError>>,
) -> Result<(), AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let join_helper = thread::Builder::new()
        .name("atm-graft-receive-loop-join".to_string())
        .spawn(move || {
            let result = match join_handle.join() {
                Ok(result) => result,
                Err(_) => Err(AtmError::daemon_unavailable("graft receive loop panicked")
                    .with_recovery(
                        "Restart the embedding host and atm-daemon before retrying graft mode.",
                    )),
            };
            let _ = result_tx.send(result);
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn graft receive-loop join helper")
                .with_source(source)
                .with_recovery(
                    "Retry graft shutdown after the embedding host can spawn one bounded join helper thread.",
                )
        })?;
    let join_helper_thread_id = join_helper.thread().id();
    match result_rx.recv_timeout(RECEIVE_LOOP_JOIN_DEADLINE) {
        Ok(result) => {
            join_helper.join().map_err(|_| {
                AtmError::daemon_unavailable("graft receive-loop join helper panicked")
            })?;
            result
        }
        Err(RecvTimeoutError::Timeout) => {
            tracing::debug!(
                timeout_ms = RECEIVE_LOOP_JOIN_DEADLINE.as_millis(),
                thread_id = ?join_helper_thread_id,
                "graft receive-loop join timed out; helper left detached after deadline"
            );
            Err(AtmError::daemon_unavailable(format!(
                "graft receive loop shutdown exceeded the {:?} join deadline",
                RECEIVE_LOOP_JOIN_DEADLINE
            ))
            .with_recovery(
                "Restart the embedding host if the graft receive loop does not shut down within the bounded join deadline.",
            ))
        }
        Err(RecvTimeoutError::Disconnected) => join_helper.join().map_or_else(
            |_| {
                Err(AtmError::daemon_unavailable(
                    "graft receive-loop join helper panicked",
                ))
            },
            |_| {
                Err(AtmError::daemon_unavailable(
                    "graft receive-loop join helper disconnected unexpectedly",
                ))
            },
        ),
    }
}

pub(crate) fn run_receive_loop(ctx: ReceiveLoopContext) -> Result<(), AtmError> {
    let session_id = ctx.drain_request.session_id.clone();
    loop {
        match ctx.stop_rx.recv_timeout(ctx.poll_interval) {
            Ok(()) => {
                return close_receive_loop_session(
                    &ctx.snapshot,
                    &ctx.client,
                    &session_id,
                    ctx.observability.as_ref(),
                );
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                set_session_state(
                    &ctx.snapshot,
                    GraftSessionState::Closed,
                    ctx.observability.as_ref(),
                )?;
                return Ok(());
            }
        }

        match ctx.client.drain_nudges(ctx.drain_request.clone()) {
            Ok(response) => {
                if read_snapshot(&ctx.snapshot)?.state != GraftSessionState::Registered {
                    set_session_state(
                        &ctx.snapshot,
                        GraftSessionState::Registered,
                        ctx.observability.as_ref(),
                    )?;
                }
                for nudge in response.nudges {
                    ctx.injector.inject_nudge(nudge.clone())?;
                    ctx.observability.nudge_delivered(&session_id, &nudge);
                }
            }
            Err(error) => {
                set_session_state(
                    &ctx.snapshot,
                    GraftSessionState::Disconnected,
                    ctx.observability.as_ref(),
                )?;
                ctx.observability
                    .session_error(&session_id, "drain_nudges", &error);
                tracing::debug!(session_id = %session_id, error = %error.message, "graft receive loop will retry after drain failure");

                match ctx
                    .client
                    .register_session(ctx.registration_request.clone())
                {
                    Ok(response) => {
                        validate_batch_limit_against_capacity(
                            ctx.drain_request.limit,
                            response.queue_capacity,
                        )?;
                        set_session_state(
                            &ctx.snapshot,
                            GraftSessionState::Registered,
                            ctx.observability.as_ref(),
                        )?;
                    }
                    Err(register_error) if is_duplicate_registration(&register_error) => {
                        set_session_state(
                            &ctx.snapshot,
                            GraftSessionState::Registered,
                            ctx.observability.as_ref(),
                        )?;
                    }
                    Err(register_error) => {
                        ctx.observability.session_error(
                            &session_id,
                            "register_session",
                            &register_error,
                        );
                        ctx.observability
                            .session_state_changed(&read_snapshot(&ctx.snapshot)?);
                        tracing::debug!(session_id = %session_id, error = %register_error.message, "graft receive loop failed to re-register session");
                    }
                }
            }
        }
    }
}

pub(crate) fn run_live_receive_loop(mut ctx: LiveReceiveLoopContext) -> Result<(), AtmError> {
    let session_id = ctx.registration_request.session_id.clone();
    let mut reconnect_attempts = 0usize;
    let mut reconnect_backoff = ctx.reconnect_backoff;

    loop {
        if stop_requested(&ctx.stop_rx) {
            return close_receive_loop_session(
                &ctx.snapshot,
                &ctx.client,
                &session_id,
                ctx.observability.as_ref(),
            );
        }

        let frame = match atm_core::protocol::read_frame(
            &mut ctx.advisory_stream.stream,
            "failed to read graft advisory-stream frame",
            "graft advisory-stream frame exceeded the maximum supported size",
        ) {
            Ok(Some(frame)) => {
                reconnect_attempts = 0;
                reconnect_backoff = ctx.reconnect_backoff;
                frame
            }
            Ok(None) => return Ok(()),
            Err(error) if is_timeout_error(&error) => continue,
            Err(error) => {
                set_session_state(
                    &ctx.snapshot,
                    GraftSessionState::Disconnected,
                    ctx.observability.as_ref(),
                )?;
                ctx.observability
                    .session_error(&session_id, "advisory_stream", &error);

                reconnect_attempts = reconnect_attempts.saturating_add(1);
                if reconnect_attempts > MAX_LIVE_RECONNECT_ATTEMPTS {
                    return Err(AtmError::daemon_unavailable(format!(
                        "graft advisory stream exceeded the reconnect cap of {MAX_LIVE_RECONNECT_ATTEMPTS} attempts"
                    ))
                    .with_recovery(
                        "Restart the embedding host or atm-daemon after the advisory stream exceeds the bounded reconnect budget.",
                    )
                    .with_source(error));
                }
                if wait_for_stop_or_timeout(&ctx.stop_rx, reconnect_backoff) {
                    return close_receive_loop_session(
                        &ctx.snapshot,
                        &ctx.client,
                        &session_id,
                        ctx.observability.as_ref(),
                    );
                }
                ctx.advisory_stream = reopen_advisory_stream(&ctx)?;
                reconnect_backoff = next_reconnect_backoff(reconnect_backoff);
                set_session_state(
                    &ctx.snapshot,
                    GraftSessionState::Registered,
                    ctx.observability.as_ref(),
                )?;
                continue;
            }
        };
        let (response_id, response) = atm_core::protocol::response_from_frame_payload(frame)?;
        if response_id != ctx.advisory_stream.request_id {
            return Err(AtmError::daemon_unavailable(format!(
                "graft advisory stream response request_id {} did not match request_id {}",
                response_id, ctx.advisory_stream.request_id
            ))
            .with_recovery(
                "Align the embedding host, atm-graft, and atm-daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
            ));
        }
        match response {
            ResponseEnvelope::GraftAdvisoryStream(batch) => {
                if read_snapshot(&ctx.snapshot)?.state != GraftSessionState::Registered {
                    set_session_state(
                        &ctx.snapshot,
                        GraftSessionState::Registered,
                        ctx.observability.as_ref(),
                    )?;
                }
                for nudge in batch.nudges {
                    ctx.injector.inject_nudge(nudge.clone())?;
                    ctx.observability.nudge_delivered(&session_id, &nudge);
                }
            }
            ResponseEnvelope::Error(error) => return Err(error.into_atm_error()),
            other => {
                return Err(AtmError::validation(format!(
                    "transport returned an unexpected response for `graft advisory stream`: {other:?}"
                ))
                .with_recovery(
                    "Retry the graft operation once. If the mismatch persists, inspect daemon/client version alignment and retained daemon logs before retrying again.",
                ));
            }
        }
    }
}

fn reopen_advisory_stream(ctx: &LiveReceiveLoopContext) -> Result<ActiveAdvisoryStream, AtmError> {
    match ctx
        .client
        .register_session(ctx.registration_request.clone())
    {
        Ok(response) => {
            validate_batch_limit_against_capacity(ctx.limit, response.queue_capacity)?;
        }
        Err(error) if is_duplicate_registration(&error) => {}
        Err(error) => return Err(error),
    }

    let readiness_probe = GraftNudgeFetchRequest {
        session_id: ctx.registration_request.session_id.clone(),
        limit: GraftBatchLimit::new(MAX_GRAFT_BATCH_LIMIT.min(ctx.limit.get()))
            .expect("bounded readiness limit"),
    };
    match ctx.client.fetch_nudges(readiness_probe) {
        Ok(_) => {}
        Err(error) if is_session_not_registered(&error) => {
            return Err(error);
        }
        Err(error) => return Err(error),
    }

    ctx.client.open_advisory_stream(GraftAdvisoryStreamRequest {
        registration: ctx.registration_request.clone(),
        limit: ctx.limit,
    })
}

fn close_receive_loop_session(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    client: &Arc<dyn GraftSessionClient>,
    session_id: &atm_core::graft::GraftSessionId,
    observability: &dyn GraftObservability,
) -> Result<(), AtmError> {
    match client.unregister_session(GraftSessionUnregistrationRequest {
        session_id: session_id.clone(),
    }) {
        Ok(_) => {
            set_session_state(snapshot, GraftSessionState::Closed, observability)?;
            Ok(())
        }
        Err(error) => {
            set_session_state(snapshot, GraftSessionState::CloseFailed, observability)?;
            observability.session_error(session_id, "unregister_session", &error);
            Err(error)
        }
    }
}

fn wait_for_stop_or_timeout(stop_rx: &mpsc::Receiver<()>, timeout: Duration) -> bool {
    match stop_rx.recv_timeout(timeout) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
        Err(RecvTimeoutError::Timeout) => false,
    }
}

fn stop_requested(stop_rx: &mpsc::Receiver<()>) -> bool {
    match stop_rx.try_recv() {
        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => true,
        Err(mpsc::TryRecvError::Empty) => false,
    }
}

fn next_reconnect_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(MAX_LIVE_RECONNECT_BACKOFF)
}

fn is_timeout_error(error: &AtmError) -> bool {
    error
        .source
        .as_ref()
        .and_then(|source| source.downcast_ref::<io::Error>())
        .is_some_and(|io_error| {
            matches!(
                io_error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            )
        })
}

fn is_duplicate_registration(error: &AtmError) -> bool {
    error.code == AtmErrorCode::DaemonGraftSessionAlreadyRegistered
}

fn is_session_not_registered(error: &AtmError) -> bool {
    error.code == AtmErrorCode::DaemonGraftSessionNotRegistered
}
