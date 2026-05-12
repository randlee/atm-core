//! Thin graft-facing daemon client contracts shared by embedded host agents.

use std::fmt;
use std::num::NonZeroUsize;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

use crate::ack::{AckOutcome, AckRequest};
use crate::error::AtmError;
use crate::read::{ReadOutcome, ReadQuery};
use crate::schema::AtmMessageId;
use crate::send::{SendOutcome, SendRequest};
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

pub const MAX_GRAFT_SESSION_ID_BYTES: usize = 128;
pub const MAX_GRAFT_BATCH_LIMIT: usize = 256;
pub const MAX_GRAFT_NUDGE_MESSAGE_BYTES: usize = 4096;

/// Open unary client surface for embedded ATM consumers.
///
/// This trait is intentionally not sealed. `atm-graft` must be able to
/// implement the concrete same-host client in a separate crate without taking
/// a Rust dependency on `atm-daemon`.
pub trait AtmGraftClient: Send + Sync {
    /// Execute one send-shaped ATM compose request.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the underlying daemon-backed send path cannot
    /// complete successfully.
    fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError>;

    /// Execute one ATM read request through the same daemon-backed semantic
    /// path used by the retained CLI.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the read request cannot be delivered or the
    /// daemon returns a typed failure.
    fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError>;

    /// Execute one send-shaped ATM acknowledgement request.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the acknowledgement request cannot be
    /// completed successfully.
    fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError>;
}

/// Open session-facing contract for embedded graft runtimes.
///
/// This trait is intentionally not sealed. `atm-graft` must be able to own
/// the concrete `GraftSession` implementation in a separate crate.
pub trait GraftSessionPort: Send + Sync {
    /// Register one active embedded graft session with the daemon runtime.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon cannot accept or persist the
    /// session registration.
    fn register_session(
        &self,
        request: GraftSessionRegistrationRequest,
    ) -> Result<GraftSessionRegistrationResponse, AtmError>;

    /// Unregister one active embedded graft session from the daemon runtime.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon cannot safely close the session.
    fn unregister_session(
        &self,
        request: GraftSessionUnregistrationRequest,
    ) -> Result<GraftSessionUnregistrationResponse, AtmError>;

    /// Fetch pending daemon-owned graft nudges without draining them.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon cannot project the current nudge
    /// queue state for the active session.
    fn fetch_nudges(
        &self,
        request: GraftNudgeFetchRequest,
    ) -> Result<GraftNudgeFetchResponse, AtmError>;

    /// Drain pending daemon-owned graft nudges for one active session.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon cannot safely hand off and clear
    /// the queued nudge state for the active session.
    fn drain_nudges(
        &self,
        request: GraftNudgeDrainRequest,
    ) -> Result<GraftNudgeDrainResponse, AtmError>;
}

/// Explicit lifecycle states for one graft session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraftSessionState {
    Inactive,
    Connecting,
    Registered,
    Disconnected,
    Closed,
    CloseFailed,
}

/// Public lifecycle projection for one graft session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftSession {
    pub team: TeamName,
    pub agent: AgentName,
    pub session_id: GraftSessionId,
    pub state: GraftSessionState,
}

/// Stable identifier for one active graft session.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct GraftSessionId(String);

impl GraftSessionId {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the supplied session id is blank, only
    /// whitespace, or exceeds [`MAX_GRAFT_SESSION_ID_BYTES`].
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            // AtmError::validation uses MessageValidationFailed, the shared boundary-validation
            // error code across all ATM subsystems; no graft-specific code is needed.
            return Err(AtmError::validation("graft session id must not be blank").with_recovery(
                "Populate a stable non-empty graft session id before calling the graft session runtime.",
            ));
        }
        if value.len() > MAX_GRAFT_SESSION_ID_BYTES {
            return Err(AtmError::validation(format!(
                "graft session id must be at most {MAX_GRAFT_SESSION_ID_BYTES} bytes"
            ))
            .with_recovery(
                "Shorten the graft session id before calling the graft session runtime.",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for GraftSessionId {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for GraftSessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for GraftSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Bounded nudge batch size requested by an embedded graft consumer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct GraftBatchLimit(NonZeroUsize);

impl GraftBatchLimit {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the limit is zero or exceeds
    /// [`MAX_GRAFT_BATCH_LIMIT`].
    pub fn new(value: usize) -> Result<Self, AtmError> {
        let value = NonZeroUsize::new(value).ok_or_else(|| {
            // AtmError::validation uses MessageValidationFailed, the shared boundary-validation
            // error code across all ATM subsystems; no graft-specific code is needed.
            AtmError::validation("graft batch limit must be greater than zero").with_recovery(
                "Use a positive graft nudge batch limit before calling the daemon graft queue surface.",
            )
        })?;
        if value.get() > MAX_GRAFT_BATCH_LIMIT {
            return Err(AtmError::validation(format!(
                "graft batch limit must not exceed {MAX_GRAFT_BATCH_LIMIT}"
            ))
            .with_recovery(
                "Lower the graft nudge batch limit to the documented daemon queue ceiling before calling the graft queue surface.",
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> usize {
        self.0.get()
    }
}

/// Daemon registration request for one embedded graft session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftSessionRegistrationRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub session_id: GraftSessionId,
    pub pid: u32,
    pub started_at: IsoTimestamp,
}

/// Daemon response after accepting one embedded graft session registration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftSessionRegistrationResponse {
    pub team: TeamName,
    pub agent: AgentName,
    pub session_id: GraftSessionId,
    pub registered_at: IsoTimestamp,
    pub queue_capacity: usize,
}

/// Dedicated advisory-stream request for one active embedded graft session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftAdvisoryStreamRequest {
    pub registration: GraftSessionRegistrationRequest,
    pub limit: GraftBatchLimit,
}

/// One emitted advisory-stream batch projected to an embedded host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftAdvisoryStreamResponse {
    pub session_id: GraftSessionId,
    pub nudges: Vec<NudgeEvent>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

/// Daemon unregistration request for one embedded graft session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftSessionUnregistrationRequest {
    pub session_id: GraftSessionId,
}

/// Daemon response after closing one embedded graft session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftSessionUnregistrationResponse {
    pub session_id: GraftSessionId,
    pub closed: bool,
}

/// Validated nudge message text.
#[derive(Clone, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct NudgeMessage(String);

impl NudgeMessage {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the message exceeds
    /// [`MAX_GRAFT_NUDGE_MESSAGE_BYTES`] UTF-8 bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.len() > MAX_GRAFT_NUDGE_MESSAGE_BYTES {
            return Err(AtmError::validation(format!(
                "graft nudge message must be at most {MAX_GRAFT_NUDGE_MESSAGE_BYTES} bytes"
            ))
            .with_recovery(
                "Shorten the daemon-owned advisory message before projecting it to the embedded graft host.",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for NudgeMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Debug for NudgeMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NudgeMessage")
            .field("bytes", &self.0.len())
            .field("redacted", &true)
            .finish()
    }
}

/// One daemon-originated nudge event projected to an embedded host.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NudgeEvent {
    pub message_id: AtmMessageId,
    pub from: AgentName,
    /// Advisory graft payload text.
    pub message: NudgeMessage,
    pub received_at: IsoTimestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
}

impl NudgeEvent {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the advisory message exceeds
    /// [`MAX_GRAFT_NUDGE_MESSAGE_BYTES`] UTF-8 bytes.
    pub fn new(
        message_id: AtmMessageId,
        from: AgentName,
        message: impl Into<String>,
        received_at: IsoTimestamp,
        task_id: Option<TaskId>,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            message_id,
            from,
            message: NudgeMessage::new(message)?,
            received_at,
            task_id,
        })
    }

    pub fn message(&self) -> &str {
        self.message.as_str()
    }
}

impl fmt::Debug for NudgeEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NudgeEvent")
            .field("message_id", &self.message_id)
            .field("from", &self.from)
            .field("message", &self.message)
            .field("received_at", &self.received_at)
            .field("task_id", &self.task_id)
            .finish()
    }
}

/// Fetch request for the current daemon-owned pending-nudge snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftNudgeFetchRequest {
    pub session_id: GraftSessionId,
    pub limit: GraftBatchLimit,
}

/// Fetch response for the current daemon-owned pending-nudge snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftNudgeFetchResponse {
    pub session_id: GraftSessionId,
    pub nudges: Vec<NudgeEvent>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

/// Drain request for one active embedded graft session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftNudgeDrainRequest {
    pub session_id: GraftSessionId,
    pub limit: GraftBatchLimit,
}

/// Drain response for one active embedded graft session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftNudgeDrainResponse {
    pub session_id: GraftSessionId,
    pub nudges: Vec<NudgeEvent>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use serde::{Deserialize, Serialize};

    use super::{
        GraftBatchLimit, GraftNudgeDrainRequest, GraftNudgeDrainResponse, GraftNudgeFetchRequest,
        GraftNudgeFetchResponse, GraftSession, GraftSessionId, GraftSessionPort,
        GraftSessionRegistrationRequest, GraftSessionRegistrationResponse, GraftSessionState,
        GraftSessionUnregistrationRequest, GraftSessionUnregistrationResponse,
        MAX_GRAFT_BATCH_LIMIT, MAX_GRAFT_NUDGE_MESSAGE_BYTES, MAX_GRAFT_SESSION_ID_BYTES,
        NudgeEvent, NudgeMessage,
    };
    use crate::error::AtmError;
    use crate::schema::AtmMessageId;
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    #[test]
    fn graft_session_id_rejects_blank_values() {
        let error = GraftSessionId::new("   ").expect_err("blank graft session id should fail");
        assert!(
            error
                .to_string()
                .contains("graft session id must not be blank")
        );
    }

    #[test]
    fn graft_batch_limit_rejects_zero() {
        let error = GraftBatchLimit::new(0).expect_err("zero graft batch limit should fail");
        assert!(
            error
                .to_string()
                .contains("graft batch limit must be greater than zero")
        );
    }

    #[test]
    fn graft_session_id_rejects_overlong_values() {
        let overlong = "a".repeat(MAX_GRAFT_SESSION_ID_BYTES + 1);
        let error = GraftSessionId::new(overlong).expect_err("overlong session id should fail");
        assert!(
            error
                .to_string()
                .contains("graft session id must be at most")
        );
    }

    #[test]
    fn graft_batch_limit_rejects_values_above_documented_ceiling() {
        let error = GraftBatchLimit::new(MAX_GRAFT_BATCH_LIMIT + 1)
            .expect_err("oversized graft batch limit should fail");
        assert!(
            error
                .to_string()
                .contains("graft batch limit must not exceed")
        );
    }

    fn registration_request() -> GraftSessionRegistrationRequest {
        GraftSessionRegistrationRequest {
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
            session_id: GraftSessionId::new("session-1").expect("session"),
            pid: 42,
            started_at: IsoTimestamp::now(),
        }
    }

    fn registration_response() -> GraftSessionRegistrationResponse {
        let request = registration_request();
        GraftSessionRegistrationResponse {
            team: request.team,
            agent: request.agent,
            session_id: request.session_id,
            registered_at: IsoTimestamp::now(),
            queue_capacity: 256,
        }
    }

    fn unregister_request() -> GraftSessionUnregistrationRequest {
        GraftSessionUnregistrationRequest {
            session_id: GraftSessionId::new("session-1").expect("session"),
        }
    }

    fn unregister_response() -> GraftSessionUnregistrationResponse {
        GraftSessionUnregistrationResponse {
            session_id: GraftSessionId::new("session-1").expect("session"),
            closed: true,
        }
    }

    fn fetch_request() -> GraftNudgeFetchRequest {
        GraftNudgeFetchRequest {
            session_id: GraftSessionId::new("session-1").expect("session"),
            limit: GraftBatchLimit::new(8).expect("limit"),
        }
    }

    fn nudge_event() -> NudgeEvent {
        NudgeEvent::new(
            AtmMessageId::new(),
            "sender".parse().expect("sender"),
            "hello",
            IsoTimestamp::now(),
            None,
        )
        .expect("nudge event")
    }

    fn fetch_response() -> GraftNudgeFetchResponse {
        GraftNudgeFetchResponse {
            session_id: GraftSessionId::new("session-1").expect("session"),
            nudges: vec![nudge_event()],
            remaining: 0,
            dropped_count: 0,
        }
    }

    fn drain_request() -> GraftNudgeDrainRequest {
        GraftNudgeDrainRequest {
            session_id: GraftSessionId::new("session-1").expect("session"),
            limit: GraftBatchLimit::new(4).expect("limit"),
        }
    }

    fn drain_response() -> GraftNudgeDrainResponse {
        GraftNudgeDrainResponse {
            session_id: GraftSessionId::new("session-1").expect("session"),
            nudges: vec![nudge_event()],
            remaining: 1,
            dropped_count: 0,
        }
    }

    fn assert_json_round_trip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + fmt::Debug,
    {
        let encoded = serde_json::to_string(value).expect("encode");
        let decoded = serde_json::from_str::<T>(&encoded).expect("decode");
        assert_eq!(&decoded, value);
    }

    #[test]
    fn graft_session_id_deserialize_rejects_blank_values() {
        let error = serde_json::from_str::<GraftSessionId>("\"   \"")
            .expect_err("blank graft session id should fail");
        assert!(
            error
                .to_string()
                .contains("graft session id must not be blank")
        );
    }

    #[test]
    fn graft_session_type_carries_all_documented_states() {
        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = "test-agent".parse().expect("agent");
        let session_id = GraftSessionId::new("session-1").expect("session");
        let states = [
            GraftSessionState::Inactive,
            GraftSessionState::Connecting,
            GraftSessionState::Registered,
            GraftSessionState::Disconnected,
            GraftSessionState::Closed,
            GraftSessionState::CloseFailed,
        ];

        for state in states {
            let session = GraftSession {
                team: team.clone(),
                agent: agent.clone(),
                session_id: session_id.clone(),
                state,
            };
            assert_eq!(session.session_id.as_str(), "session-1");
            assert_json_round_trip(&session);
        }
    }

    #[test]
    fn graft_registration_request_round_trips_json() {
        assert_json_round_trip(&registration_request());
    }

    #[test]
    fn graft_registration_response_round_trips_json() {
        assert_json_round_trip(&registration_response());
    }

    #[test]
    fn graft_unregistration_request_round_trips_json() {
        assert_json_round_trip(&unregister_request());
    }

    #[test]
    fn graft_unregistration_response_round_trips_json() {
        assert_json_round_trip(&unregister_response());
    }

    #[test]
    fn graft_fetch_request_round_trips_json() {
        assert_json_round_trip(&fetch_request());
    }

    #[test]
    fn graft_fetch_response_round_trips_json() {
        assert_json_round_trip(&fetch_response());
    }

    #[test]
    fn nudge_event_round_trips_json() {
        assert_json_round_trip(&nudge_event());
    }

    #[test]
    fn nudge_message_contract_exposes_documented_max_bytes() {
        assert_eq!(MAX_GRAFT_NUDGE_MESSAGE_BYTES, 4096);
    }

    #[test]
    fn nudge_message_rejects_overlong_values() {
        let value = "a".repeat(MAX_GRAFT_NUDGE_MESSAGE_BYTES + 1);
        let error = NudgeMessage::new(value).expect_err("overlong nudge message should fail");
        assert!(
            error
                .to_string()
                .contains("graft nudge message must be at most")
        );
    }

    #[test]
    fn graft_drain_request_round_trips_json() {
        assert_json_round_trip(&drain_request());
    }

    #[test]
    fn graft_drain_response_round_trips_json() {
        assert_json_round_trip(&drain_response());
    }

    struct MockGraftSessionPort;

    impl GraftSessionPort for MockGraftSessionPort {
        fn register_session(
            &self,
            request: GraftSessionRegistrationRequest,
        ) -> Result<GraftSessionRegistrationResponse, AtmError> {
            Ok(GraftSessionRegistrationResponse {
                team: request.team,
                agent: request.agent,
                session_id: request.session_id,
                registered_at: IsoTimestamp::now(),
                queue_capacity: 256,
            })
        }

        fn unregister_session(
            &self,
            request: GraftSessionUnregistrationRequest,
        ) -> Result<GraftSessionUnregistrationResponse, AtmError> {
            Ok(GraftSessionUnregistrationResponse {
                session_id: request.session_id,
                closed: true,
            })
        }

        fn fetch_nudges(
            &self,
            request: GraftNudgeFetchRequest,
        ) -> Result<GraftNudgeFetchResponse, AtmError> {
            Ok(GraftNudgeFetchResponse {
                session_id: request.session_id,
                nudges: vec![nudge_event()],
                remaining: 0,
                dropped_count: 0,
            })
        }

        fn drain_nudges(
            &self,
            request: GraftNudgeDrainRequest,
        ) -> Result<GraftNudgeDrainResponse, AtmError> {
            Ok(GraftNudgeDrainResponse {
                session_id: request.session_id,
                nudges: vec![nudge_event()],
                remaining: 0,
                dropped_count: 0,
            })
        }
    }

    #[test]
    fn graft_session_port_mock_is_object_safe_and_typed() {
        let port: &dyn GraftSessionPort = &MockGraftSessionPort;
        let registration = port
            .register_session(registration_request())
            .expect("register");
        assert_eq!(registration.session_id.as_str(), "session-1");

        let fetch = port.fetch_nudges(fetch_request()).expect("fetch");
        assert_eq!(fetch.nudges[0].message(), "hello");

        let drain = port.drain_nudges(drain_request()).expect("drain");
        assert_eq!(drain.nudges[0].from.as_str(), "sender");

        let unregister = port
            .unregister_session(unregister_request())
            .expect("unregister");
        assert!(unregister.closed);
    }
}
