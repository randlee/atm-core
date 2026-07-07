use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

use crate::graft_rpc;
use crate::graft_rpc::ResponseEnvelope;
use crate::graft_rpc::{AdvisoryDrainResponse, AdvisorySessionId, AdvisoryStreamResponse};
use atm_core::GraftConfig;
use atm_core::error::{AtmError, AtmErrorKind};

use crate::transport::ActiveAdvisoryStream;
use crate::{
    AdvisoryBatchLimit, AdvisoryDrainRequest, AdvisorySessionPort,
    AdvisorySessionRegistrationRequest, AdvisorySessionRegistrationResponse, AdvisorySessionState,
    AdvisorySessionUnregistrationRequest, AdvisoryStreamRequest, GraftObservability,
    GraftSessionClient, RECEIVE_LOOP_JOIN_DEADLINE, SessionSnapshot,
};

const MAX_LIVE_RECONNECT_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) fn load_graft_config(workspace_root: &Path) -> Result<Option<GraftConfig>, AtmError> {
    let config = atm_core::load_atm_config(workspace_root)?;
    Ok(config.map(|config| config.graft))
}

pub(crate) fn read_snapshot(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
) -> Result<SessionSnapshot, AtmError> {
    snapshot
        .read()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| {
            AtmError::new(
                AtmErrorKind::Internal,
                "graft session snapshot lock poisoned",
            )
            .with_recovery(
                "Restart the embedding host before retrying graft session lifecycle operations.",
            )
        })
}

fn write_snapshot(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    state: AdvisorySessionState,
) -> Result<(), AtmError> {
    let mut snapshot = snapshot.write().map_err(|_| {
        AtmError::new(
            AtmErrorKind::Internal,
            "graft session snapshot lock poisoned",
        )
        .with_recovery(
            "Restart the embedding host before retrying graft session lifecycle operations.",
        )
    })?;
    snapshot.state = state;
    Ok(())
}

pub(crate) fn set_session_state(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    state: AdvisorySessionState,
    observability: &dyn GraftObservability,
) -> Result<(), AtmError> {
    write_snapshot(snapshot, state)?;
    observability.session_state_changed(&read_snapshot(snapshot)?);
    Ok(())
}

pub(crate) fn validate_batch_limit_against_capacity(
    batch_limit: AdvisoryBatchLimit,
    queue_capacity: usize,
) -> Result<(), AtmError> {
    if batch_limit.get() > queue_capacity {
        return Err(AtmError::validation(format!(
            "graft batch limit {} exceeds daemon queue capacity {}",
            batch_limit.get(),
            queue_capacity
        ))
        .with_recovery(
            "Lower the graft batch limit or restart against a daemon that advertises a larger graft nudge queue before retrying session activation.",
        ));
    }
    Ok(())
}

pub(crate) fn register_session_with_validated_batch_limit(
    client: &dyn AdvisorySessionPort,
    request: AdvisorySessionRegistrationRequest,
    batch_limit: AdvisoryBatchLimit,
) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
    let response = client.register_session(request.clone())?;
    if let Err(validation_error) =
        validate_batch_limit_against_capacity(batch_limit, response.queue_capacity)
    {
        return Err(cleanup_registered_session_after_error(
            client,
            &request.session_id,
            "graft batch-limit validation",
            validation_error,
        ));
    }
    Ok(response)
}

pub(crate) fn cleanup_registered_session_after_error(
    client: &dyn AdvisorySessionPort,
    session_id: &crate::AdvisorySessionId,
    failed_step: &str,
    original_error: AtmError,
) -> AtmError {
    let original_code = original_error.code;
    let original_message = original_error.message.clone();
    match client.unregister_session(AdvisorySessionUnregistrationRequest {
        session_id: session_id.clone(),
    }) {
        Ok(cleanup) if cleanup.closed => original_error,
        Ok(_) => AtmError::daemon_advisory_session_cleanup_failed(format!(
            "graft advisory session {session_id} may still be registered after {failed_step} failed ({original_code}: {original_message}); daemon unregister completed without closing the session"
        ))
        .with_source(original_error),
        Err(cleanup_error) => AtmError::daemon_advisory_session_cleanup_failed(format!(
            "graft advisory session {session_id} may still be registered after {failed_step} failed ({original_code}: {original_message}); daemon unregister failed during cleanup"
        ))
        .with_source(cleanup_error),
    }
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
                Err(_) => Err(AtmError::new(
                    AtmErrorKind::Internal,
                    "graft receive loop panicked",
                )
                    .with_recovery(
                        "Restart the embedding host and atm-daemon before retrying graft mode.",
                    )),
            };
            let _ = result_tx.send(result);
        })
        .map_err(|source| {
            AtmError::new(
                AtmErrorKind::Internal,
                "failed to spawn graft receive-loop join helper",
            )
                .with_source(source)
                .with_recovery(
                    "Retry graft shutdown after the embedding host can spawn one bounded join helper thread.",
                )
        })?;
    let join_helper_thread_id = join_helper.thread().id();
    match result_rx.recv_timeout(RECEIVE_LOOP_JOIN_DEADLINE) {
        Ok(result) => {
            join_helper.join().map_err(|_| {
                AtmError::new(
                    AtmErrorKind::Internal,
                    "graft receive-loop join helper panicked",
                )
                .with_recovery(
                    "Restart the embedding host and atm-daemon before retrying graft mode.",
                )
            })?;
            result
        }
        Err(RecvTimeoutError::Timeout) => {
            tracing::debug!(
                timeout_ms = RECEIVE_LOOP_JOIN_DEADLINE.as_millis(),
                thread_id = ?join_helper_thread_id,
                "graft receive-loop join timed out; helper left detached after deadline"
            );
            Err(AtmError::new(
                AtmErrorKind::Timeout,
                format!(
                    "graft receive loop shutdown exceeded the {:?} join deadline",
                    RECEIVE_LOOP_JOIN_DEADLINE
                ),
            )
            .with_recovery(
                "Restart the embedding host if the graft receive loop does not shut down within the bounded join deadline.",
            ))
        }
        Err(RecvTimeoutError::Disconnected) => join_helper.join().map_or_else(
            |_| {
                Err(AtmError::new(
                    AtmErrorKind::Internal,
                    "graft receive-loop join helper panicked",
                )
                .with_recovery(
                    "Restart the embedding host and atm-daemon before retrying graft mode.",
                ))
            },
            |_| {
                Err(AtmError::new(
                    AtmErrorKind::Internal,
                    "graft receive-loop join helper disconnected unexpectedly",
                )
                .with_recovery(
                    "Restart the embedding host and atm-daemon before retrying graft mode.",
                ))
            },
        ),
    }
}

pub(crate) struct ReceiveLoopContext {
    pub(crate) client: Arc<dyn GraftSessionClient>,
    pub(crate) registration_request: AdvisorySessionRegistrationRequest,
    pub(crate) drain_request: AdvisoryDrainRequest,
    pub(crate) poll_interval: std::time::Duration,
    pub(crate) snapshot: Arc<RwLock<SessionSnapshot>>,
    pub(crate) injector: Arc<dyn crate::HostNudgeInjector>,
    pub(crate) observability: Arc<dyn GraftObservability>,
    pub(crate) stop_rx: Receiver<()>,
}

pub(crate) struct LiveReceiveLoopContext {
    pub(crate) client: Arc<dyn GraftSessionClient>,
    pub(crate) registration_request: AdvisorySessionRegistrationRequest,
    pub(crate) advisory_stream: ActiveAdvisoryStream,
    pub(crate) limit: AdvisoryBatchLimit,
    pub(crate) reconnect_backoff: std::time::Duration,
    pub(crate) snapshot: Arc<RwLock<SessionSnapshot>>,
    pub(crate) injector: Arc<dyn crate::HostNudgeInjector>,
    pub(crate) observability: Arc<dyn GraftObservability>,
    pub(crate) stop_rx: Receiver<()>,
}

pub(crate) fn run_receive_loop(ctx: ReceiveLoopContext) -> Result<(), AtmError> {
    let session_id = ctx.drain_request.session_id.clone();
    loop {
        if should_stop_receive_loop(&ctx, &session_id)? {
            return Ok(());
        }

        match ctx.client.drain_nudges(ctx.drain_request.clone()) {
            Ok(response) => handle_drain_response(&ctx, &session_id, response)?,
            Err(error) => handle_drain_failure(&ctx, &session_id, error)?,
        }
    }
}

pub(crate) fn run_live_receive_loop(mut ctx: LiveReceiveLoopContext) -> Result<(), AtmError> {
    let mut backoff = ctx.reconnect_backoff;
    loop {
        if stop_requested(&ctx.stop_rx) {
            return close_live_receive_loop(&ctx);
        }

        let frame = match graft_rpc::read_frame(
            &mut ctx.advisory_stream.stream,
            "failed to read graft advisory-stream frame",
            "graft advisory-stream frame exceeded the maximum supported size",
        ) {
            Ok(Some(frame)) => frame,
            Ok(None) => return Ok(()),
            Err(error) if is_socket_timeout_error(&error) => {
                continue;
            }
            Err(error) => {
                backoff = reconnect_live_receive_loop(&mut ctx, error, backoff)?;
                continue;
            }
        };
        let (response_id, response) = graft_rpc::response_from_raw_parts(
            frame.request_id,
            frame.message_kind.code(),
            frame.flags,
            frame.bytes,
        )?;
        if response_id != ctx.advisory_stream.request_id {
            return Err(AtmError::new(
                AtmErrorKind::Internal,
                format!(
                    "advisory stream response request_id {} did not match request_id {}",
                    response_id, ctx.advisory_stream.request_id
                ),
            )
            .with_recovery(
                "Align the embedding host, atm-graft, and atm-daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
            ));
        }
        match response {
            ResponseEnvelope::AdvisoryStream(batch) => {
                backoff = handle_live_advisory_batch(&ctx, batch)?;
            }
            ResponseEnvelope::Error(error) => return Err(error.into_atm_error()),
            other => {
                return Err(crate::transport::unexpected_response(
                    "advisory stream",
                    other,
                ));
            }
        }
    }
}

fn should_stop_receive_loop(
    ctx: &ReceiveLoopContext,
    session_id: &AdvisorySessionId,
) -> Result<bool, AtmError> {
    match ctx.stop_rx.recv_timeout(ctx.poll_interval) {
        Ok(()) => {
            unregister_session_and_close(
                &*ctx.client,
                session_id,
                &ctx.snapshot,
                ctx.observability.as_ref(),
            )?;
            Ok(true)
        }
        Err(RecvTimeoutError::Timeout) => Ok(false),
        Err(RecvTimeoutError::Disconnected) => {
            set_session_state(
                &ctx.snapshot,
                AdvisorySessionState::Closed,
                ctx.observability.as_ref(),
            )?;
            Ok(true)
        }
    }
}

fn handle_drain_response(
    ctx: &ReceiveLoopContext,
    session_id: &AdvisorySessionId,
    response: AdvisoryDrainResponse,
) -> Result<(), AtmError> {
    ensure_registered_snapshot(&ctx.snapshot, ctx.observability.as_ref())?;
    for nudge in response.nudges {
        ctx.injector.inject_nudge(nudge.clone())?;
        ctx.observability.nudge_delivered(session_id, &nudge);
    }
    Ok(())
}

fn handle_drain_failure(
    ctx: &ReceiveLoopContext,
    session_id: &AdvisorySessionId,
    error: AtmError,
) -> Result<(), AtmError> {
    set_session_state(
        &ctx.snapshot,
        AdvisorySessionState::Disconnected,
        ctx.observability.as_ref(),
    )?;
    ctx.observability
        .session_error(session_id, "drain_nudges", &error);
    tracing::debug!(session_id = %session_id, error = %error.message, "graft receive loop will retry after drain failure");
    attempt_receive_loop_reregistration(ctx, session_id)
}

fn attempt_receive_loop_reregistration(
    ctx: &ReceiveLoopContext,
    session_id: &AdvisorySessionId,
) -> Result<(), AtmError> {
    match register_session_with_validated_batch_limit(
        &*ctx.client,
        ctx.registration_request.clone(),
        ctx.drain_request.limit,
    ) {
        Ok(_) => ensure_registered_snapshot(&ctx.snapshot, ctx.observability.as_ref()),
        Err(register_error) if is_duplicate_registration(&register_error) => {
            ensure_registered_snapshot(&ctx.snapshot, ctx.observability.as_ref())
        }
        Err(register_error) => {
            if register_error.is_validation() {
                ctx.observability.session_error(
                    session_id,
                    "validate_batch_limit",
                    &register_error,
                );
                return Err(register_error);
            }
            ctx.observability
                .session_error(session_id, "register_session", &register_error);
            ctx.observability
                .session_state_changed(&read_snapshot(&ctx.snapshot)?);
            tracing::debug!(session_id = %session_id, error = %register_error.message, "graft receive loop failed to re-register session");
            Ok(())
        }
    }
}

fn ensure_registered_snapshot(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    observability: &dyn GraftObservability,
) -> Result<(), AtmError> {
    if read_snapshot(snapshot)?.state != AdvisorySessionState::Registered {
        set_session_state(snapshot, AdvisorySessionState::Registered, observability)?;
    }
    Ok(())
}

fn close_live_receive_loop(ctx: &LiveReceiveLoopContext) -> Result<(), AtmError> {
    unregister_session_and_close(
        &*ctx.client,
        &ctx.registration_request.session_id,
        &ctx.snapshot,
        ctx.observability.as_ref(),
    )
}

fn reconnect_live_receive_loop(
    ctx: &mut LiveReceiveLoopContext,
    error: AtmError,
    backoff: std::time::Duration,
) -> Result<std::time::Duration, AtmError> {
    set_session_state(
        &ctx.snapshot,
        AdvisorySessionState::Disconnected,
        ctx.observability.as_ref(),
    )?;
    ctx.observability.session_error(
        &ctx.registration_request.session_id,
        "advisory_stream",
        &error,
    );
    let _ = ctx.stop_rx.recv_timeout(backoff);
    if stop_requested(&ctx.stop_rx) {
        close_live_receive_loop(ctx)?;
        return Ok(backoff);
    }
    reregister_live_receive_loop(ctx)?;
    let advisory_stream = ctx
        .client
        .open_advisory_stream(AdvisoryStreamRequest {
            registration: ctx.registration_request.clone(),
            limit: ctx.limit,
        })
        .map_err(|error| {
            cleanup_registered_session_after_error(
                &*ctx.client,
                &ctx.registration_request.session_id,
                "graft advisory-stream reconnect",
                error,
            )
        })?;
    ctx.advisory_stream = advisory_stream;
    ensure_registered_snapshot(&ctx.snapshot, ctx.observability.as_ref()).map_err(|error| {
        cleanup_registered_session_after_error(
            &*ctx.client,
            &ctx.registration_request.session_id,
            "graft advisory-stream reconnect state publication",
            error,
        )
    })?;
    Ok(std::cmp::min(
        backoff.saturating_mul(2),
        MAX_LIVE_RECONNECT_BACKOFF,
    ))
}

fn reregister_live_receive_loop(ctx: &LiveReceiveLoopContext) -> Result<(), AtmError> {
    match register_session_with_validated_batch_limit(
        &*ctx.client,
        ctx.registration_request.clone(),
        ctx.limit,
    ) {
        Ok(_) => Ok(()),
        Err(register_error) if is_duplicate_registration(&register_error) => Ok(()),
        Err(register_error) => Err(register_error),
    }
}

fn handle_live_advisory_batch(
    ctx: &LiveReceiveLoopContext,
    batch: AdvisoryStreamResponse,
) -> Result<std::time::Duration, AtmError> {
    ensure_registered_snapshot(&ctx.snapshot, ctx.observability.as_ref())?;
    for nudge in batch.nudges {
        ctx.injector.inject_nudge(nudge.clone())?;
        ctx.observability
            .nudge_delivered(&ctx.registration_request.session_id, &nudge);
    }
    Ok(ctx.reconnect_backoff)
}

fn unregister_session_and_close(
    client: &dyn AdvisorySessionPort,
    session_id: &AdvisorySessionId,
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    observability: &dyn GraftObservability,
) -> Result<(), AtmError> {
    match client.unregister_session(AdvisorySessionUnregistrationRequest {
        session_id: session_id.clone(),
    }) {
        Ok(_) => set_session_state(snapshot, AdvisorySessionState::Closed, observability),
        Err(error) => {
            set_session_state(snapshot, AdvisorySessionState::CloseFailed, observability)?;
            observability.session_error(session_id, "unregister_session", &error);
            Err(error)
        }
    }
}

fn is_duplicate_registration(error: &AtmError) -> bool {
    error.code == atm_core::error_codes::AtmErrorCode::DaemonAdvisorySessionAlreadyRegistered
}

fn stop_requested(stop_rx: &Receiver<()>) -> bool {
    matches!(stop_rx.try_recv(), Ok(()) | Err(TryRecvError::Disconnected))
}

fn is_socket_timeout_error(error: &AtmError) -> bool {
    error
        .source
        .as_ref()
        .and_then(|source| source.downcast_ref::<io::Error>())
        .is_some_and(|source| {
            matches!(
                source.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            )
        })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, RwLock};
    use std::time::Duration;

    use atm_core::ack::{AckOutcome, AckRequest};
    use atm_core::error::{AtmError, AtmErrorKind};
    use atm_core::graft::AtmGraftClient;
    use atm_core::protocol;
    use atm_core::read::{ReadOutcome, ReadQuery};
    use atm_core::send::{SendOutcome, SendRequest};
    use atm_core::types::IsoTimestamp;
    use interprocess::local_socket::prelude::*;
    use interprocess::local_socket::{ListenerOptions, Stream as LocalSocketStream};
    use tempfile::TempDir;

    use super::{
        LiveReceiveLoopContext, ReceiveLoopContext, attempt_receive_loop_reregistration,
        read_snapshot, reconnect_live_receive_loop, register_session_with_validated_batch_limit,
    };
    use crate::{
        AdvisoryBatchLimit, AdvisoryDrainRequest, AdvisoryDrainResponse, AdvisoryEvent,
        AdvisoryFetchRequest, AdvisoryFetchResponse, AdvisorySessionId, AdvisorySessionPort,
        AdvisorySessionRegistrationRequest, AdvisorySessionRegistrationResponse,
        AdvisorySessionState, AdvisorySessionUnregistrationRequest,
        AdvisorySessionUnregistrationResponse, AdvisoryStreamRequest, GraftObservability,
        GraftSessionClient, HostNudgeInjector, SessionSnapshot, transport::ActiveAdvisoryStream,
    };

    #[derive(Debug, Clone, Copy)]
    enum UnregisterBehavior {
        Closed,
        NotClosed,
        Error,
    }

    #[derive(Debug)]
    struct RecordingSessionClient {
        queue_capacity: usize,
        unregister_behavior: UnregisterBehavior,
        unregister_calls: Mutex<Vec<AdvisorySessionId>>,
    }

    impl RecordingSessionClient {
        fn new(queue_capacity: usize) -> Self {
            Self {
                queue_capacity,
                unregister_behavior: UnregisterBehavior::Closed,
                unregister_calls: Mutex::new(Vec::new()),
            }
        }

        fn with_unregister_behavior(
            queue_capacity: usize,
            unregister_behavior: UnregisterBehavior,
        ) -> Self {
            Self {
                queue_capacity,
                unregister_behavior,
                unregister_calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl AtmGraftClient for RecordingSessionClient {
        fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
            panic!("send_message is not used by runtime registration tests")
        }

        fn read_message(&self, _query: ReadQuery) -> Result<ReadOutcome, AtmError> {
            panic!("read_message is not used by runtime registration tests")
        }

        fn acknowledge_message(&self, _request: AckRequest) -> Result<AckOutcome, AtmError> {
            panic!("acknowledge_message is not used by runtime registration tests")
        }
    }

    impl AdvisorySessionPort for RecordingSessionClient {
        fn register_session(
            &self,
            request: AdvisorySessionRegistrationRequest,
        ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
            Ok(AdvisorySessionRegistrationResponse {
                team: request.team,
                agent: request.agent,
                session_id: request.session_id,
                registered_at: IsoTimestamp::now(),
                queue_capacity: self.queue_capacity,
            })
        }

        fn unregister_session(
            &self,
            request: AdvisorySessionUnregistrationRequest,
        ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
            self.unregister_calls
                .lock()
                .expect("unregister calls")
                .push(request.session_id.clone());
            match self.unregister_behavior {
                UnregisterBehavior::Closed => Ok(AdvisorySessionUnregistrationResponse {
                    session_id: request.session_id,
                    closed: true,
                }),
                UnregisterBehavior::NotClosed => Ok(AdvisorySessionUnregistrationResponse {
                    session_id: request.session_id,
                    closed: false,
                }),
                UnregisterBehavior::Error => Err(AtmError::new(
                    AtmErrorKind::Internal,
                    "simulated unregister failure",
                )),
            }
        }

        fn fetch_nudges(
            &self,
            _request: AdvisoryFetchRequest,
        ) -> Result<AdvisoryFetchResponse, AtmError> {
            panic!("fetch_nudges is not used by runtime registration tests")
        }

        fn drain_nudges(
            &self,
            _request: AdvisoryDrainRequest,
        ) -> Result<AdvisoryDrainResponse, AtmError> {
            panic!("drain_nudges is not used by runtime registration tests")
        }
    }

    impl GraftSessionClient for RecordingSessionClient {
        fn supports_live_advisory_stream(&self) -> bool {
            false
        }

        fn open_advisory_stream(
            &self,
            _request: AdvisoryStreamRequest,
        ) -> Result<crate::transport::ActiveAdvisoryStream, AtmError> {
            panic!("open_advisory_stream is not used by runtime registration tests")
        }
    }

    #[derive(Debug, Default)]
    struct NoopInjector;

    impl HostNudgeInjector for NoopInjector {
        fn inject_nudge(&self, _nudge: AdvisoryEvent) -> Result<(), AtmError> {
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct NoopObservability;

    impl GraftObservability for NoopObservability {}

    fn registration_request() -> AdvisorySessionRegistrationRequest {
        AdvisorySessionRegistrationRequest {
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
            session_id: AdvisorySessionId::new("session-1").expect("session id"),
            pid: 4242,
            started_at: IsoTimestamp::now(),
        }
    }

    #[test]
    fn register_session_with_invalid_batch_limit_cleans_up_registered_slot() {
        let client = RecordingSessionClient::new(1);
        let error = register_session_with_validated_batch_limit(
            &client,
            registration_request(),
            AdvisoryBatchLimit::new(8).expect("limit"),
        )
        .expect_err("batch-limit validation should fail");

        assert!(error.is_validation());
        assert_eq!(
            client
                .unregister_calls
                .lock()
                .expect("unregister calls")
                .len(),
            1
        );
    }

    #[test]
    fn register_session_with_invalid_batch_limit_reports_not_closed_cleanup_failure() {
        let client =
            RecordingSessionClient::with_unregister_behavior(1, UnregisterBehavior::NotClosed);
        let error = register_session_with_validated_batch_limit(
            &client,
            registration_request(),
            AdvisoryBatchLimit::new(8).expect("limit"),
        )
        .expect_err("batch-limit validation should fail");

        assert!(!error.is_validation());
        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::DaemonAdvisorySessionCleanupFailed
        );
        assert!(error.message.contains("without closing the session"));
        assert_eq!(
            client
                .unregister_calls
                .lock()
                .expect("unregister calls")
                .len(),
            1
        );
    }

    #[test]
    fn register_session_with_invalid_batch_limit_reports_unregister_cleanup_failure() {
        let client = RecordingSessionClient::with_unregister_behavior(1, UnregisterBehavior::Error);
        let error = register_session_with_validated_batch_limit(
            &client,
            registration_request(),
            AdvisoryBatchLimit::new(8).expect("limit"),
        )
        .expect_err("batch-limit validation should fail");

        assert!(!error.is_validation());
        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::DaemonAdvisorySessionCleanupFailed
        );
        assert!(
            error
                .message
                .contains("daemon unregister failed during cleanup")
        );
        let source = error.source.as_ref().expect("cleanup source").to_string();
        assert!(source.contains("simulated unregister failure"));
        assert!(!source.contains("atm-daemon binary is installed"));
        assert_eq!(
            client
                .unregister_calls
                .lock()
                .expect("unregister calls")
                .len(),
            1
        );
    }

    #[test]
    fn reregistration_cleans_up_registered_slot_when_batch_limit_validation_fails() {
        let client = Arc::new(RecordingSessionClient::new(1));
        let registration = registration_request();
        let session_id = registration.session_id.clone();
        let snapshot = Arc::new(RwLock::new(SessionSnapshot {
            team: registration.team.clone(),
            agent: registration.agent.clone(),
            session_id: session_id.clone(),
            state: AdvisorySessionState::Disconnected,
        }));
        let (_stop_tx, stop_rx) = mpsc::channel();
        let ctx = ReceiveLoopContext {
            client: client.clone(),
            registration_request: registration,
            drain_request: AdvisoryDrainRequest {
                session_id: session_id.clone(),
                limit: AdvisoryBatchLimit::new(8).expect("limit"),
            },
            poll_interval: Duration::from_millis(10),
            snapshot: snapshot.clone(),
            injector: Arc::new(NoopInjector),
            observability: Arc::new(NoopObservability),
            stop_rx,
        };

        let error = attempt_receive_loop_reregistration(&ctx, &session_id)
            .expect_err("invalid batch limit must fail after cleanup");

        assert!(error.is_validation());
        assert_eq!(
            client
                .unregister_calls
                .lock()
                .expect("unregister calls")
                .len(),
            1
        );
        assert_eq!(
            read_snapshot(&snapshot).expect("snapshot").state,
            AdvisorySessionState::Disconnected
        );
    }

    #[derive(Debug)]
    struct ReconnectFailureClient {
        unregister_calls: Mutex<Vec<AdvisorySessionId>>,
    }

    impl AdvisorySessionPort for ReconnectFailureClient {
        fn register_session(
            &self,
            request: AdvisorySessionRegistrationRequest,
        ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
            Ok(AdvisorySessionRegistrationResponse {
                team: request.team,
                agent: request.agent,
                session_id: request.session_id,
                registered_at: IsoTimestamp::now(),
                queue_capacity: 16,
            })
        }

        fn unregister_session(
            &self,
            request: AdvisorySessionUnregistrationRequest,
        ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
            self.unregister_calls
                .lock()
                .expect("unregister calls")
                .push(request.session_id.clone());
            Ok(AdvisorySessionUnregistrationResponse {
                session_id: request.session_id,
                closed: true,
            })
        }

        fn fetch_nudges(
            &self,
            _request: AdvisoryFetchRequest,
        ) -> Result<AdvisoryFetchResponse, AtmError> {
            panic!("fetch_nudges is not used by reconnect tests")
        }

        fn drain_nudges(
            &self,
            _request: AdvisoryDrainRequest,
        ) -> Result<AdvisoryDrainResponse, AtmError> {
            panic!("drain_nudges is not used by reconnect tests")
        }
    }

    impl AtmGraftClient for ReconnectFailureClient {
        fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
            panic!("send_message is not used by reconnect tests")
        }

        fn read_message(&self, _query: ReadQuery) -> Result<ReadOutcome, AtmError> {
            panic!("read_message is not used by reconnect tests")
        }

        fn acknowledge_message(&self, _request: AckRequest) -> Result<AckOutcome, AtmError> {
            panic!("acknowledge_message is not used by reconnect tests")
        }
    }

    impl GraftSessionClient for ReconnectFailureClient {
        fn supports_live_advisory_stream(&self) -> bool {
            true
        }

        fn open_advisory_stream(
            &self,
            _request: AdvisoryStreamRequest,
        ) -> Result<ActiveAdvisoryStream, AtmError> {
            Err(AtmError::new(
                AtmErrorKind::Internal,
                "simulated advisory-stream reopen failure",
            ))
        }
    }

    #[test]
    fn reconnect_live_receive_loop_cleans_up_when_stream_reopen_fails() {
        let tempdir = TempDir::new().expect("tempdir");
        let endpoint_path = tempdir.path().join("reconnect.sock");
        let listener = ListenerOptions::new()
            .name(protocol::daemon_local_ipc_name_from_path(&endpoint_path).expect("endpoint"))
            .create_sync()
            .expect("create listener");
        let server = std::thread::spawn(move || {
            let _stream = listener.accept().expect("accept");
        });
        let stream = LocalSocketStream::connect(
            protocol::daemon_local_ipc_name_from_path(&endpoint_path).expect("endpoint"),
        )
        .expect("connect");
        let client = Arc::new(ReconnectFailureClient {
            unregister_calls: Mutex::new(Vec::new()),
        });
        let snapshot = Arc::new(RwLock::new(SessionSnapshot {
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
            session_id: AdvisorySessionId::new("session-1").expect("session"),
            state: AdvisorySessionState::Registered,
        }));
        let (_stop_tx, stop_rx) = mpsc::channel();
        let mut ctx = LiveReceiveLoopContext {
            client: Arc::clone(&client) as Arc<dyn GraftSessionClient>,
            registration_request: registration_request(),
            advisory_stream: ActiveAdvisoryStream {
                stream,
                request_id: atm_daemon_client::RequestId::new(
                    protocol::next_request_id().into_inner(),
                )
                .expect("request id"),
            },
            limit: AdvisoryBatchLimit::new(8).expect("limit"),
            reconnect_backoff: Duration::from_millis(1),
            snapshot,
            injector: Arc::new(NoopInjector),
            observability: Arc::new(NoopObservability),
            stop_rx,
        };

        let error = reconnect_live_receive_loop(
            &mut ctx,
            AtmError::new(AtmErrorKind::Internal, "stream read failed"),
            Duration::from_millis(1),
        )
        .expect_err("stream reopen should fail");

        assert_eq!(error.message, "simulated advisory-stream reopen failure");
        assert_eq!(
            client
                .unregister_calls
                .lock()
                .expect("unregister calls")
                .len(),
            1
        );
        server.join().expect("join server");
    }

    #[derive(Debug)]
    struct ReconnectValidationFailureClient {
        unregister_calls: Mutex<Vec<AdvisorySessionId>>,
    }

    impl AdvisorySessionPort for ReconnectValidationFailureClient {
        fn register_session(
            &self,
            request: AdvisorySessionRegistrationRequest,
        ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
            Ok(AdvisorySessionRegistrationResponse {
                team: request.team,
                agent: request.agent,
                session_id: request.session_id,
                registered_at: IsoTimestamp::now(),
                queue_capacity: 1,
            })
        }

        fn unregister_session(
            &self,
            request: AdvisorySessionUnregistrationRequest,
        ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
            self.unregister_calls
                .lock()
                .expect("unregister calls")
                .push(request.session_id.clone());
            Ok(AdvisorySessionUnregistrationResponse {
                session_id: request.session_id,
                closed: true,
            })
        }

        fn fetch_nudges(
            &self,
            _request: AdvisoryFetchRequest,
        ) -> Result<AdvisoryFetchResponse, AtmError> {
            panic!("fetch_nudges is not used by reconnect validation tests")
        }

        fn drain_nudges(
            &self,
            _request: AdvisoryDrainRequest,
        ) -> Result<AdvisoryDrainResponse, AtmError> {
            panic!("drain_nudges is not used by reconnect validation tests")
        }
    }

    impl AtmGraftClient for ReconnectValidationFailureClient {
        fn send_message(&self, _request: SendRequest) -> Result<SendOutcome, AtmError> {
            panic!("send_message is not used by reconnect validation tests")
        }

        fn read_message(&self, _query: ReadQuery) -> Result<ReadOutcome, AtmError> {
            panic!("read_message is not used by reconnect validation tests")
        }

        fn acknowledge_message(&self, _request: AckRequest) -> Result<AckOutcome, AtmError> {
            panic!("acknowledge_message is not used by reconnect validation tests")
        }
    }

    impl GraftSessionClient for ReconnectValidationFailureClient {
        fn supports_live_advisory_stream(&self) -> bool {
            true
        }

        fn open_advisory_stream(
            &self,
            _request: AdvisoryStreamRequest,
        ) -> Result<ActiveAdvisoryStream, AtmError> {
            panic!("open_advisory_stream should not run after validation failure")
        }
    }

    #[test]
    fn reconnect_live_receive_loop_cleans_up_when_reregistration_batch_limit_is_invalid() {
        let tempdir = TempDir::new().expect("tempdir");
        let endpoint_path = tempdir.path().join("reconnect-validation.sock");
        let listener = ListenerOptions::new()
            .name(protocol::daemon_local_ipc_name_from_path(&endpoint_path).expect("endpoint"))
            .create_sync()
            .expect("create listener");
        let server = std::thread::spawn(move || {
            let _stream = listener.accept().expect("accept");
        });
        let stream = LocalSocketStream::connect(
            protocol::daemon_local_ipc_name_from_path(&endpoint_path).expect("endpoint"),
        )
        .expect("connect");
        let client = Arc::new(ReconnectValidationFailureClient {
            unregister_calls: Mutex::new(Vec::new()),
        });
        let snapshot = Arc::new(RwLock::new(SessionSnapshot {
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
            session_id: AdvisorySessionId::new("session-1").expect("session"),
            state: AdvisorySessionState::Registered,
        }));
        let (_stop_tx, stop_rx) = mpsc::channel();
        let mut ctx = LiveReceiveLoopContext {
            client: Arc::clone(&client) as Arc<dyn GraftSessionClient>,
            registration_request: registration_request(),
            advisory_stream: ActiveAdvisoryStream {
                stream,
                request_id: atm_daemon_client::RequestId::new(
                    protocol::next_request_id().into_inner(),
                )
                .expect("request id"),
            },
            limit: AdvisoryBatchLimit::new(8).expect("limit"),
            reconnect_backoff: Duration::from_millis(1),
            snapshot,
            injector: Arc::new(NoopInjector),
            observability: Arc::new(NoopObservability),
            stop_rx,
        };

        let error = reconnect_live_receive_loop(
            &mut ctx,
            AtmError::new(AtmErrorKind::Internal, "stream read failed"),
            Duration::from_millis(1),
        )
        .expect_err("invalid batch limit must fail after cleanup");

        assert!(error.is_validation());
        assert_eq!(
            client
                .unregister_calls
                .lock()
                .expect("unregister calls")
                .len(),
            1
        );
        server.join().expect("join server");
    }
}
