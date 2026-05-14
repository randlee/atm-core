use std::collections::{HashMap, VecDeque};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::Duration;

use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::graft::{
    AdvisoryDrainRequest, AdvisoryDrainResponse, AdvisoryEvent, AdvisoryFetchRequest,
    AdvisoryFetchResponse, AdvisoryMessage, AdvisorySessionId, AdvisorySessionRegistrationRequest,
    AdvisorySessionRegistrationResponse, AdvisorySessionUnregistrationRequest,
    AdvisorySessionUnregistrationResponse, AdvisoryStreamRequest, AdvisoryStreamResponse,
};
use atm_core::protocol::ResponseEnvelope;
use atm_core::send::SendOutcome;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};

use crate::{DaemonSubsystem, SubsystemObservability};

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
    #[allow(dead_code)]
    pub(crate) fn new() -> Self {
        Self::new_with_observability(SubsystemObservability::disabled(
            DaemonSubsystem::AdvisoryRuntime,
        ))
    }

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
            let _ = self.observability.emit_event(event);
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
            let _ = self.observability.emit_event(event);
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
        let _ = self.observability.emit_event(event);

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
        let _ = self.observability.emit(
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

    pub(crate) fn enqueue_nudge_for_recipient(
        &self,
        outcome: &SendOutcome,
    ) -> Result<(), AtmError> {
        let mut state = self.lock_state_write()?;
        let message = outcome
            .message
            .clone()
            .or_else(|| outcome.summary.clone())
            .unwrap_or_default();
        let nudge = AdvisoryEvent {
            message_id: outcome.message_id,
            from: outcome.sender.clone(),
            message: AdvisoryMessage::new(message)?,
            received_at: IsoTimestamp::now(),
            task_id: outcome.task_id.clone(),
        };
        let mut matched = false;
        let mut overflowed = false;
        for (session_id, session) in state.sessions.iter_mut() {
            if session.team == outcome.team && session.agent == outcome.agent {
                matched = true;
                if session.nudges.len() >= self.max_nudges_per_session {
                    session.dropped_count = session.dropped_count.saturating_add(1);
                    overflowed = true;
                    let mut event = self
                        .observability
                        .event(
                            "enqueue_nudge",
                            "degraded",
                            "advisory queue rejected an event because the bounded session queue is full",
                        )
                        .with_team(outcome.team.clone())
                        .with_agent(outcome.agent.clone());
                    event = event.with_message_id(outcome.message_id);
                    if let Some(task_id) = outcome.task_id.clone() {
                        event = event.with_task_id(task_id);
                    }
                    let _ = self.observability.emit_event(event);
                    tracing::debug!(
                        session_id = %session_id,
                        team = %outcome.team,
                        agent = %outcome.agent,
                        cap = self.max_nudges_per_session,
                        dropped_count = session.dropped_count,
                        "advisory queue rejected an event because the bounded session queue is full"
                    );
                    continue;
                }
                session.nudges.push_back(nudge.clone());
            }
        }
        if matched {
            let mut event = self
                .observability
                .event(
                    "enqueue_nudge",
                    if overflowed { "degraded" } else { "ok" },
                    if overflowed {
                        "advisory runtime enqueued at least one nudge and dropped at least one due to queue pressure"
                    } else {
                        "advisory runtime queued a nudge for a registered session"
                    },
                )
                .with_team(outcome.team.clone())
                .with_agent(outcome.agent.clone())
                .with_recipient(outcome.agent.clone())
                .with_sender(outcome.sender.clone());
            event = event.with_message_id(outcome.message_id);
            if let Some(task_id) = outcome.task_id.clone() {
                event = event.with_task_id(task_id);
            }
            let _ = self.observability.emit_event(event);
            tracing::debug!(
                team = %outcome.team,
                agent = %outcome.agent,
                message_id = %outcome.message_id,
                "queued advisory event for registered session"
            );
        }
        if overflowed {
            return Err(
                AtmError::daemon_unavailable(
                    "advisory queue is full; at least one registered session did not receive the event",
                )
                .with_recovery(
                    "Drain or fetch advisory events from the active session before retrying the send operation.",
                ),
            );
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::AdvisoryRuntime;
    use atm_core::boundary;
    use atm_core::error::AtmError;
    use atm_core::graft::{
        AdvisoryBatchLimit, AdvisoryDrainRequest, AdvisoryFetchRequest, AdvisorySessionId,
        AdvisorySessionRegistrationRequest, AdvisorySessionUnregistrationRequest,
        AdvisoryStreamRequest, AdvisoryStreamResponse,
    };
    use atm_core::protocol::ResponseEnvelope;
    use atm_core::send::{SendOutcome, WarningEntry};
    use atm_core::types::{CommandAction, IsoTimestamp};

    fn registration_request() -> AdvisorySessionRegistrationRequest {
        AdvisorySessionRegistrationRequest {
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
            session_id: AdvisorySessionId::new("session-1").expect("session id"),
            pid: 4242,
            started_at: IsoTimestamp::now(),
        }
    }

    fn send_outcome(body: &str) -> SendOutcome {
        SendOutcome {
            action: CommandAction::Send,
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
            sender: "sender".parse().expect("sender"),
            outcome: "sent".to_string(),
            message_id: atm_core::schema::AtmMessageId::new(),
            requires_ack: false,
            task_id: None,
            summary: Some("summary".to_string()),
            message: Some(body.to_string()),
            warnings: Vec::<WarningEntry>::new(),
            dry_run: false,
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
            .enqueue_nudge_for_recipient(&send_outcome("first"))
            .expect("enqueue first");
        runtime
            .enqueue_nudge_for_recipient(&send_outcome("second"))
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
            .enqueue_nudge_for_recipient(&send_outcome("first"))
            .expect("enqueue first");
        runtime
            .enqueue_nudge_for_recipient(&send_outcome("second"))
            .expect("enqueue second");
        let error = runtime
            .enqueue_nudge_for_recipient(&send_outcome("third"))
            .expect_err("overflow should reject new event");
        assert_eq!(
            error.message,
            "advisory queue is full; at least one registered session did not receive the event"
        );

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
            .enqueue_nudge_for_recipient(&send_outcome("streamed"))
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
}
