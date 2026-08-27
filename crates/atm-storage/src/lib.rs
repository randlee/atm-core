//! Shared audited storage contract and canonical storage-facing domain types
//! for ATM backends and their callers.

pub mod analyst_query;
pub mod contract;
pub mod error;
mod error_catalog;
pub mod error_codes;
pub mod factory;
pub mod schema;
pub mod search;
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
pub use contract::{
    AckRequirementState, AckTransition, AcknowledgementCommit, AcknowledgementReplyBuilder,
    AcknowledgementSource, AgentType, AsyncMessageStore, BuiltInNudgeTemplateKind,
    CertificateFingerprint, GraftEndpointStoreError, GraftReceiverEndpointStore,
    GraftReceiverLease, GraftReceiverRegistration, HttpsInterface, LocalCertificate,
    MAX_NUDGE_ATTEMPTS, MailMessageState, MailboxBucketCounts, Message, MessageFingerprint,
    MessageKey, MessageQuery, MessageReceivedEvent, MessageStore, NudgeClaim,
    NudgeTemplateOverrideStore, PeerConfigStore, PendingNudgeStore, PrivateKeyRef,
    RosterChangedEvent, RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot, RosterStore,
    StorageNotifier, TaskState, TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow,
    TrustedPeer, derive_ack_requirement,
};
pub use error::AtmError;
pub use error_codes::AtmErrorCode;
pub use factory::{StorageFactory, StorageHandleParts, StorageHandles};
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
