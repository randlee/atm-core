//! Phase R boundary skeleton contracts.

mod sealed {
    pub trait Sealed {}
}

/// Stub ATM request envelope for the Phase R protocol skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AtmRequestEnvelope;

/// Stub ATM response envelope for the Phase R protocol skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AtmResponseEnvelope;

/// Stub ATM frame payload for the Phase R protocol skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AtmFramePayload;

/// Stub outbound client-transport request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientTransportRequest;

/// Stub outbound client-transport response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientTransportResponse;

/// Stub inbound server-transport request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerTransportRequest;

/// Stub inbound server-transport response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerTransportResponse;

/// Stub dispatcher request envelope for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchRequestEnvelope;

/// Stub dispatcher response envelope for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchResponseEnvelope;

/// Stub outbound notification event for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationEvent;

/// Stub inbound runtime-status snapshot for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeStatusSnapshot;

/// Stub watch-subscription request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchSubscriptionRequest;

/// Stub watch event batch for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchEventBatch;

/// Stub reconcile request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileRequest;

/// Stub reconcile result for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileResult;

/// BOUNDARY-AtmProtocol — see docs/atm-core/boundaries.md.
pub trait AtmProtocol: sealed::Sealed {}

/// BOUNDARY-ClientTransport — see docs/atm-core/boundaries.md.
pub trait ClientTransport: sealed::Sealed {}

/// BOUNDARY-ServerTransport — see docs/atm-core/boundaries.md.
pub trait ServerTransport: sealed::Sealed {}

/// BOUNDARY-RequestDispatcher — see docs/atm-core/boundaries.md.
pub trait RequestDispatcher: sealed::Sealed {}

/// BOUNDARY-NotificationSink — see docs/atm-core/boundaries.md.
pub trait NotificationSink: sealed::Sealed {}

/// BOUNDARY-StatusSource — see docs/atm-core/boundaries.md.
pub trait StatusSource: sealed::Sealed {}

/// BOUNDARY-WatchEventSource — see docs/atm-core/boundaries.md.
pub trait WatchEventSource: sealed::Sealed {}

/// BOUNDARY-ReconcileCoordinator — see docs/atm-core/boundaries.md.
pub trait ReconcileCoordinator: sealed::Sealed {}
