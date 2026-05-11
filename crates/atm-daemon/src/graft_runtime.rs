use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use atm_core::error::AtmError;
use atm_core::graft::{
    GraftNudge, GraftNudgeDrainRequest, GraftNudgeDrainResponse, GraftNudgeFetchRequest,
    GraftNudgeFetchResponse, GraftSessionId, GraftSessionRegistrationRequest,
    GraftSessionRegistrationResponse, GraftSessionUnregistrationRequest,
    GraftSessionUnregistrationResponse,
};
use atm_core::send::SendOutcome;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};

const MAX_GRAFT_SESSIONS: usize = 128;
const MAX_GRAFT_NUDGES_PER_SESSION: usize = 256;

#[derive(Debug, Default)]
pub(crate) struct GraftRuntime {
    state: Mutex<GraftRuntimeState>,
    max_sessions: usize,
    max_nudges_per_session: usize,
}

#[derive(Debug, Default)]
struct GraftRuntimeState {
    sessions: HashMap<GraftSessionId, RegisteredGraftSession>,
}

#[derive(Debug)]
struct RegisteredGraftSession {
    team: TeamName,
    agent: AgentName,
    _pid: u32,
    _started_at: IsoTimestamp,
    _registered_at: IsoTimestamp,
    nudges: VecDeque<GraftNudge>,
    dropped_count: usize,
}

impl GraftRuntime {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(GraftRuntimeState::default()),
            max_sessions: MAX_GRAFT_SESSIONS,
            max_nudges_per_session: MAX_GRAFT_NUDGES_PER_SESSION,
        }
    }

    #[cfg(test)]
    fn with_limits_for_test(max_sessions: usize, max_nudges_per_session: usize) -> Self {
        Self {
            state: Mutex::new(GraftRuntimeState::default()),
            max_sessions,
            max_nudges_per_session,
        }
    }

    pub(crate) fn register_session(
        &self,
        request: GraftSessionRegistrationRequest,
    ) -> Result<GraftSessionRegistrationResponse, AtmError> {
        let mut state = self.lock_state()?;
        if state.sessions.contains_key(&request.session_id) {
            return Err(AtmError::validation(format!(
                "graft session {} is already registered",
                request.session_id
            ))
            .with_recovery(
                "Unregister the existing graft session or choose a new session id before retrying registration.",
            ));
        }
        if state.sessions.len() >= self.max_sessions {
            return Err(AtmError::daemon_unavailable(format!(
                "graft session registration rejected because the daemon session cap {} is exhausted",
                self.max_sessions
            ))
            .with_recovery(
                "Drain and unregister inactive graft sessions before retrying registration.",
            ));
        }

        let registered_at = IsoTimestamp::now();
        state.sessions.insert(
            request.session_id.clone(),
            RegisteredGraftSession {
                team: request.team.clone(),
                agent: request.agent.clone(),
                _pid: request.pid,
                _started_at: request.started_at,
                _registered_at: registered_at,
                nudges: VecDeque::new(),
                dropped_count: 0,
            },
        );

        Ok(GraftSessionRegistrationResponse {
            team: request.team,
            agent: request.agent,
            session_id: request.session_id,
            registered_at,
            queue_capacity: self.max_nudges_per_session,
        })
    }

    pub(crate) fn unregister_session(
        &self,
        request: GraftSessionUnregistrationRequest,
    ) -> Result<GraftSessionUnregistrationResponse, AtmError> {
        let mut state = self.lock_state()?;
        let closed = state.sessions.remove(&request.session_id).is_some();
        Ok(GraftSessionUnregistrationResponse {
            session_id: request.session_id,
            closed,
        })
    }

    pub(crate) fn fetch_nudges(
        &self,
        request: GraftNudgeFetchRequest,
    ) -> Result<GraftNudgeFetchResponse, AtmError> {
        let state = self.lock_state()?;
        let session = state.sessions.get(&request.session_id).ok_or_else(|| {
            AtmError::validation(format!(
                "graft session {} is not registered",
                request.session_id
            ))
            .with_recovery("Register the graft session before fetching daemon-owned nudge state.")
        })?;
        let limit = request.limit.get();
        let nudges = session
            .nudges
            .iter()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let remaining = session.nudges.len().saturating_sub(nudges.len());
        Ok(GraftNudgeFetchResponse {
            session_id: request.session_id,
            nudges,
            remaining,
            dropped_count: session.dropped_count,
        })
    }

    pub(crate) fn drain_nudges(
        &self,
        request: GraftNudgeDrainRequest,
    ) -> Result<GraftNudgeDrainResponse, AtmError> {
        let mut state = self.lock_state()?;
        let session = state.sessions.get_mut(&request.session_id).ok_or_else(|| {
            AtmError::validation(format!(
                "graft session {} is not registered",
                request.session_id
            ))
            .with_recovery("Register the graft session before draining daemon-owned nudge state.")
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
        Ok(GraftNudgeDrainResponse {
            session_id: request.session_id,
            nudges,
            remaining,
            dropped_count: session.dropped_count,
        })
    }

    pub(crate) fn enqueue_nudge_for_recipient(
        &self,
        outcome: &SendOutcome,
    ) -> Result<(), AtmError> {
        let mut state = self.lock_state()?;
        let message = outcome
            .message
            .clone()
            .or_else(|| outcome.summary.clone())
            .unwrap_or_default();
        let nudge = GraftNudge {
            message_id: outcome.message_id,
            from: outcome.sender.clone(),
            message,
            received_at: IsoTimestamp::now(),
            task_id: outcome.task_id.clone(),
        };
        let mut matched = false;
        for (session_id, session) in state.sessions.iter_mut() {
            if session.team == outcome.team && session.agent == outcome.agent {
                matched = true;
                if session.nudges.len() >= self.max_nudges_per_session {
                    session.nudges.pop_front();
                    session.dropped_count = session.dropped_count.saturating_add(1);
                    tracing::warn!(
                        session_id = %session_id,
                        team = %outcome.team,
                        agent = %outcome.agent,
                        cap = self.max_nudges_per_session,
                        dropped_count = session.dropped_count,
                        "graft nudge queue overflowed; oldest nudge dropped"
                    );
                }
                session.nudges.push_back(nudge.clone());
            }
        }
        if matched {
            tracing::debug!(
                team = %outcome.team,
                agent = %outcome.agent,
                message_id = %outcome.message_id,
                "queued graft nudge for registered session"
            );
        }
        Ok(())
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, GraftRuntimeState>, AtmError> {
        self.state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("graft session state lock poisoned"))
    }
}

#[cfg(test)]
mod tests {
    use super::GraftRuntime;
    use atm_core::graft::{
        GraftBatchLimit, GraftNudgeDrainRequest, GraftNudgeFetchRequest, GraftSessionId,
        GraftSessionRegistrationRequest, GraftSessionUnregistrationRequest,
    };
    use atm_core::send::{SendOutcome, WarningEntry};
    use atm_core::types::{CommandAction, IsoTimestamp};

    fn registration_request() -> GraftSessionRegistrationRequest {
        GraftSessionRegistrationRequest {
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
            session_id: GraftSessionId::new("session-1").expect("session id"),
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
            message_id: atm_core::schema::LegacyMessageId::new(),
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
        let runtime = GraftRuntime::with_limits_for_test(2, 2);
        let response = runtime
            .register_session(registration_request())
            .expect("register session");
        assert_eq!(response.queue_capacity, 2);

        let closed = runtime
            .unregister_session(GraftSessionUnregistrationRequest {
                session_id: GraftSessionId::new("session-1").expect("session id"),
            })
            .expect("unregister session");
        assert!(closed.closed);
    }

    #[test]
    fn fetch_does_not_drain_and_drain_clears_in_queue_order() {
        let runtime = GraftRuntime::with_limits_for_test(2, 4);
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
            .fetch_nudges(GraftNudgeFetchRequest {
                session_id: request.session_id.clone(),
                limit: GraftBatchLimit::new(8).expect("limit"),
            })
            .expect("fetch");
        assert_eq!(fetch.nudges.len(), 2);
        assert_eq!(fetch.nudges[0].message, "first");
        assert_eq!(fetch.nudges[1].message, "second");
        assert_eq!(fetch.remaining, 0);

        let drain = runtime
            .drain_nudges(GraftNudgeDrainRequest {
                session_id: request.session_id.clone(),
                limit: GraftBatchLimit::new(1).expect("limit"),
            })
            .expect("drain");
        assert_eq!(drain.nudges.len(), 1);
        assert_eq!(drain.nudges[0].message, "first");
        assert_eq!(drain.remaining, 1);

        let final_drain = runtime
            .drain_nudges(GraftNudgeDrainRequest {
                session_id: request.session_id,
                limit: GraftBatchLimit::new(8).expect("limit"),
            })
            .expect("final drain");
        assert_eq!(final_drain.nudges.len(), 1);
        assert_eq!(final_drain.nudges[0].message, "second");
        assert_eq!(final_drain.remaining, 0);
    }

    #[test]
    fn overflow_drops_oldest_and_reports_dropped_count() {
        let runtime = GraftRuntime::with_limits_for_test(2, 2);
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
        runtime
            .enqueue_nudge_for_recipient(&send_outcome("third"))
            .expect("enqueue third");

        let drain = runtime
            .drain_nudges(GraftNudgeDrainRequest {
                session_id: request.session_id,
                limit: GraftBatchLimit::new(8).expect("limit"),
            })
            .expect("drain");
        assert_eq!(drain.dropped_count, 1);
        assert_eq!(drain.nudges.len(), 2);
        assert_eq!(drain.nudges[0].message, "second");
        assert_eq!(drain.nudges[1].message, "third");
    }
}
