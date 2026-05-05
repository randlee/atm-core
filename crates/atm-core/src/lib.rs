/// Acknowledgement workflows for ack-required mailbox messages.
pub mod ack;
/// Public agent-address parsing and normalization helpers.
pub mod address;
/// Phase R boundary traits and placeholder contract types.
pub mod boundary;
/// Hidden support helpers used by concrete boundary adapter crates.
#[doc(hidden)]
pub mod boundary_support;
/// Mailbox cleanup workflows for read and acknowledged messages.
pub mod clear;
/// Internal configuration discovery and resolution helpers.
pub(crate) mod config;
/// Doctor-report types and health checks for the CLI surface.
pub mod doctor;
/// Shared ATM error types and recovery-oriented error helpers.
pub mod error;
/// Stable ATM-owned error-code registry used by core and CLI layers.
pub mod error_codes;
/// Public ATM home and team-path resolution helpers.
pub mod home;
/// Internal identity resolution and hook lookup helpers.
pub(crate) mod identity;
/// Log query and filtering types for the CLI log surface.
pub mod log;
/// Internal mailbox persistence and parsing helpers.
pub(crate) mod mailbox;
/// Internal model-registry plumbing reserved for follow-on work.
pub(crate) mod model_registry;
/// Observability adapter traits and event payload types.
pub mod observability;
/// Internal atomic persistence helpers for shared mutable state files.
pub(crate) mod persistence;
/// Internal process-liveness helpers shared across lock implementations.
pub(crate) mod process;
/// Shared protocol DTOs used by boundary transport and adapter contracts.
pub mod protocol;
/// Mailbox read/query workflows and output models.
pub mod read;
/// Reserved production role constants shared across runtime and tests.
pub mod roles;
/// Public mailbox and team schema types shared with CLI tests and adapters.
pub mod schema;
/// Mailbox send workflows and request/response models.
pub mod send;
/// Internal service-owned seams that isolate retained command orchestration
/// from direct helper/path access.
pub(crate) mod service_runtime;
/// Retained local team discovery, roster repair, and backup/restore workflows.
pub mod team_admin;
/// Shared synthetic test identities and role constants used across crate tests.
#[doc(hidden)]
pub mod test_support;
/// Internal text-formatting helpers used by ATM core surfaces.
pub(crate) mod text;
/// Shared enums and semantic newtypes used across ATM core workflows.
pub mod types;
/// Internal ATM-owned workflow-state helpers shared across mailbox services.
pub(crate) mod workflow;

pub use boundary::{
    AtmProtocol, ClientTransport, ConfigIngress, ConfigLoadRequest, ConfigLoadResponse,
    ConfigTeamLoadRequest, ConfigTeamLoadResponse, InboxExport, InboxExportReexportMessageRequest,
    InboxExportReexportMessageResponse, InboxExportRequest, InboxExportResponse, InboxIngress,
    InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
    InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
    InboxIngressImportRequest, InboxIngressImportResponse, InboxIngressRequest,
    InboxIngressResponse, InboxSourceFileRecord, MailStore, MailStoreBootstrapRequest,
    MailStoreBootstrapResponse, MailStoreHealthSnapshot, MailStoreHealthSnapshotRequest,
    MailStoreHealthSnapshotResponse, MailStoreIngestReplayState,
    MailStoreLoadIngestReplayStateRequest, MailStoreLoadIngestReplayStateResponse,
    MailStoreLoadMessageRequest, MailStoreLoadMessageResponse, MailStoreLoadVisibilityStateRequest,
    MailStoreLoadVisibilityStateResponse, MailStoreMessageRecord,
    MailStoreRecordIngestReplayStateRequest, MailStoreRecordIngestReplayStateResponse,
    MailStoreRequest, MailStoreResponse, MailStoreTransactionRequest, MailStoreTransactionResponse,
    MailStoreUpsertMessageRequest, MailStoreUpsertMessageResponse,
    MailStoreUpsertVisibilityStateRequest, MailStoreUpsertVisibilityStateResponse,
    MailStoreVisibilityState, NotificationEvent, NotificationSink, ReconcileCoordinator,
    ReconcileRequest, ReconcileResult, RequestDispatcher, RosterStore, RosterStoreHealthSnapshot,
    RosterStoreHealthSnapshotRequest, RosterStoreHealthSnapshotResponse,
    RosterStoreLoadRosterRequest, RosterStoreLoadRosterResponse, RosterStoreQueryMembershipRequest,
    RosterStoreQueryMembershipResponse, RosterStoreReplaceRosterRequest,
    RosterStoreReplaceRosterResponse, RosterStoreRequest, RosterStoreResponse,
    RuntimeStatusSnapshot, ServerTransport, StatusSource, TaskStore,
    TaskStoreAttachMessageLinkRequest, TaskStoreAttachMessageLinkResponse,
    TaskStoreCreateTaskRequest, TaskStoreCreateTaskResponse, TaskStoreDetachMessageLinkRequest,
    TaskStoreDetachMessageLinkResponse, TaskStoreLoadTaskRequest, TaskStoreLoadTaskResponse,
    TaskStoreQueryTaskMetadataRequest, TaskStoreQueryTaskMetadataResponse,
    TaskStoreRecordAckTransitionRequest, TaskStoreRecordAckTransitionResponse, TaskStoreRequest,
    TaskStoreResponse, TaskStoreTaskMetadata, TaskStoreTaskRecord, TaskStoreUpdateTaskRequest,
    TaskStoreUpdateTaskResponse, WatchEventBatch, WatchEventSource, WatchSubscriptionRequest,
};
pub use config::AtmConfig;
pub use protocol::{FramePayload, RequestEnvelope, ResponseEnvelope};
