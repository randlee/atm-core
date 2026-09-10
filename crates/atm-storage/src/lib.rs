//! Shared audited storage contract and canonical storage-facing domain types
//! for ATM backends and their callers.

pub mod analyst_query;
pub mod attention;
pub mod contract;
pub mod diagnostics;
pub mod error;
mod error_catalog;
pub mod error_codes;
pub mod factory;
pub mod peer_catalog_audit;
mod peer_contract;
pub mod request_budget;
pub mod schema;
pub mod search;
mod task_ledger;
pub mod task_mutation;
pub mod task_state;
pub mod task_store;
pub mod template_catalog;
pub mod template_workflow;
/// Shared no-op test doubles for storage contract traits (RBQA-F002/F003).
#[doc(hidden)]
#[cfg(any(test, feature = "test-utils"))]
pub mod testing;
pub mod tls;
pub mod types;
mod validation;

/// Canonical protocol identities shared by storage fixtures and higher layers.
pub mod roles {
    pub const TEAM_ATM_DEV: &str = "atm-dev";
    pub const ROLE_TEAM_LEAD: &str = "team-lead";
    pub const ROLE_QUALITY_MANAGER: &str = "quality-mgr";
    pub const ROLE_WORKER: &str = "worker";
}

pub use analyst_query::{AnalystQueryRow, AnalystQueryStore, AnalystQueryValue};
pub use attention::{
    AsyncAttentionScheduleStore, AttentionCandidates, AttentionCursor, AttentionFinalizeOutcome,
    AttentionFinalizeRequest, AttentionItem, AttentionLane, AttentionReservation,
    AttentionReservationRequest, AttentionReservationStatus, AttentionScheduleStore,
    AttentionSelection, EphemeralMessageCandidate, IdleOpportunity, IdleOpportunityId,
    PersistentTaskCandidate, select_attention_item,
};
pub use contract::{
    AckRequirementState, AckTransition, AcknowledgementCommit, AcknowledgementReplyBuilder,
    AcknowledgementSource, AgentType, AsyncGraftReceiverEndpointStore, AsyncMailboxReader,
    AsyncMessageStore, AsyncTaskLedgerReader, BuiltInNudgeTemplateKind, CertificateFingerprint,
    GraftEndpointStoreError, GraftReceiverEndpointStore, GraftReceiverLease,
    GraftReceiverRegistration, HttpsInterface, LocalCertificate, MAX_NUDGE_ATTEMPTS,
    MailMessageState, MailboxBucketCounts, MailboxScope, Message, MessageFingerprint, MessageKey,
    MessageQuery, MessageReceivedEvent, MessageStore, NudgeClaim, NudgeTemplateOverrideStore,
    PeerConfigStore, PendingNudgeStore, PrivateKeyRef, ReadDeadline, ReadLaneError,
    RosterChangedEvent, RosterHarness, RosterMember, RosterMemberEphemeralState, RosterMemberKind,
    RosterRuntimeIdentity, RosterRuntimeMirror, RosterRuntimeMutationOutcome,
    RosterRuntimeObservation, RosterRuntimeObservationUpdate, RosterSnapshot, RosterStateRevision,
    RosterStore, RosterUniqueName, RuntimeMemberState, RuntimeObservationAvailability,
    RuntimeObservationSource, StorageNotifier, TeamNudgeTemplateOverrideMode,
    TeamNudgeTemplateOverrideRow, TrustedPeer, derive_ack_requirement,
    roster_unique_name_collision_error, roster_unique_name_collisions, roster_write_delta,
    team_scoped_roster_unique_name_collisions,
};
pub use diagnostics::{
    DIAGNOSTIC_QUERY_DEFAULT_LIMIT, DIAGNOSTIC_QUERY_MAX_LIMIT, DiagnosticCursor, DiagnosticEvent,
    DiagnosticQuery, DiagnosticRecordError, DiagnosticTimelineStore,
};
pub use error::AtmError;
pub use error_codes::AtmErrorCode;
pub use factory::{
    EffectiveReaderPool, EffectiveReaderPoolMetrics, StorageFactory, StorageHandleParts,
    StorageHandles, WriteThroughRosterStore,
};
pub use peer_catalog_audit::TrustedPeerCatalogAudit;
pub use roles::ROLE_WORKER;
pub use schema::{AlertKind, AtmMessageId, InboxMessage, MessageEnvelope, PendingAck, ThreadMode};
pub use search::{
    AsyncMessageSearchStore, InMemoryMessageSearchStore, MessageSearchPage, MessageSearchQuery,
    MessageSearchStore, SearchAggregate, SearchAtom, SearchCursor, SearchDeadline,
    SearchExpression, SearchFilters, SearchGroup, SearchGroupBy, SearchGroupField, SearchKey,
    SearchLimit, SearchMatchField, SearchMetadataMatch, SearchPageRequest, SearchResultKey,
    SearchTimestampField, SearchValue, SimpleAggregate, StoredSearchAddress, StoredSearchMatch,
    StoredWorkflowMetadata, TimeRange,
};
pub use task_mutation::{
    AsyncTaskMutationStore, AsyncTaskSchedulerAuditStore, PreparedAssignment, PreparedMessage,
    TaskLeadNotificationAuditRequest, TaskMutationDeadline, TaskMutationOutcome,
    TaskMutationRequest, TaskOperation, TaskReminderAuditRequest,
};
pub use task_state::{
    AssignmentAttempt, DAEMON_ACTOR_NAME, LogicalTaskRow, TaskAbortReason, TaskActor,
    TaskAssignmentAttempt, TaskEvent, TaskEventKind, TaskEventMarker, TaskEventRow,
    TaskLedgerScope, TaskLifecycleAction, TaskLifecycleEventKind, TaskLifecycleEventRow,
    TaskLifecycleState, TaskLifecycleTransition, TaskOperationId, TaskOutcome, TaskPriority,
    TaskRejected, TaskRow, TaskState, Transition, admit, lifecycle_transition, transition,
};
pub use task_store::{
    DummyTaskStore, EscalationScope, MAX_ESCALATION_RECIPIENTS, MessageWriteOrigin,
    ReminderOutcome, TASK_STALLED_REMINDER_THRESHOLD, TaskStore,
};
pub use template_catalog::{
    DecomposedMessageAdmission, DecomposedMessageAdmissionOutcome, DecomposedMessageRecord,
    MergedVarsJson, MessageBody, StoredTemplate, TemplateCatalogStore, TemplateFirstSeen,
    TemplateListFilter, TemplateMessageAdmission, TemplateOutputFormat, TemplateRegistration,
    TemplateRegistrationOutcome, TemplateSummary, WorkflowAdmission,
};
pub use template_workflow::{
    DerivedTag, EffectiveTag, InstanceTag, MessageTagProvenance, RESERVED_DERIVED_TAG_PREFIXES,
    TemplateTag, TemplateTagDeclaration, TemplateVariableName, TemplateWorkflowDeclaration,
    WorkflowIteration, WorkflowScopeId, WorkflowScopeKind, WorkflowSnapshot, WorkflowStage,
    WorkflowState, WorkflowTransition,
};
pub use tls::{
    PinnedClientVerifier, TlsIdentity, certificate_fingerprint, certificate_valid_now,
    install_tls_provider, normalize_fingerprint,
};
pub use types::{
    AgentId, AgentIdentity, AgentName, ChatId, HostName, IsoTimestamp, LOCAL_CAPABILITY_BYTES,
    LocalCapability, MemberKey, ModelName, OwnerGeneration, PaneId, TaskId, TeamName,
    TemplateFrontmatter, TemplateSha,
};
pub use validation::{validate_agent_at_team, validate_path_segment};
