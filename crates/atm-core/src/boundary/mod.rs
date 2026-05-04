//! Phase R boundary skeleton contracts.

use crate::error::AtmError;

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

/// Stub mail-store request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreRequest;

/// Stub mail-store response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreResponse;

/// Stub task-store request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreRequest;

/// Stub task-store response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreResponse;

/// Stub roster-store request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreRequest;

/// Stub roster-store response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreResponse;

/// Stub config-ingress request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigLoadRequest;

/// Stub config-ingress response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigLoadResponse;

/// Stub inbox-ingress request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressRequest;

/// Stub inbox-ingress response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressResponse;

/// Stub inbox-export request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportRequest;

/// Stub inbox-export response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportResponse;

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

/// BOUNDARY-MailStore — see docs/atm-core/boundaries.md.
pub trait MailStore: sealed::Sealed {
    fn mail_state(&self, request: MailStoreRequest) -> Result<MailStoreResponse, AtmError>;
}

/// BOUNDARY-TaskStore — see docs/atm-core/boundaries.md.
pub trait TaskStore: sealed::Sealed {
    fn task_state(&self, request: TaskStoreRequest) -> Result<TaskStoreResponse, AtmError>;
}

/// BOUNDARY-RosterStore — see docs/atm-core/boundaries.md.
pub trait RosterStore: sealed::Sealed {
    fn roster_state(&self, request: RosterStoreRequest) -> Result<RosterStoreResponse, AtmError>;
}

/// BOUNDARY-ConfigIngress — see docs/atm-core/boundaries.md.
pub trait ConfigIngress: sealed::Sealed {
    fn load_config(&self, request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError>;
}

/// BOUNDARY-InboxIngress — see docs/atm-core/boundaries.md.
pub trait InboxIngress: sealed::Sealed {
    fn import_inbox(&self, request: InboxIngressRequest) -> Result<InboxIngressResponse, AtmError>;
}

/// BOUNDARY-InboxExport — see docs/atm-core/boundaries.md.
pub trait InboxExport: sealed::Sealed {
    fn export_inbox(&self, request: InboxExportRequest) -> Result<InboxExportResponse, AtmError>;
}
