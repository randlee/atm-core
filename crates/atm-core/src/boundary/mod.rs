//! Phase R boundary skeleton contracts.

use crate::error::AtmError;

pub mod sealed {
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
pub struct MailStoreBootstrapRequest;

/// Stub mail-store response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreBootstrapResponse;

/// Stub mail-store transaction request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreTransactionRequest;

/// Stub mail-store transaction response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreTransactionResponse;

/// Stub mail-store upsert-message request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreUpsertMessageRequest;

/// Stub mail-store upsert-message response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreUpsertMessageResponse;

/// Stub mail-store load-message request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreLoadMessageRequest;

/// Stub mail-store load-message response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreLoadMessageResponse;

/// Stub mail-store upsert-visibility request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreUpsertVisibilityStateRequest;

/// Stub mail-store upsert-visibility response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreUpsertVisibilityStateResponse;

/// Stub mail-store load-visibility request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreLoadVisibilityStateRequest;

/// Stub mail-store load-visibility response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreLoadVisibilityStateResponse;

/// Stub mail-store record-ingest-replay request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreRecordIngestReplayStateRequest;

/// Stub mail-store record-ingest-replay response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreRecordIngestReplayStateResponse;

/// Stub mail-store load-ingest-replay request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreLoadIngestReplayStateRequest;

/// Stub mail-store load-ingest-replay response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreLoadIngestReplayStateResponse;

/// Stub mail-store health-snapshot request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreHealthSnapshotRequest;

/// Stub mail-store health-snapshot response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreHealthSnapshotResponse;

pub type MailStoreRequest = MailStoreBootstrapRequest;
pub type MailStoreResponse = MailStoreBootstrapResponse;

/// Stub task-store request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreCreateTaskRequest;

/// Stub task-store response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreCreateTaskResponse;

/// Stub task-store load-task request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreLoadTaskRequest;

/// Stub task-store load-task response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreLoadTaskResponse;

/// Stub task-store update-task request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreUpdateTaskRequest;

/// Stub task-store update-task response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreUpdateTaskResponse;

/// Stub task-store attach-message-link request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreAttachMessageLinkRequest;

/// Stub task-store attach-message-link response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreAttachMessageLinkResponse;

/// Stub task-store detach-message-link request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreDetachMessageLinkRequest;

/// Stub task-store detach-message-link response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreDetachMessageLinkResponse;

/// Stub task-store record-ack-transition request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreRecordAckTransitionRequest;

/// Stub task-store record-ack-transition response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreRecordAckTransitionResponse;

/// Stub task-store query-task-metadata request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreQueryTaskMetadataRequest;

/// Stub task-store query-task-metadata response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreQueryTaskMetadataResponse;

pub type TaskStoreRequest = TaskStoreCreateTaskRequest;
pub type TaskStoreResponse = TaskStoreCreateTaskResponse;

/// Stub roster-store request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreReplaceRosterRequest;

/// Stub roster-store response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreReplaceRosterResponse;

/// Stub roster-store load-roster request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreLoadRosterRequest;

/// Stub roster-store load-roster response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreLoadRosterResponse;

/// Stub roster-store query-membership request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreQueryMembershipRequest;

/// Stub roster-store query-membership response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreQueryMembershipResponse;

/// Stub roster-store health-snapshot request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreHealthSnapshotRequest;

/// Stub roster-store health-snapshot response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreHealthSnapshotResponse;

pub type RosterStoreRequest = RosterStoreReplaceRosterRequest;
pub type RosterStoreResponse = RosterStoreReplaceRosterResponse;

/// Stub config-ingress request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigLoadRequest;

/// Stub config-ingress response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigLoadResponse;

/// Stub inbox-ingress request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressImportRequest;

/// Stub inbox-ingress response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressImportResponse;

/// Stub inbox-ingress identity-fingerprint request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressIdentityFingerprintRequest;

/// Stub inbox-ingress identity-fingerprint response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressIdentityFingerprintResponse;

/// Stub inbox-ingress diagnostics request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressDiagnosticsRequest;

/// Stub inbox-ingress diagnostics response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressDiagnosticsResponse;

pub type InboxIngressRequest = InboxIngressImportRequest;
pub type InboxIngressResponse = InboxIngressImportResponse;

/// Stub inbox-export request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportRecordRequest;

/// Stub inbox-export response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportRecordResponse;

/// Stub inbox-export re-export request for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportReexportMessageRequest;

/// Stub inbox-export re-export response for the Phase R skeleton.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportReexportMessageResponse;

pub type InboxExportRequest = InboxExportRecordRequest;
pub type InboxExportResponse = InboxExportRecordResponse;

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
    fn bootstrap(
        &self,
        request: MailStoreBootstrapRequest,
    ) -> Result<MailStoreBootstrapResponse, AtmError>;

    fn run_transaction(
        &self,
        request: MailStoreTransactionRequest,
    ) -> Result<MailStoreTransactionResponse, AtmError>;

    fn upsert_message(
        &self,
        request: MailStoreUpsertMessageRequest,
    ) -> Result<MailStoreUpsertMessageResponse, AtmError>;

    fn load_message(
        &self,
        request: MailStoreLoadMessageRequest,
    ) -> Result<MailStoreLoadMessageResponse, AtmError>;

    fn upsert_visibility_state(
        &self,
        request: MailStoreUpsertVisibilityStateRequest,
    ) -> Result<MailStoreUpsertVisibilityStateResponse, AtmError>;

    fn load_visibility_state(
        &self,
        request: MailStoreLoadVisibilityStateRequest,
    ) -> Result<MailStoreLoadVisibilityStateResponse, AtmError>;

    fn record_ingest_replay_state(
        &self,
        request: MailStoreRecordIngestReplayStateRequest,
    ) -> Result<MailStoreRecordIngestReplayStateResponse, AtmError>;

    fn load_ingest_replay_state(
        &self,
        request: MailStoreLoadIngestReplayStateRequest,
    ) -> Result<MailStoreLoadIngestReplayStateResponse, AtmError>;

    fn health_snapshot(
        &self,
        request: MailStoreHealthSnapshotRequest,
    ) -> Result<MailStoreHealthSnapshotResponse, AtmError>;
}

/// BOUNDARY-TaskStore — see docs/atm-core/boundaries.md.
pub trait TaskStore: sealed::Sealed {
    fn create_task(
        &self,
        request: TaskStoreCreateTaskRequest,
    ) -> Result<TaskStoreCreateTaskResponse, AtmError>;

    fn load_task(
        &self,
        request: TaskStoreLoadTaskRequest,
    ) -> Result<TaskStoreLoadTaskResponse, AtmError>;

    fn update_task(
        &self,
        request: TaskStoreUpdateTaskRequest,
    ) -> Result<TaskStoreUpdateTaskResponse, AtmError>;

    fn attach_message_link(
        &self,
        request: TaskStoreAttachMessageLinkRequest,
    ) -> Result<TaskStoreAttachMessageLinkResponse, AtmError>;

    fn detach_message_link(
        &self,
        request: TaskStoreDetachMessageLinkRequest,
    ) -> Result<TaskStoreDetachMessageLinkResponse, AtmError>;

    fn record_ack_transition(
        &self,
        request: TaskStoreRecordAckTransitionRequest,
    ) -> Result<TaskStoreRecordAckTransitionResponse, AtmError>;

    fn query_task_metadata(
        &self,
        request: TaskStoreQueryTaskMetadataRequest,
    ) -> Result<TaskStoreQueryTaskMetadataResponse, AtmError>;
}

/// BOUNDARY-RosterStore — see docs/atm-core/boundaries.md.
pub trait RosterStore: sealed::Sealed {
    fn replace_roster(
        &self,
        request: RosterStoreReplaceRosterRequest,
    ) -> Result<RosterStoreReplaceRosterResponse, AtmError>;

    fn load_roster(
        &self,
        request: RosterStoreLoadRosterRequest,
    ) -> Result<RosterStoreLoadRosterResponse, AtmError>;

    fn query_membership(
        &self,
        request: RosterStoreQueryMembershipRequest,
    ) -> Result<RosterStoreQueryMembershipResponse, AtmError>;

    fn health_snapshot(
        &self,
        request: RosterStoreHealthSnapshotRequest,
    ) -> Result<RosterStoreHealthSnapshotResponse, AtmError>;
}

/// BOUNDARY-ConfigIngress — see docs/atm-core/boundaries.md.
pub trait ConfigIngress: sealed::Sealed {
    fn load_config(&self, request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError>;
}

/// BOUNDARY-InboxIngress — see docs/atm-core/boundaries.md.
pub trait InboxIngress: sealed::Sealed {
    fn import_inbox_source(
        &self,
        request: InboxIngressImportRequest,
    ) -> Result<InboxIngressImportResponse, AtmError>;

    fn compute_identity_fingerprint(
        &self,
        request: InboxIngressIdentityFingerprintRequest,
    ) -> Result<InboxIngressIdentityFingerprintResponse, AtmError>;

    fn report_diagnostics(
        &self,
        request: InboxIngressDiagnosticsRequest,
    ) -> Result<InboxIngressDiagnosticsResponse, AtmError>;
}

/// BOUNDARY-InboxExport — see docs/atm-core/boundaries.md.
pub trait InboxExport: sealed::Sealed {
    fn export_record(
        &self,
        request: InboxExportRecordRequest,
    ) -> Result<InboxExportRecordResponse, AtmError>;

    fn reexport_message(
        &self,
        request: InboxExportReexportMessageRequest,
    ) -> Result<InboxExportReexportMessageResponse, AtmError>;
}
