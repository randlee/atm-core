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

pub const MAX_ADVISORY_SESSION_ID_BYTES: usize = 128;
pub const MAX_ADVISORY_BATCH_LIMIT: usize = 256;
pub const MAX_ADVISORY_MESSAGE_BYTES: usize = 4096;

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

/// Open session-facing contract for embedded advisory consumers.
///
/// This trait is intentionally not sealed. `atm-graft` must be able to own
/// the concrete client runtime implementation in a separate crate.
pub trait AdvisorySessionPort: Send + Sync {
    /// Register one active embedded advisory session with the daemon runtime.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon cannot accept or persist the
    /// session registration.
    fn register_session(
        &self,
        request: AdvisorySessionRegistrationRequest,
    ) -> Result<AdvisorySessionRegistrationResponse, AtmError>;

    /// Unregister one active embedded advisory session from the daemon runtime.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon cannot safely close the session.
    fn unregister_session(
        &self,
        request: AdvisorySessionUnregistrationRequest,
    ) -> Result<AdvisorySessionUnregistrationResponse, AtmError>;

    /// Fetch pending daemon-owned advisory events without draining them.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon cannot project the current nudge
    /// queue state for the active session.
    fn fetch_nudges(
        &self,
        request: AdvisoryFetchRequest,
    ) -> Result<AdvisoryFetchResponse, AtmError>;

    /// Drain pending daemon-owned advisory events for one active session.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when the daemon cannot safely hand off and clear
    /// the queued nudge state for the active session.
    fn drain_nudges(
        &self,
        request: AdvisoryDrainRequest,
    ) -> Result<AdvisoryDrainResponse, AtmError>;
}

/// Explicit lifecycle states for one advisory session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdvisorySessionState {
    Inactive,
    Connecting,
    Registered,
    Disconnected,
    Closed,
    CloseFailed,
}

/// Public lifecycle projection for one advisory session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySession {
    pub team: TeamName,
    pub agent: AgentName,
    pub session_id: AdvisorySessionId,
    pub state: AdvisorySessionState,
}

/// Stable identifier for one active advisory session.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AdvisorySessionId(String);

impl AdvisorySessionId {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the supplied session id is blank, only
    /// whitespace, or exceeds [`MAX_ADVISORY_SESSION_ID_BYTES`].
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            // AtmError::validation uses MessageValidationFailed, the shared boundary-validation
            // error code across all ATM subsystems; no graft-specific code is needed.
            return Err(
                AtmError::validation("advisory session id must not be blank").with_recovery(
                    "Populate a stable non-empty advisory session id before calling the advisory session runtime.",
                ),
            );
        }
        if value.len() > MAX_ADVISORY_SESSION_ID_BYTES {
            return Err(AtmError::validation(format!(
                "advisory session id must be at most {MAX_ADVISORY_SESSION_ID_BYTES} bytes"
            ))
            .with_recovery(
                "Shorten the advisory session id before calling the advisory session runtime.",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AdvisorySessionId {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for AdvisorySessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for AdvisorySessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Bounded advisory batch size requested by an embedded consumer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AdvisoryBatchLimit(NonZeroUsize);

impl AdvisoryBatchLimit {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the limit is zero or exceeds
    /// [`MAX_ADVISORY_BATCH_LIMIT`].
    pub fn new(value: usize) -> Result<Self, AtmError> {
        let value = NonZeroUsize::new(value).ok_or_else(|| {
            // AtmError::validation uses MessageValidationFailed, the shared boundary-validation
            // error code across all ATM subsystems; no graft-specific code is needed.
            AtmError::validation("advisory batch limit must be greater than zero").with_recovery(
                "Use a positive advisory batch limit before calling the daemon advisory queue surface.",
            )
        })?;
        if value.get() > MAX_ADVISORY_BATCH_LIMIT {
            return Err(AtmError::validation(format!(
                "advisory batch limit must not exceed {MAX_ADVISORY_BATCH_LIMIT}"
            ))
            .with_recovery(
                "Lower the advisory batch limit to the documented daemon queue ceiling before calling the advisory queue surface.",
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> usize {
        self.0.get()
    }
}

/// Daemon registration request for one embedded advisory session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySessionRegistrationRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub session_id: AdvisorySessionId,
    pub pid: u32,
    pub started_at: IsoTimestamp,
}

/// Daemon response after accepting one embedded advisory session registration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySessionRegistrationResponse {
    pub team: TeamName,
    pub agent: AgentName,
    pub session_id: AdvisorySessionId,
    pub registered_at: IsoTimestamp,
    pub queue_capacity: usize,
}

/// Dedicated advisory-stream request for one active embedded advisory session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryStreamRequest {
    pub registration: AdvisorySessionRegistrationRequest,
    pub limit: AdvisoryBatchLimit,
}

/// One emitted advisory-stream batch projected to an embedded host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryStreamResponse {
    pub session_id: AdvisorySessionId,
    pub nudges: Vec<AdvisoryEvent>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

/// Daemon unregistration request for one embedded advisory session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySessionUnregistrationRequest {
    pub session_id: AdvisorySessionId,
}

/// Daemon response after closing one embedded advisory session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorySessionUnregistrationResponse {
    pub session_id: AdvisorySessionId,
    pub closed: bool,
}

/// One daemon-originated nudge event projected to an embedded host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryEvent {
    pub message_id: AtmMessageId,
    pub from: AgentName,
    /// Advisory payload text.
    ///
    /// Implementations producing `AdvisoryEvent` values must bound this field to
    /// at most [`MAX_ADVISORY_MESSAGE_BYTES`] UTF-8 bytes before crossing
    /// the public ATM graft boundary.
    pub message: AdvisoryMessage,
    pub received_at: IsoTimestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct AdvisoryMessage(String);

impl AdvisoryMessage {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.len() > MAX_ADVISORY_MESSAGE_BYTES {
            return Err(AtmError::validation(format!(
                "advisory message exceeds the {MAX_ADVISORY_MESSAGE_BYTES}-byte limit"
            ))
            .with_recovery(
                "Shorten the advisory payload before crossing the shared ATM advisory boundary.",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for AdvisoryMessage {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for AdvisoryMessage {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for AdvisoryMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AdvisoryMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl PartialEq<&str> for AdvisoryMessage {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// Fetch request for the current daemon-owned pending-nudge snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryFetchRequest {
    pub session_id: AdvisorySessionId,
    pub limit: AdvisoryBatchLimit,
}

/// Fetch response for the current daemon-owned pending-nudge snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryFetchResponse {
    pub session_id: AdvisorySessionId,
    pub nudges: Vec<AdvisoryEvent>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

/// Drain request for one active embedded advisory session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryDrainRequest {
    pub session_id: AdvisorySessionId,
    pub limit: AdvisoryBatchLimit,
}

/// Drain response for one active embedded advisory session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisoryDrainResponse {
    pub session_id: AdvisorySessionId,
    pub nudges: Vec<AdvisoryEvent>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use serde::{Deserialize, Serialize};

    use super::{
        AdvisoryBatchLimit, AdvisoryDrainRequest, AdvisoryDrainResponse, AdvisoryEvent,
        AdvisoryFetchRequest, AdvisoryFetchResponse, AdvisoryMessage, AdvisorySession,
        AdvisorySessionId, AdvisorySessionPort, AdvisorySessionRegistrationRequest,
        AdvisorySessionRegistrationResponse, AdvisorySessionState,
        AdvisorySessionUnregistrationRequest, AdvisorySessionUnregistrationResponse,
        MAX_ADVISORY_BATCH_LIMIT, MAX_ADVISORY_MESSAGE_BYTES, MAX_ADVISORY_SESSION_ID_BYTES,
    };
    use crate::error::AtmError;
    use crate::schema::AtmMessageId;
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    #[test]
    fn advisory_session_id_rejects_blank_values() {
        let error =
            AdvisorySessionId::new("   ").expect_err("blank advisory session id should fail");
        assert!(
            error
                .to_string()
                .contains("advisory session id must not be blank")
        );
    }

    #[test]
    fn advisory_batch_limit_rejects_zero() {
        let error = AdvisoryBatchLimit::new(0).expect_err("zero graft batch limit should fail");
        assert!(
            error
                .to_string()
                .contains("advisory batch limit must be greater than zero")
        );
    }

    #[test]
    fn advisory_session_id_rejects_overlong_values() {
        let overlong = "a".repeat(MAX_ADVISORY_SESSION_ID_BYTES + 1);
        let error = AdvisorySessionId::new(overlong).expect_err("overlong session id should fail");
        assert!(
            error
                .to_string()
                .contains("advisory session id must be at most")
        );
    }

    #[test]
    fn advisory_batch_limit_rejects_values_above_documented_ceiling() {
        let error = AdvisoryBatchLimit::new(MAX_ADVISORY_BATCH_LIMIT + 1)
            .expect_err("oversized graft batch limit should fail");
        assert!(
            error
                .to_string()
                .contains("advisory batch limit must not exceed")
        );
    }

    fn registration_request() -> AdvisorySessionRegistrationRequest {
        AdvisorySessionRegistrationRequest {
            team: "test-team".parse().expect("team"),
            agent: "test-agent".parse().expect("agent"),
            session_id: AdvisorySessionId::new("session-1").expect("session"),
            pid: 42,
            started_at: IsoTimestamp::now(),
        }
    }

    fn registration_response() -> AdvisorySessionRegistrationResponse {
        let request = registration_request();
        AdvisorySessionRegistrationResponse {
            team: request.team,
            agent: request.agent,
            session_id: request.session_id,
            registered_at: IsoTimestamp::now(),
            queue_capacity: 256,
        }
    }

    fn unregister_request() -> AdvisorySessionUnregistrationRequest {
        AdvisorySessionUnregistrationRequest {
            session_id: AdvisorySessionId::new("session-1").expect("session"),
        }
    }

    fn unregister_response() -> AdvisorySessionUnregistrationResponse {
        AdvisorySessionUnregistrationResponse {
            session_id: AdvisorySessionId::new("session-1").expect("session"),
            closed: true,
        }
    }

    fn fetch_request() -> AdvisoryFetchRequest {
        AdvisoryFetchRequest {
            session_id: AdvisorySessionId::new("session-1").expect("session"),
            limit: AdvisoryBatchLimit::new(8).expect("limit"),
        }
    }

    fn nudge_event() -> AdvisoryEvent {
        AdvisoryEvent {
            message_id: AtmMessageId::new(),
            from: "sender".parse().expect("sender"),
            message: AdvisoryMessage::new("hello").expect("message"),
            received_at: IsoTimestamp::now(),
            task_id: None,
        }
    }

    fn fetch_response() -> AdvisoryFetchResponse {
        AdvisoryFetchResponse {
            session_id: AdvisorySessionId::new("session-1").expect("session"),
            nudges: vec![nudge_event()],
            remaining: 0,
            dropped_count: 0,
        }
    }

    fn drain_request() -> AdvisoryDrainRequest {
        AdvisoryDrainRequest {
            session_id: AdvisorySessionId::new("session-1").expect("session"),
            limit: AdvisoryBatchLimit::new(4).expect("limit"),
        }
    }

    fn drain_response() -> AdvisoryDrainResponse {
        AdvisoryDrainResponse {
            session_id: AdvisorySessionId::new("session-1").expect("session"),
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
    fn advisory_session_id_deserialize_rejects_blank_values() {
        let error = serde_json::from_str::<AdvisorySessionId>("\"   \"")
            .expect_err("blank advisory session id should fail");
        assert!(
            error
                .to_string()
                .contains("advisory session id must not be blank")
        );
    }

    #[test]
    fn advisory_session_type_carries_all_documented_states() {
        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = "test-agent".parse().expect("agent");
        let session_id = AdvisorySessionId::new("session-1").expect("session");
        let states = [
            AdvisorySessionState::Inactive,
            AdvisorySessionState::Connecting,
            AdvisorySessionState::Registered,
            AdvisorySessionState::Disconnected,
            AdvisorySessionState::Closed,
            AdvisorySessionState::CloseFailed,
        ];

        for state in states {
            let session = AdvisorySession {
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
    fn advisory_registration_request_round_trips_json() {
        assert_json_round_trip(&registration_request());
    }

    #[test]
    fn advisory_registration_response_round_trips_json() {
        assert_json_round_trip(&registration_response());
    }

    #[test]
    fn advisory_unregistration_request_round_trips_json() {
        assert_json_round_trip(&unregister_request());
    }

    #[test]
    fn advisory_unregistration_response_round_trips_json() {
        assert_json_round_trip(&unregister_response());
    }

    #[test]
    fn advisory_fetch_request_round_trips_json() {
        assert_json_round_trip(&fetch_request());
    }

    #[test]
    fn advisory_fetch_response_round_trips_json() {
        assert_json_round_trip(&fetch_response());
    }

    #[test]
    fn nudge_event_round_trips_json() {
        assert_json_round_trip(&nudge_event());
    }

    #[test]
    fn nudge_message_contract_exposes_documented_max_bytes() {
        assert_eq!(MAX_ADVISORY_MESSAGE_BYTES, 4096);
    }

    #[test]
    fn advisory_drain_request_round_trips_json() {
        assert_json_round_trip(&drain_request());
    }

    #[test]
    fn advisory_drain_response_round_trips_json() {
        assert_json_round_trip(&drain_response());
    }

    struct MockAdvisorySessionPort;

    impl AdvisorySessionPort for MockAdvisorySessionPort {
        fn register_session(
            &self,
            request: AdvisorySessionRegistrationRequest,
        ) -> Result<AdvisorySessionRegistrationResponse, AtmError> {
            Ok(AdvisorySessionRegistrationResponse {
                team: request.team,
                agent: request.agent,
                session_id: request.session_id,
                registered_at: IsoTimestamp::now(),
                queue_capacity: 256,
            })
        }

        fn unregister_session(
            &self,
            request: AdvisorySessionUnregistrationRequest,
        ) -> Result<AdvisorySessionUnregistrationResponse, AtmError> {
            Ok(AdvisorySessionUnregistrationResponse {
                session_id: request.session_id,
                closed: true,
            })
        }

        fn fetch_nudges(
            &self,
            request: AdvisoryFetchRequest,
        ) -> Result<AdvisoryFetchResponse, AtmError> {
            Ok(AdvisoryFetchResponse {
                session_id: request.session_id,
                nudges: vec![nudge_event()],
                remaining: 0,
                dropped_count: 0,
            })
        }

        fn drain_nudges(
            &self,
            request: AdvisoryDrainRequest,
        ) -> Result<AdvisoryDrainResponse, AtmError> {
            Ok(AdvisoryDrainResponse {
                session_id: request.session_id,
                nudges: vec![nudge_event()],
                remaining: 0,
                dropped_count: 0,
            })
        }
    }

    #[test]
    fn advisory_session_port_mock_is_object_safe_and_typed() {
        let port: &dyn AdvisorySessionPort = &MockAdvisorySessionPort;
        let registration = port
            .register_session(registration_request())
            .expect("register");
        assert_eq!(registration.session_id.as_str(), "session-1");

        let fetch = port.fetch_nudges(fetch_request()).expect("fetch");
        assert_eq!(fetch.nudges[0].message, "hello");

        let drain = port.drain_nudges(drain_request()).expect("drain");
        assert_eq!(drain.nudges[0].from.as_str(), "sender");

        let unregister = port
            .unregister_session(unregister_request())
            .expect("unregister");
        assert!(unregister.closed);
    }
}
