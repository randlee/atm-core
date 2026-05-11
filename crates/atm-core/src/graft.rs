//! Thin graft-facing daemon client contracts shared by embedded host agents.

use std::fmt;
use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

use crate::ack::{AckOutcome, AckRequest};
use crate::error::AtmError;
use crate::read::{ReadOutcome, ReadQuery};
use crate::schema::LegacyMessageId;
use crate::send::{SendOutcome, SendRequest};
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

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

/// Stable identifier for one active graft session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct GraftSessionId(String);

impl GraftSessionId {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the supplied session id is blank or only
    /// whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(AtmError::validation("graft session id must not be blank").with_recovery(
                "Populate a stable non-empty graft session id before calling the graft session runtime.",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
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
    /// Returns [`AtmError`] when the limit is zero.
    pub fn new(value: usize) -> Result<Self, AtmError> {
        let value = NonZeroUsize::new(value).ok_or_else(|| {
            AtmError::validation("graft batch limit must be greater than zero").with_recovery(
                "Use a positive graft nudge batch limit before calling the daemon graft queue surface.",
            )
        })?;
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

/// One daemon-originated graft nudge projected to an embedded host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraftNudge {
    pub message_id: LegacyMessageId,
    pub from: AgentName,
    pub message: String,
    pub received_at: IsoTimestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
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
    pub nudges: Vec<GraftNudge>,
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
    pub nudges: Vec<GraftNudge>,
    pub remaining: usize,
    #[serde(default)]
    pub dropped_count: usize,
}

#[cfg(test)]
mod tests {
    use super::{GraftBatchLimit, GraftSessionId};

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
}
