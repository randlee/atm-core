pub mod ack;
/// Public agent-address parsing and normalization helpers.
pub mod address;
/// Acknowledgement workflows for ack-required mailbox messages.
pub mod api;
/// `ATM_TEMP` scratch-root resolution and shared-host safety validation
/// (ADR-055).
pub mod atm_temp;
/// `$ATM_TEMP` TTL sweep config validation and one pure, `tokio`-free sweep
/// pass (ADR-055 decision (b)).
pub mod atm_temp_sweeper;
/// Phase R boundary traits and placeholder contract types.
pub mod boundary;
/// Hidden daemon-private ingress/export helpers used by concrete boundary
/// adapter crates.
#[doc(hidden)]
pub mod boundary_support;
/// Shared caller-context resolution and ATM-owned environment parsing helpers.
pub mod caller_context;
/// Mailbox cleanup workflows for read and acknowledged messages.
pub mod clear;
/// Internal configuration discovery and resolution helpers.
pub(crate) mod config;
/// Recipient delivery-channel classification (tmux/Herdr/Graft/bare) derived
/// from durable roster data, used by the queue-aware nudge dispatch seam.
pub mod delivery_channel;
/// Internal shared delivery-plan execution helpers for message-path unification.
pub(crate) mod delivery_execution;
/// Internal typed delivery-plan contracts owned by the message state machines.
pub(crate) mod delivery_plan;
/// Internal delivery-policy coordinator and event-family state machines.
pub(crate) mod delivery_policy;
/// Doctor-report types and health checks for the CLI surface.
pub mod doctor;
/// Shared ATM error types and recovery-oriented error helpers.
pub mod error;
/// Stable ATM-owned error-code registry used by core and CLI layers.
pub mod error_codes;
/// Thin graft-facing daemon client traits.
pub mod graft;
/// Public ATM home and team-path resolution helpers.
pub mod home;
/// Internal identity resolution and hook lookup helpers.
pub(crate) mod identity;
/// Bounded metadata queue query workflows and output models.
pub mod list;
/// Runtime-local endpoint metadata and capability validation for local HTTP.
pub mod local_http;
/// Log query and filtering types for the CLI log surface.
pub mod log;
/// Internal mailbox persistence and parsing helpers.
pub(crate) mod mailbox;
/// Internal model-registry plumbing reserved for follow-on work.
pub(crate) mod model_registry;
/// Rebuilds a receiver-hook dispatch from durable message-store state for a
/// deferred (`atm queue`) claim replay; the write-time planner never reloads.
pub mod nudge_dispatch;
/// Observability adapter traits and event payload types.
pub mod observability;
/// Transport-neutral peer-wire policy vocabulary selected at daemon launch.
pub mod peer_wire;
/// Internal atomic persistence helpers for shared mutable state files.
pub(crate) mod persistence;
/// Picker member projection (`atm teams --json --members`), PRD §4.2/§5a.
pub mod picker_projection;
/// Hidden process-liveness helpers shared across lock implementations.
#[doc(hidden)]
pub mod process;
/// Shared protocol DTOs used by boundary transport and adapter contracts.
pub mod protocol;
/// Shared authenticated/immutable write provenance validation.
pub mod provenance;
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
/// Transport-neutral, local-only message search query contract.
pub mod search;
/// Mailbox send workflows and request/response models.
pub mod send;
/// Send-To CLI-surface support: picker-recipient resolution, same-host/remote
/// classification, attachment staging, and transfer-script invocation
/// (ADR-055 decisions (c)-(g)).
pub mod send_to;
/// Internal service-owned seams that isolate retained command orchestration
/// from direct helper/path access.
pub(crate) mod service_runtime;
/// Transitional legacy store adapters used by the retained service runtime.
pub(crate) mod service_runtime_store;
/// Retained local team discovery, roster repair, and backup/restore workflows.
pub mod team_admin;
/// Pure resolution of template-declared workflow snapshots.
pub mod template_workflow;
/// Shared synthetic test identities and role constants used across crate tests.
#[doc(hidden)]
#[cfg(any(test, feature = "test-utils"))]
pub mod test_support;
/// Internal text-formatting helpers used by ATM core surfaces.
pub(crate) mod text;
/// Internal linear-thread helpers shared by send/read/ack workflow logic.
pub(crate) mod threading;
/// Cross-host transfer-script resolution and argv-array invocation contract
/// (ADR-055). Resolution and the safety check only; execution is wired in
/// by the `atm send` CLI surface.
pub mod transfer_script;
/// Hidden transport test utilities shared by CLI-layer tests.
#[doc(hidden)]
#[cfg(feature = "test-utils")]
pub mod transport;
/// Shared enums and semantic newtypes used across ATM core workflows.
pub mod types;
/// Generic local lifecycle projection over immutable workflow admission facts.
pub mod workflow_analytics;
/// First-party-only telemetry contract for workflow lifecycle projections.
pub mod workflow_telemetry;
/// Canonical write pipeline shared by `ack` and `send`; consumed only
/// through their facades, so the module itself stays crate-private.
pub(crate) mod write;

pub use api::{
    ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, DaemonApiClient,
    MAX_HTTP_REQUEST_BODY_BYTES, RequestDeadline,
};
pub use atm_storage::derive_ack_requirement;
pub use atm_storage::{GraftEndpointStoreError, GraftReceiverEndpointStore, GraftReceiverLease};
pub use atm_storage::{TemplateFrontmatter, TemplateSha};
pub use atm_temp::{
    AtmTemp, AtmTempError, EnvSource, ProcessEnvSource, resolve_atm_temp, send_to_staging_dir,
};
pub use atm_temp_sweeper::{
    EntryAge, EntryAgeSource, RealEntryAgeSource, SweepConfig, SweepReport, SweeperError,
    sweep_once, sweep_once_cancellable, sweep_once_with_age_source, validate_sweep_config,
};
#[allow(deprecated)]
pub use boundary::{
    AckTransition, AsyncMessageReceivedHookEmitter, BuiltInNudgeSinkTarget,
    BuiltInNudgeTemplateKind, ConfigDoctor, ConfigDoctorReport, ConfigIngress, ConfigLoadRequest,
    ConfigLoadResponse, DoctorFinding, HerdrNudgeTarget, InternalNudgeEnvelope,
    LoadMailMessageStateRequest, LoadMailMessageStateResponse, LocalSteerTarget, MailMessageState,
    MailStore, MailStoreDoctor, MailStoreDoctorReport, MailStoreHealthSnapshot,
    MailStoreMailboxMetadataCounts, MailStoreMailboxMetadataRow, Message, MessageFingerprint,
    MessageKey, MessageReceivedHookEmitter, NotificationEvent, NudgeTemplateOverrideStore,
    PostSendHookEvent, RenderedBody, ResolvedBuiltInNudgeTemplate, RosterEntry, RosterHarness,
    RosterMemberKind, RosterStore, RosterStoreDoctor, RosterStoreDoctorReport,
    RosterStoreHealthSnapshot, RuntimeStatusSnapshot, SourceSpan, StatusSource, TaskState,
    TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow, TemplateComposer,
    TemplateInspection, TemplateReference, TemplateReferenceKind, TemplateRoot, TemplateSource,
    UpsertMailMessageStateRequest, UpsertMailMessageStateResponse,
};
pub use config::AtmConfig;
pub use config::load_config as load_atm_config;
pub use config::types::GraftConfig;
pub use delivery_channel::{
    DeliveryChannel, GraftLeaseState, HerdrSession, LocalMessageReceivedBackend,
    classify_delivery_channel, local_message_received_backend,
};
/// Canonical stable import path for the retained thin graft-facing client
/// boundary. Shared advisory/session protocol DTOs are not part of the
/// accepted `atm-core` surface.
pub use graft::AtmGraftClient;
pub use picker_projection::{
    PICKER_MEMBERS_SCHEMA_VERSION, PickerMember, PickerMemberStatus, PickerMembersProjection,
    build_picker_members_projection,
};
pub use protocol::{RequestEnvelope, ResponseEnvelope};
pub use search::{SearchAggregateInput, SearchHit, SearchInput, SearchRequest, SearchResponse};
pub use send_to::{
    AddressResolutionError, PICKER_OUTPUT_SCHEMA_VERSION, PickerOutput, PickerOutputError,
    PickerRecipientId, ROSTER_HOST_METADATA_KEY, RecipientLocality, classify_recipient_locality,
    format_attachment_note, member_host, parse_picker_output, resolve_picker_recipient,
    stage_same_host_attachments, validate_landed_dir_stdout,
};
// The canonical `GraftEndpointStoreError` -> `AtmError` mapping. Every
// dispatch path (HTTP handlers, the retained-runtime bridge) must produce the
// same `AtmError` — and therefore the same HTTP status — for a given store
// error; see the doc comment on `service_runtime::graft_store_error`.
pub use service_runtime::graft_store_error;
pub use service_runtime::{
    LocalFileNonClaudeOutbound, LocalServiceRuntime, with_default_local_service_runtime,
};
pub use transfer_script::{
    ConfiguredTransferScript, TransferInvocation, TransferScript, TransferScriptKind,
    resolve_transfer_script,
};
pub use workflow_analytics::{
    LifecycleObservation, WorkflowFact, WorkflowProjectionRequest, WorkflowSelector,
    project_lifecycles,
};
pub use workflow_telemetry::{
    NoopWorkflowTelemetrySink, WorkflowTelemetryError, WorkflowTelemetryObservation,
    WorkflowTelemetryRecord, WorkflowTelemetrySink,
};
