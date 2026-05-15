//! Phase R boundary skeleton contracts.

use crate::error::AtmError;
use crate::graft::AdvisoryStreamRequest;
use crate::protocol::{FramePayload, RequestEnvelope, RequestId, ResponseEnvelope};
use crate::schema::AtmMessageId;
pub use crate::protocol::{
    NotificationEvent, ReconcileRequest, ReconcileResult, RuntimeStatusSnapshot, WatchEventBatch,
    WatchSubscriptionRequest,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

/// Workspace-convention seal only; not compiler-enforced outside this crate.
///
/// Only ATM workspace crates may implement boundary traits. Enforced by
/// boundary lint, forbidden-edge rules, and review gates.
#[doc(hidden)]
pub mod sealed {
    pub trait Sealed {}
}

mod mail;
mod store;

pub use mail::*;
pub use store::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct MessageKey(String);

impl MessageKey {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the key is blank or only whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("message key must not be blank").with_recovery(
                    "Populate a stable ATM message key before calling the Phase R boundary.",
                ),
            );
        }
        Ok(Self(value))
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when the derived key is blank or only whitespace.
    pub fn for_atm_message(message_id: AtmMessageId) -> Result<Self, AtmError> {
        Self::new(format!("atm:{message_id}"))
    }

    /// # Errors
    ///
    /// Returns [`AtmError`] when the derived key is blank or only whitespace.
    pub fn for_inbox_path(path: &Path) -> Result<Self, AtmError> {
        Self::new(path.to_string_lossy().into_owned())
    }
}

impl AsRef<str> for MessageKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for MessageKey {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct TaskState(String);

impl TaskState {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the state is blank or only whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("task state must not be blank").with_recovery(
                    "Populate a non-empty task state before calling the Phase R boundary.",
                ),
            );
        }
        Ok(Self(value))
    }
}

impl AsRef<str> for TaskState {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for TaskState {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl PartialEq<&str> for TaskState {
    fn eq(&self, other: &&str) -> bool {
        self.as_ref() == *other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AckTransition(String);

impl AckTransition {
    /// # Errors
    ///
    /// Returns [`AtmError`] when the transition is blank or only whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("ack transition must not be blank").with_recovery(
                    "Populate a non-empty ack transition before calling the Phase R boundary.",
                ),
            );
        }
        Ok(Self(value))
    }
}

impl AsRef<str> for AckTransition {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AckTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for AckTransition {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// BOUNDARY-AtmProtocol — see docs/atm-core/boundaries.md.
pub trait AtmProtocol: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when a protocol request envelope cannot be converted
    /// into a frame payload.
    fn request_to_frame(
        &self,
        request_id: RequestId,
        request: RequestEnvelope,
    ) -> Result<FramePayload, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a frame payload cannot be decoded into a
    /// protocol request envelope.
    fn request_from_frame(
        &self,
        frame: FramePayload,
    ) -> Result<(RequestId, RequestEnvelope), AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a protocol response envelope cannot be
    /// converted into a frame payload.
    fn response_to_frame(
        &self,
        request_id: RequestId,
        response: ResponseEnvelope,
    ) -> Result<FramePayload, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a frame payload cannot be decoded into a
    /// protocol response envelope.
    fn response_from_frame(
        &self,
        frame: FramePayload,
    ) -> Result<(RequestId, ResponseEnvelope), AtmError>;
}

/// BOUNDARY-ClientTransport — see docs/atm-core/boundaries.md.
pub trait ClientTransport: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when the framed request cannot be delivered or when
    /// the peer returns an unrecoverable protocol response.
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;
}

/// BOUNDARY-ServerTransport — see docs/atm-core/boundaries.md.
pub trait ServerTransport: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when framing, transport serving, or dispatch handoff
    /// cannot proceed reliably.
    fn serve(
        &self,
        dispatcher: std::sync::Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<(), AtmError>;
}

/// BOUNDARY-RequestDispatcher — see docs/atm-core/boundaries.md.
pub trait RequestDispatcher: sealed::Sealed + Send + Sync {
    /// # Errors
    ///
    /// Returns `AtmError` when protocol request routing or handler dispatch
    /// cannot produce a valid response.
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a long-lived advisory stream cannot be
    /// established or when advisory delivery cannot continue reliably.
    fn dispatch_advisory_stream(
        &self,
        request: AdvisoryStreamRequest,
        sink: &mut dyn AdvisoryStreamSink,
    ) -> Result<(), AtmError>;
}

/// Shared framed response sink used by same-host advisory stream transports.
pub trait AdvisoryStreamSink {
    /// # Errors
    ///
    /// Returns `AtmError` when the next advisory response frame cannot be
    /// delivered to the connected client.
    fn emit(&mut self, response: ResponseEnvelope) -> Result<(), AtmError>;

    fn stop_requested(&self) -> bool {
        false
    }
}

/// BOUNDARY-NotificationSink — see docs/atm-core/boundaries.md.
pub trait NotificationSink: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when notification delivery cannot be executed.
    fn deliver(&self, event: NotificationEvent) -> Result<(), AtmError>;
}

/// BOUNDARY-StatusSource — see docs/atm-core/boundaries.md.
pub trait StatusSource: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when a runtime status snapshot cannot be collected.
    fn snapshot(&self) -> Result<RuntimeStatusSnapshot, AtmError>;
}

/// BOUNDARY-WatchEventSource — see docs/atm-core/boundaries.md.
pub trait WatchEventSource: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when watch subscriptions cannot be created or events
    /// cannot be delivered as a batch.
    fn poll(&self, request: WatchSubscriptionRequest) -> Result<WatchEventBatch, AtmError>;
}

/// BOUNDARY-ReconcileCoordinator — see docs/atm-core/boundaries.md.
pub trait ReconcileCoordinator: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when reconcile policy cannot be executed for the
    /// request input.
    fn reconcile(&self, request: ReconcileRequest) -> Result<ReconcileResult, AtmError>;
}
