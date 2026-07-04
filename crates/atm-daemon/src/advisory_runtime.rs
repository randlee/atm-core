use std::collections::{HashMap, VecDeque};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::Duration;

use atm_core::PostSendHookEvent;
use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::graft::{
    AdvisoryDrainRequest, AdvisoryDrainResponse, AdvisoryEvent, AdvisoryFetchRequest,
    AdvisoryFetchResponse, AdvisoryMessage, AdvisorySessionId, AdvisorySessionRegistrationRequest,
    AdvisorySessionRegistrationResponse, AdvisorySessionUnregistrationRequest,
    AdvisorySessionUnregistrationResponse, AdvisoryStreamRequest, AdvisoryStreamResponse,
};
use atm_core::protocol::ResponseEnvelope;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};

#[cfg(test)]
use crate::DaemonSubsystem;
use crate::SubsystemObservability;
use crate::daemon_runtime_observability::DaemonEvent;

const MAX_ADVISORY_SESSIONS: usize = 128;
const MAX_ADVISORY_EVENTS_PER_SESSION: usize = 256;
const STREAM_IDLE_WAIT: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub(crate) struct AdvisoryRuntime {
    // Advisory fetch/drain/read operations are read-heavy and independent per session, so an
    // RwLock keeps concurrent readers off the registration/drain write path without requiring
    // broader actor-style coordination for this bounded in-process runtime cache.
    state: RwLock<AdvisoryRuntimeState>,
    max_sessions: usize,
    max_nudges_per_session: usize,
    observability: SubsystemObservability,
}

#[derive(Debug, Default)]
struct AdvisoryRuntimeState {
    sessions: HashMap<AdvisorySessionId, RegisteredAdvisorySession>,
}

#[derive(Debug)]
struct RegisteredAdvisorySession {
    team: TeamName,
    agent: AgentName,
    _pid: u32,
    _started_at: IsoTimestamp,
    _registered_at: IsoTimestamp,
    nudges: VecDeque<AdvisoryEvent>,
    dropped_count: usize,
}

impl AdvisoryRuntime {
    pub(crate) fn new_with_observability(observability: SubsystemObservability) -> Self {
        Self {
            state: RwLock::new(AdvisoryRuntimeState::default()),
            max_sessions: MAX_ADVISORY_SESSIONS,
            max_nudges_per_session: MAX_ADVISORY_EVENTS_PER_SESSION,
            observability,
        }
    }

    #[cfg(test)]
    fn with_limits_for_test(max_sessions: usize, max_nudges_per_session: usize) -> Self {
        Self {
            state: RwLock::new(AdvisoryRuntimeState::default()),
            max_sessions,
            max_nudges_per_session,
            observability: SubsystemObservability::disabled(DaemonSubsystem::AdvisoryRuntime),
        }
    }

    pub(crate) fn register_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
        let mut state = self.lock_state_write()?;
        if state.sessions.contains_key(&request.session_id) {
            let event = self
                .observability
                .event(
                    "register_session",
                    "rejected",
                    "advisory session registration reused an existing session id",
                )
                .with_team(request.team.clone())
                .with_agent(request.agent.clone());
            self.observability.emit_event_or_warn(event);
            return Err(AtmError::daemon_advisory_session_already_registered(format!(
                "advisory session {} is already registered",
                request.session_id
            ))
            .with_recovery(
                "Unregister the existing advisory session or choose a new session id before retrying registration.",
            ));
        }
        if state.sessions.len() >= self.max_sessions {
            let event = self
                .observability
                .event(
                    "register_session",
                    "rejected",
                    "advisory session registration hit the bounded session cap",
                )
                .with_team(request.team.clone())
                .with_agent(request.agent.clone());
            self.observability.emit_event_or_warn(event);
            return Err(AtmError::daemon_unavailable(format!(
                "advisory session registration rejected because the daemon session cap {} is exhausted",
                self.max_sessions
            ))
            .with_recovery(
                "Drain and unregister inactive advisory sessions before retrying registration.",
            ));
        }

        let registered_at = IsoTimestamp::now();
        state.sessions.insert(
            request.session_id.clone(),
            RegisteredAdvisorySession {
                team: request.team.clone(),
                agent: request.agent.clone(),
                _pid: request.pid,
                _started_at: request.started_at,
                _registered_at: registered_at,
                nudges: VecDeque::new(),
                dropped_count: 0,
            },
        );
        let event = self
            .observability
            .event("register_session", "ok", "advisory session registered")
            .with_team(request.team.clone())
            .with_agent(request.agent.clone());
        self.observability.emit_event_or_warn(event);

        Ok(AdvisorySessionRegistrationResponse {
            team: request.team,
            agent: request.agent,
            session_id: request.session_id,
            registered_at,
            queue_capacity: self.max_nudges_per_session,
        })
    }

    pub(crate) fn unregister_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
        let mut state = self.lock_state_write()?;
        let closed = state.sessions.remove(&request.session_id).is_some();
        self.observability.emit_or_warn(
            "unregister_session",
            if closed { "ok" } else { "noop" },
            if closed {
                "advisory session unregistered"
            } else {
                "advisory session unregistration found no registered session"
            },
        );
        Ok(AdvisorySessionUnregistrationResponse {
            session_id: request.session_id,
            closed,
        })
    }

    pub(crate) fn fetch_nudges(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError> {
        let state = self.lock_state_read()?;
        let session = state.sessions.get(&request.session_id).ok_or_else(|| {
            AtmError::daemon_advisory_session_not_registered(format!(
                "advisory session {} is not registered",
                request.session_id
            ))
        })?;
        let limit = request.limit.get();
        let nudges = session
            .nudges
            .iter()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let remaining = session.nudges.len().saturating_sub(nudges.len());
        Ok(AdvisoryFetchResponse {
            session_id: request.session_id,
            nudges,
            remaining,
            dropped_count: session.dropped_count,
        })
    }

    pub(crate) fn drain_nudges(
        &self,
        request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError> {
        let mut state = self.lock_state_write()?;
        let session = state.sessions.get_mut(&request.session_id).ok_or_else(|| {
            AtmError::daemon_advisory_session_not_registered(format!(
                "advisory session {} is not registered",
                request.session_id
            ))
        })?;
        let limit = request.limit.get();
        let mut nudges = Vec::with_capacity(limit.min(session.nudges.len()));
        for _ in 0..limit {
            let Some(nudge) = session.nudges.pop_front() else {
                break;
            };
            nudges.push(nudge);
        }
        let remaining = session.nudges.len();
        Ok(AdvisoryDrainResponse {
            session_id: request.session_id,
            nudges,
            remaining,
            dropped_count: session.dropped_count,
        })
    }

    pub(crate) fn stream_nudges(
        &self,
        request: AdvisoryStreamRequest,
        sink: &mut dyn boundary::AdvisoryStreamSink,
    ) -> Result<(), AtmError> {
        let drain_request = AdvisoryDrainRequest {
            session_id: request.registration.session_id.clone(),
            limit: request.limit,
        };
        loop {
            if sink.stop_requested() {
                return Ok(());
            }
            match self.drain_nudges(drain_request.clone()) {
                Ok(batch) => {
                    if batch.nudges.is_empty() {
                        if sink.stop_requested() {
                            return Ok(());
                        }
                        thread::sleep(STREAM_IDLE_WAIT);
                        continue;
                    }
                    sink.emit(ResponseEnvelope::AdvisoryStream(AdvisoryStreamResponse {
                        session_id: batch.session_id,
                        nudges: batch.nudges,
                        remaining: batch.remaining,
                        dropped_count: batch.dropped_count,
                    }))?;
                }
                Err(error)
                    if error.code
                        == atm_core::error_codes::AtmErrorCode::DaemonAdvisorySessionNotRegistered =>
                {
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn deliver_post_send_impl(&self, event: &PostSendHookEvent) -> Result<(), AtmError> {
        let mut state = self.lock_state_write()?;
        let nudge = advisory_nudge_from_event(event)?;
        let mut matched = false;
        let mut overflowed = false;
        for (session_id, session) in state.sessions.iter_mut() {
            if session.team == event.recipient_team && session.agent == event.recipient {
                matched = true;
                if let EnqueueSessionResult::Overflow =
                    self.enqueue_nudge_for_session(session_id, session, event, &nudge)
                {
                    overflowed = true;
                }
            }
        }
        self.emit_enqueue_outcome(matched, overflowed, event);
        if !matched {
            return Err(AtmError::new_with_code(
                atm_core::error_codes::AtmErrorCode::PostSendGraftUnavailable,
                atm_core::error::AtmErrorKind::Validation,
                format!(
                    "recipient {}@{} has no active graft advisory session",
                    event.recipient, event.recipient_team
                ),
            )
            .with_recovery(
                "Start or reconnect the graft-backed recipient session before retrying if a fresh nudge is still required.",
            ));
        }
        if overflowed {
            return Err(AtmError::new_with_code(
                atm_core::error_codes::AtmErrorCode::PostSendAdvisoryDeliveryFailed,
                atm_core::error::AtmErrorKind::DaemonUnavailable,
                "advisory queue is full; at least one registered graft session did not receive the post-send event",
            )
            .with_recovery(
                "Drain or fetch graft advisory events from the active session before retrying if a fresh nudge is still required.",
            ));
        }
        Ok(())
    }

    fn enqueue_nudge_for_session(
        &self,
        session_id: &atm_core::graft::AdvisorySessionId,
        session: &mut RegisteredAdvisorySession,
        event: &PostSendHookEvent,
        nudge: &AdvisoryEvent,
    ) -> EnqueueSessionResult {
        if session.nudges.len() < self.max_nudges_per_session {
            session.nudges.push_back(nudge.clone());
            return EnqueueSessionResult::Queued;
        }
        session.dropped_count = session.dropped_count.saturating_add(1);
        self.emit_queue_overflow_event(session_id, session.dropped_count, event);
        EnqueueSessionResult::Overflow
    }

    fn emit_queue_overflow_event(
        &self,
        session_id: &atm_core::graft::AdvisorySessionId,
        dropped_count: usize,
        post_send: &PostSendHookEvent,
    ) {
        let daemon_event = advisory_runtime_event(
            self.observability.event(
                "enqueue_nudge",
                "degraded",
                "advisory queue rejected an event because the bounded session queue is full",
            ),
            post_send,
        );
        self.observability.emit_event_or_warn(daemon_event);
        tracing::debug!(
            session_id = %session_id,
            team = %post_send.recipient_team,
            agent = %post_send.recipient,
            cap = self.max_nudges_per_session,
            dropped_count,
            "advisory queue rejected an event because the bounded session queue is full"
        );
    }

    fn emit_enqueue_outcome(&self, matched: bool, overflowed: bool, post_send: &PostSendHookEvent) {
        let message = if overflowed {
            "advisory runtime enqueued at least one nudge and dropped at least one due to queue pressure"
        } else if matched {
            "advisory runtime queued a nudge for a registered session"
        } else {
            "advisory runtime found no registered graft advisory session for the post-send event"
        };
        let outcome_label = if overflowed {
            "degraded"
        } else if matched {
            "ok"
        } else {
            "noop"
        };
        let daemon_event = advisory_runtime_event(
            self.observability
                .event("enqueue_nudge", outcome_label, message)
                .with_recipient(post_send.recipient.clone())
                .with_sender(post_send.sender.clone()),
            post_send,
        );
        self.observability.emit_event_or_warn(daemon_event);
        tracing::debug!(
            team = %post_send.recipient_team,
            agent = %post_send.recipient,
            message_id = %post_send.message_id,
            "queued advisory event for registered session"
        );
    }

    fn lock_state_read(&self) -> Result<RwLockReadGuard<'_, AdvisoryRuntimeState>, AtmError> {
        self.state
            .read()
            .map_err(|_| AtmError::daemon_unavailable("advisory session state lock poisoned"))
            .map_err(|error| {
                error.with_recovery(
                    "Restart atm-daemon; advisory session state can no longer be trusted after the poisoned lock.",
                )
            })
    }

    fn lock_state_write(&self) -> Result<RwLockWriteGuard<'_, AdvisoryRuntimeState>, AtmError> {
        self.state
            .write()
            .map_err(|_| AtmError::daemon_unavailable("advisory session state lock poisoned"))
            .map_err(|error| {
                error.with_recovery(
                    "Restart atm-daemon; advisory session state can no longer be trusted after the poisoned lock.",
                )
            })
    }
}

enum EnqueueSessionResult {
    Queued,
    Overflow,
}

fn advisory_nudge_from_event(event: &PostSendHookEvent) -> Result<AdvisoryEvent, AtmError> {
    Ok(AdvisoryEvent {
        message_id: event.message_id,
        from: event.sender.clone(),
        message: AdvisoryMessage::new(event.message.clone())?,
        received_at: IsoTimestamp::now(),
        task_id: event.task_id.clone(),
    })
}

fn advisory_runtime_event(event: DaemonEvent, post_send: &PostSendHookEvent) -> DaemonEvent {
    let mut event = event
        .with_team(post_send.recipient_team.clone())
        .with_agent(post_send.recipient.clone())
        .with_message_id(post_send.message_id);
    if let Some(task_id) = post_send.task_id.clone() {
        event = event.with_task_id(task_id);
    }
    event
}

impl atm_core::boundary::sealed::Sealed for AdvisoryRuntime {}

impl atm_core::boundary::GraftPostSendPort for AdvisoryRuntime {
    fn deliver_post_send(&self, event: &PostSendHookEvent) -> Result<(), AtmError> {
        self.deliver_post_send_impl(event)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::AdvisoryRuntime;
    use atm_core::PostSendHookEvent;
    use atm_core::boundary::{self, GraftPostSendPort};
    use atm_core::error::AtmError;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::graft::{
        AdvisoryBatchLimit, AdvisoryDrainRequest, AdvisoryFetchRequest, AdvisorySessionId,
        AdvisorySessionRegistrationRequest, AdvisorySessionUnregistrationRequest,
        AdvisoryStreamRequest, AdvisoryStreamResponse,
    };
    use atm_core::protocol::ResponseEnvelope;
    use atm_core::types::IsoTimestamp;

    fn registration_request() -> AdvisorySessionRegistrationRequest {
        AdvisorySessionRegistrationRequest {
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
            session_id: AdvisorySessionId::new("session-1").expect("session id"),
            pid: 4242,
            started_at: IsoTimestamp::now(),
        }
    }

    fn post_send_event(body: &str) -> PostSendHookEvent {
        PostSendHookEvent {
            sender: "sender".parse().expect("sender"),
            sender_team: "sender-team".parse().expect("sender team"),
            recipient: "test-agent".parse().expect("recipient"),
            recipient_team: "test-team".parse().expect("team"),
            message_id: atm_core::schema::AtmMessageId::new(),
            message: body.to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id: None,
        }
    }

    #[test]
    fn registration_and_unregistration_round_trip() {
        let runtime = AdvisoryRuntime::with_limits_for_test(2, 2);
        let response = runtime
            .register_session(registration_request())
            .expect("register session");
        assert_eq!(response.queue_capacity, 2);

        let closed = runtime
            .unregister_session(AdvisorySessionUnregistrationRequest {
                session_id: AdvisorySessionId::new("session-1").expect("session id"),
            })
            .expect("unregister session");
        assert!(closed.closed);
    }

    #[test]
    fn fetch_does_not_drain_and_drain_clears_in_queue_order() {
        let runtime = AdvisoryRuntime::with_limits_for_test(2, 4);
        let request = registration_request();
        runtime
            .register_session(request.clone())
            .expect("register session");
        runtime
            .deliver_post_send(&post_send_event("first"))
            .expect("enqueue first");
        runtime
            .deliver_post_send(&post_send_event("second"))
            .expect("enqueue second");

        let fetch = runtime
            .fetch_nudges(AdvisoryFetchRequest {
                session_id: request.session_id.clone(),
                limit: AdvisoryBatchLimit::new(8).expect("limit"),
            })
            .expect("fetch");
        assert_eq!(fetch.nudges.len(), 2);
        assert_eq!(fetch.nudges[0].message, "first");
        assert_eq!(fetch.nudges[1].message, "second");
        assert_eq!(fetch.remaining, 0);

        let drain = runtime
            .drain_nudges(AdvisoryDrainRequest {
                session_id: request.session_id.clone(),
                limit: AdvisoryBatchLimit::new(1).expect("limit"),
            })
            .expect("drain");
        assert_eq!(drain.nudges.len(), 1);
        assert_eq!(drain.nudges[0].message, "first");
        assert_eq!(drain.remaining, 1);

        let final_drain = runtime
            .drain_nudges(AdvisoryDrainRequest {
                session_id: request.session_id,
                limit: AdvisoryBatchLimit::new(8).expect("limit"),
            })
            .expect("final drain");
        assert_eq!(final_drain.nudges.len(), 1);
        assert_eq!(final_drain.nudges[0].message, "second");
        assert_eq!(final_drain.remaining, 0);
    }

    #[test]
    fn overflow_rejects_new_nudge_and_reports_dropped_count() {
        let runtime = AdvisoryRuntime::with_limits_for_test(2, 2);
        let request = registration_request();
        runtime
            .register_session(request.clone())
            .expect("register session");
        runtime
            .deliver_post_send(&post_send_event("first"))
            .expect("enqueue first");
        runtime
            .deliver_post_send(&post_send_event("second"))
            .expect("enqueue second");
        let error = runtime
            .deliver_post_send(&post_send_event("third"))
            .expect_err("overflow should reject new event");
        assert_eq!(error.code, AtmErrorCode::PostSendAdvisoryDeliveryFailed);

        let drain = runtime
            .drain_nudges(AdvisoryDrainRequest {
                session_id: request.session_id,
                limit: AdvisoryBatchLimit::new(8).expect("limit"),
            })
            .expect("drain");
        assert_eq!(drain.dropped_count, 1);
        assert_eq!(drain.nudges.len(), 2);
        assert_eq!(drain.nudges[0].message, "first");
        assert_eq!(drain.nudges[1].message, "second");
    }

    #[derive(Debug)]
    struct NotifyingStreamSink {
        batch_tx: mpsc::Sender<AdvisoryStreamResponse>,
    }

    impl boundary::AdvisoryStreamSink for NotifyingStreamSink {
        fn emit(&mut self, response: ResponseEnvelope) -> Result<(), AtmError> {
            match response {
                ResponseEnvelope::AdvisoryStream(batch) => self
                    .batch_tx
                    .send(batch)
                    .map_err(|_| AtmError::daemon_unavailable("test stream sink disconnected")),
                other => Err(AtmError::validation(format!(
                    "unexpected stream response in advisory runtime test: {other:?}"
                ))),
            }
        }
    }

    #[test]
    fn advisory_stream_emits_nudges_and_exits_after_unregistration() {
        let runtime = Arc::new(AdvisoryRuntime::with_limits_for_test(2, 4));
        let request = registration_request();
        runtime
            .register_session(request.clone())
            .expect("register session");
        let (batch_tx, batch_rx) = mpsc::channel();
        let runtime_for_stream = Arc::clone(&runtime);
        let request_for_stream = request.clone();

        let join = std::thread::spawn(move || {
            let mut sink = NotifyingStreamSink { batch_tx };
            runtime_for_stream.stream_nudges(
                AdvisoryStreamRequest {
                    registration: request_for_stream,
                    limit: AdvisoryBatchLimit::new(8).expect("limit"),
                },
                &mut sink,
            )
        });

        runtime
            .deliver_post_send(&post_send_event("streamed"))
            .expect("enqueue nudge");

        let batch = batch_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive advisory batch");
        assert_eq!(batch.session_id, request.session_id);
        assert_eq!(batch.nudges.len(), 1);
        assert_eq!(batch.nudges[0].message, "streamed");
        assert_eq!(batch.remaining, 0);

        runtime
            .unregister_session(AdvisorySessionUnregistrationRequest {
                session_id: request.session_id,
            })
            .expect("unregister session");

        join.join()
            .expect("join advisory stream thread")
            .expect("stream loop should exit cleanly");
    }

    #[test]
    fn unregistered_recipient_returns_graft_unavailable() {
        let runtime = AdvisoryRuntime::with_limits_for_test(2, 2);
        let error = runtime
            .deliver_post_send(&post_send_event("hello"))
            .expect_err("missing graft session should surface as unavailable");
        assert_eq!(error.code, AtmErrorCode::PostSendGraftUnavailable);
    }
}
