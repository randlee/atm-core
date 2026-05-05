//! Shared protocol DTO stubs for the core transport boundary family.

/// Shared protocol request envelope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestEnvelope;

/// Shared protocol response envelope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseEnvelope;

/// Raw protocol frame payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FramePayload;

/// Shared notification event payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationEvent;

/// Runtime status snapshot transport payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeStatusSnapshot;

/// Watch subscription request payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchSubscriptionRequest;

/// Watch event batch transport payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchEventBatch;

/// Reconcile request transport payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileRequest;

/// Reconcile outcome transport payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileResult;
