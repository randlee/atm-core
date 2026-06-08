/// Acknowledgement workflows for ack-required mailbox messages.
pub mod ack;
/// Public agent-address parsing and normalization helpers.
pub mod address;
/// Phase R boundary traits and placeholder contract types.
pub mod boundary;
/// Hidden daemon-private ingress/export helpers used by concrete boundary
/// adapter crates.
#[doc(hidden)]
pub mod boundary_support;
/// Mailbox cleanup workflows for read and acknowledged messages.
pub mod clear;
/// Internal configuration discovery and resolution helpers.
pub(crate) mod config;
/// Internal shared delivery-plan execution helpers for message-path unification.
pub(crate) mod delivery_execution;
/// Internal typed delivery-plan contracts owned by the message state machines.
pub(crate) mod delivery_plan;
/// Internal delivery-policy coordinator and event-family state machines.
pub(crate) mod delivery_policy;
/// Hidden daemon-facing wrapper surface over crate-private boundary helpers.
#[doc(hidden)]
pub mod direct_boundaries;
/// Doctor-report types and health checks for the CLI surface.
pub mod doctor;
/// Shared ATM error types and recovery-oriented error helpers.
pub mod error;
/// Stable ATM-owned error-code registry used by core and CLI layers.
pub mod error_codes;
/// Thin graft-facing daemon client traits and typed session DTOs.
pub mod graft;
/// Public ATM home and team-path resolution helpers.
pub mod home;
/// Internal identity resolution and hook lookup helpers.
pub(crate) mod identity;
/// Bounded metadata queue query workflows and output models.
pub mod list;
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
/// Hidden process-liveness helpers shared across lock implementations.
#[doc(hidden)]
pub mod process;
/// Shared protocol DTOs used by boundary transport and adapter contracts.
pub mod protocol;
/// Mailbox read/query workflows and output models.
pub mod read;
/// Reserved production role constants shared across runtime and tests.
pub mod roles;
/// Hidden bounded installation hooks for daemon composition/bootstrap and test
/// support crates.
#[doc(hidden)]
pub mod runtime_install_hooks;
/// Public mailbox and team schema types shared with CLI tests and adapters.
pub mod schema;
/// Mailbox send workflows and request/response models.
pub mod send;
/// Internal service-owned seams that isolate retained command orchestration
/// from direct helper/path access.
pub(crate) mod service_runtime;
/// Transitional legacy store adapters used by the retained service runtime.
pub(crate) mod service_runtime_store;
/// Retained local team discovery, roster repair, and backup/restore workflows.
pub mod team_admin;
/// Shared synthetic test identities and role constants used across crate tests.
#[doc(hidden)]
pub mod test_support;
/// Internal text-formatting helpers used by ATM core surfaces.
pub(crate) mod text;
/// Internal linear-thread helpers shared by send/read/ack workflow logic.
pub(crate) mod threading;
/// Hidden transport test utilities shared by CLI-layer tests.
#[doc(hidden)]
#[cfg(feature = "test-utils")]
pub mod transport;
/// Shared enums and semantic newtypes used across ATM core workflows.
pub mod types;

pub use config::load_claude_team_config_document;
/// Internal ATM-owned workflow-state helpers shared across mailbox services.
pub(crate) mod workflow;

#[allow(
    deprecated,
    reason = "AC.4 preserves the legacy mail bootstrap surface as a temporary compile bridge during storage-boundary adoption."
)]
pub use boundary::{
    AckTransition, AtmProtocol, ProjectionExport,
    ProjectionExportReexportMessageRequest, ProjectionExportReexportMessageResponse,
    ProjectionExportRequest, ProjectionExportResponse, ProjectionRoster, ProjectionRosterMember,
    ClientTransport, ConfigDoctor, ConfigDoctorReport, ConfigIngress, ConfigLoadRequest,
    ConfigLoadResponse, DoctorFinding, LoadMailMessageStateRequest,
    LoadMailMessageStateResponse, MailMessageState, MailStore, MailStoreBootstrapRequest,
    MailStoreBootstrapResponse, MailStoreDoctor, MailStoreDoctorReport, MailStoreHealthSnapshot,
    MailStoreIngestReplayState, MailStoreMailboxMetadataCounts, MailStoreMailboxMetadataRow,
    MailStoreMessageRecord, MessageFingerprint, MessageKey, NotificationEvent, NotificationSink,
    ReconcileCoordinator, ReconcileRequest, ReconcileResult, RemoteReplayStateRecord,
    RemoteReplayStore, RequestDispatcher, RosterHarness, RosterMemberKind, RosterMemberRecord,
    RosterStore, RosterStoreDoctor, RosterStoreDoctorReport, RosterStoreHealthSnapshot,
    RuntimeStatusSnapshot, RuntimeStorageFinalizer, ServerTransport, SourceFileRecord,
    SourceIngress, SourceIngressDiagnosticsRequest, SourceIngressDiagnosticsResponse,
    SourceIngressIdentityFingerprintRequest, SourceIngressIdentityFingerprintResponse,
    SourceIngressImportRequest, SourceIngressImportResponse, SourceIngressRequest,
    SourceIngressResponse, StatusSource, TaskState, TaskStore,
    TaskStoreAttachMessageLinkRequest, TaskStoreAttachMessageLinkResponse,
    TaskStoreCreateTaskRequest, TaskStoreCreateTaskResponse, TaskStoreDetachMessageLinkRequest,
    TaskStoreDetachMessageLinkResponse, TaskStoreDoctor, TaskStoreDoctorReport,
    TaskStoreLoadTaskRequest, TaskStoreLoadTaskResponse, TaskStoreQueryTaskMetadataRequest,
    TaskStoreQueryTaskMetadataResponse, TaskStoreRecordAckTransitionRequest,
    TaskStoreRecordAckTransitionResponse, TaskStoreRequest, TaskStoreResponse,
    TaskStoreTaskMetadata, TaskStoreTaskRecord, TaskStoreUpdateTaskRequest,
    TaskStoreUpdateTaskResponse, UpsertMailMessageStateRequest, UpsertMailMessageStateResponse,
    WatchEventBatch, WatchEventSource, WatchSubscriptionRequest,
};
pub use config::AtmConfig;
pub use config::load_config as load_atm_config;
pub use config::types::GraftConfig;
/// Canonical stable import path for the public graft-facing client/session
/// boundary. External consumers, including `atm-graft`, should import these
/// types from `atm_core::...` rather than reaching into the module path.
pub use graft::{
    AdvisoryBatchLimit, AdvisoryDrainRequest, AdvisoryDrainResponse, AdvisoryEvent,
    AdvisoryFetchRequest, AdvisoryFetchResponse, AdvisorySession, AdvisorySessionId,
    AdvisorySessionPort, AdvisorySessionRegistrationRequest, AdvisorySessionRegistrationResponse,
    AdvisorySessionState, AdvisorySessionUnregistrationRequest,
    AdvisorySessionUnregistrationResponse, AdvisoryStreamRequest, AdvisoryStreamResponse,
    AtmGraftClient,
};
pub use protocol::{FramePayload, RequestEnvelope, ResponseEnvelope};
pub use service_runtime::{
    LocalFileNonClaudeOutbound, LocalFileNotificationSink, LocalServiceRuntime,
};
