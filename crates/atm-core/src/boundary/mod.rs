//! Phase R boundary skeleton contracts.

use crate::error::AtmError;
use crate::protocol::{FramePayload, RequestEnvelope, ResponseEnvelope};
pub use crate::protocol::{
    NotificationEvent, ReconcileRequest, ReconcileResult, RuntimeStatusSnapshot, WatchEventBatch,
    WatchSubscriptionRequest,
};

/// Workspace-convention seal only; not compiler-enforced outside this crate.
///
/// Only ATM workspace crates may implement boundary traits. Enforced by
/// boundary lint, forbidden-edge rules, and review gates.
#[doc(hidden)]
pub mod sealed {
    pub trait Sealed {}
}

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

/// Canonical Phase R mail-store request entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreRequest;

/// Canonical Phase R mail-store response entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailStoreResponse;

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

/// Canonical Phase R task-store request entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreRequest;

/// Canonical Phase R task-store response entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStoreResponse;

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

/// Canonical Phase R roster-store request entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterStoreRequest;

/// Canonical Phase R roster-store response entrypoint payload.
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

/// Canonical Phase R inbox-ingress request entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressRequest;

/// Canonical Phase R inbox-ingress response entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxIngressResponse;

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

/// Canonical Phase R inbox-export request entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportRequest;

/// Canonical Phase R inbox-export response entrypoint payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxExportResponse;

/// BOUNDARY-AtmProtocol — see docs/atm-core/boundaries.md.
pub trait AtmProtocol: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when a protocol request envelope cannot be converted
    /// into a frame payload.
    fn request_to_frame(&self, request: RequestEnvelope) -> Result<FramePayload, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a frame payload cannot be decoded into a
    /// protocol request envelope.
    fn request_from_frame(&self, frame: FramePayload) -> Result<RequestEnvelope, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a protocol response envelope cannot be
    /// converted into a frame payload.
    fn response_to_frame(&self, response: ResponseEnvelope) -> Result<FramePayload, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a frame payload cannot be decoded into a
    /// protocol response envelope.
    fn response_from_frame(&self, frame: FramePayload) -> Result<ResponseEnvelope, AtmError>;
}

/// BOUNDARY-ClientTransport — see docs/atm-core/boundaries.md.
pub trait ClientTransport: sealed::Sealed {
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
    fn serve(&self, dispatcher: &dyn RequestDispatcher) -> Result<(), AtmError>;
}

/// BOUNDARY-RequestDispatcher — see docs/atm-core/boundaries.md.
pub trait RequestDispatcher: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when protocol request routing or handler dispatch
    /// cannot produce a valid response.
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;
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

/// BOUNDARY-MailStore — see docs/atm-core/boundaries.md.
pub trait MailStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when durable mailbox persistence, transaction
    /// boundaries, or replay-state access cannot satisfy the contract.
    fn bootstrap(
        &self,
        request: MailStoreBootstrapRequest,
    ) -> Result<MailStoreBootstrapResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a mailbox transaction cannot run.
    fn run_transaction(
        &self,
        request: MailStoreTransactionRequest,
    ) -> Result<MailStoreTransactionResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the mailbox transaction cannot be started,
    /// executed, or committed safely.
    fn upsert_message(
        &self,
        request: MailStoreUpsertMessageRequest,
    ) -> Result<MailStoreUpsertMessageResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the requested message cannot be loaded.
    fn load_message(
        &self,
        request: MailStoreLoadMessageRequest,
    ) -> Result<MailStoreLoadMessageResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when visibility state persistence fails.
    fn upsert_visibility_state(
        &self,
        request: MailStoreUpsertVisibilityStateRequest,
    ) -> Result<MailStoreUpsertVisibilityStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when visibility state cannot be loaded.
    fn load_visibility_state(
        &self,
        request: MailStoreLoadVisibilityStateRequest,
    ) -> Result<MailStoreLoadVisibilityStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when ingest-replay state persistence fails.
    fn record_ingest_replay_state(
        &self,
        request: MailStoreRecordIngestReplayStateRequest,
    ) -> Result<MailStoreRecordIngestReplayStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when ingest-replay state cannot be loaded.
    fn load_ingest_replay_state(
        &self,
        request: MailStoreLoadIngestReplayStateRequest,
    ) -> Result<MailStoreLoadIngestReplayStateResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a mailbox health snapshot cannot be read.
    fn health_snapshot(
        &self,
        request: MailStoreHealthSnapshotRequest,
    ) -> Result<MailStoreHealthSnapshotResponse, AtmError>;
}

/// BOUNDARY-TaskStore — see docs/atm-core/boundaries.md.
pub trait TaskStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when durable task-state persistence or task/message
    /// linkage updates fail to satisfy the contract.
    fn create_task(
        &self,
        request: TaskStoreCreateTaskRequest,
    ) -> Result<TaskStoreCreateTaskResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the requested task cannot be loaded.
    fn load_task(
        &self,
        request: TaskStoreLoadTaskRequest,
    ) -> Result<TaskStoreLoadTaskResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the requested task cannot be updated.
    fn update_task(
        &self,
        request: TaskStoreUpdateTaskRequest,
    ) -> Result<TaskStoreUpdateTaskResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a message link cannot be attached.
    fn attach_message_link(
        &self,
        request: TaskStoreAttachMessageLinkRequest,
    ) -> Result<TaskStoreAttachMessageLinkResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a message link cannot be detached.
    fn detach_message_link(
        &self,
        request: TaskStoreDetachMessageLinkRequest,
    ) -> Result<TaskStoreDetachMessageLinkResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when an ack transition cannot be recorded.
    fn record_ack_transition(
        &self,
        request: TaskStoreRecordAckTransitionRequest,
    ) -> Result<TaskStoreRecordAckTransitionResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when task metadata cannot be queried.
    fn query_task_metadata(
        &self,
        request: TaskStoreQueryTaskMetadataRequest,
    ) -> Result<TaskStoreQueryTaskMetadataResponse, AtmError>;
}

/// BOUNDARY-RosterStore — see docs/atm-core/boundaries.md.
pub trait RosterStore: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when durable roster persistence or membership
    /// queries fail to satisfy the contract.
    fn replace_roster(
        &self,
        request: RosterStoreReplaceRosterRequest,
    ) -> Result<RosterStoreReplaceRosterResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a roster snapshot cannot be loaded.
    fn load_roster(
        &self,
        request: RosterStoreLoadRosterRequest,
    ) -> Result<RosterStoreLoadRosterResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when roster membership cannot be queried.
    fn query_membership(
        &self,
        request: RosterStoreQueryMembershipRequest,
    ) -> Result<RosterStoreQueryMembershipResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when a roster health snapshot cannot be collected.
    fn health_snapshot(
        &self,
        request: RosterStoreHealthSnapshotRequest,
    ) -> Result<RosterStoreHealthSnapshotResponse, AtmError>;
}

/// BOUNDARY-ConfigIngress — see docs/atm-core/boundaries.md.
pub trait ConfigIngress: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when persisted ATM/team configuration cannot be
    /// loaded, parsed, or validated into typed models.
    fn load_config(&self, request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError>;
}

/// BOUNDARY-InboxIngress — see docs/atm-core/boundaries.md.
pub trait InboxIngress: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when compatibility inbox material cannot be
    /// imported, fingerprinted, or diagnosed into ATM-owned state.
    fn import_inbox_source(
        &self,
        request: InboxIngressImportRequest,
    ) -> Result<InboxIngressImportResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when identity fingerprinting cannot be computed.
    fn compute_identity_fingerprint(
        &self,
        request: InboxIngressIdentityFingerprintRequest,
    ) -> Result<InboxIngressIdentityFingerprintResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when inbox diagnostics cannot be generated.
    fn report_diagnostics(
        &self,
        request: InboxIngressDiagnosticsRequest,
    ) -> Result<InboxIngressDiagnosticsResponse, AtmError>;
}

/// BOUNDARY-InboxExport — see docs/atm-core/boundaries.md.
pub trait InboxExport: sealed::Sealed {
    /// # Errors
    ///
    /// Returns `AtmError` when ATM-owned state cannot be projected back to the
    /// compatibility inbox/export surfaces.
    fn export_record(
        &self,
        request: InboxExportRecordRequest,
    ) -> Result<InboxExportRecordResponse, AtmError>;

    /// # Errors
    ///
    /// Returns `AtmError` when the message re-export cannot be materialized.
    fn reexport_message(
        &self,
        request: InboxExportReexportMessageRequest,
    ) -> Result<InboxExportReexportMessageResponse, AtmError>;
}
