//! Phase R boundary skeleton contracts.

use crate::error::AtmError;
use crate::graft::AdvisoryStreamRequest;
use crate::protocol::{FramePayload, RequestEnvelope, RequestId, ResponseEnvelope};
pub use crate::protocol::{
    NotificationEvent, ReconcileRequest, ReconcileResult, RuntimeStatusSnapshot, WatchEventBatch,
    WatchSubscriptionRequest,
};
pub use atm_storage::contract::{
    AckTransition, Message, MessageFingerprint, MessageKey, TaskState,
};

/// Workspace-convention seal only; not compiler-enforced outside this crate.
///
/// Only ATM workspace crates may implement boundary traits. Enforced by
/// boundary lint, forbidden-edge rules, and review gates; this is a documented
/// enforcement limitation until the trait surfaces move behind stricter crate
/// extraction or compiler-enforced sealing.
#[doc(hidden)]
pub mod sealed {
    pub trait Sealed {}
}

mod mail;
mod runtime;
mod store;

// Intentional re-export façade: the boundary module is the stable public import
// surface for Phase R/AA contracts, so callers should not need to know whether
// an item lives in `mail` or `store`.
pub use mail::*;
pub use runtime::*;
pub use store::*;

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
///
/// Open by design: transports outside `atm-core` may provide the concrete sink
/// that owns framing and client backpressure for advisory stream responses.
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
