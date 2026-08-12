//! Shared audited storage contract and canonical storage-facing domain types
//! for ATM backends and their callers.

pub mod contract;
pub mod error;
mod error_catalog;
pub mod error_codes;
pub mod factory;
pub mod schema;
pub mod search;
pub mod template_catalog;
pub mod tls;
pub mod types;
mod validation;

// Protocol role identity for worker agents used in shared storage fixtures.
pub const ROLE_WORKER: &str = "worker";
pub use contract::{
    AckRequirementState, AckTransition, AcknowledgementCommit, AcknowledgementReplyBuilder,
    AcknowledgementSource, AgentType, AsyncMessageStore, BuiltInNudgeTemplateKind,
    CertificateFingerprint, HttpsInterface, LocalCertificate, MailMessageState,
    MailboxBucketCounts, Message, MessageFingerprint, MessageKey, MessageQuery,
    MessageReceivedEvent, MessageStore, NudgeTemplateOverrideStore, PeerConfigStore, PrivateKeyRef,
    RosterChangedEvent, RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot, RosterStore,
    StorageNotifier, TaskState, TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow,
    TrustedPeer, derive_ack_requirement,
};
pub use error::AtmError;
pub use error_codes::AtmErrorCode;
pub use factory::{StorageFactory, StorageHandleParts, StorageHandles};
pub use schema::{AlertKind, AtmMessageId, InboxMessage, MessageEnvelope, PendingAck, ThreadMode};
pub use search::{
    AsyncMessageSearchStore, InMemoryMessageSearchStore, MessageSearchPage, MessageSearchQuery,
    MessageSearchStore, SearchAggregate, SearchAtom, SearchCursor, SearchDeadline,
    SearchExpression, SearchFilters, SearchGroup, SearchGroupBy, SearchGroupField, SearchKey,
    SearchLimit, SearchMatchField, SearchMetadataMatch, SearchPageRequest, SearchResultKey,
    SearchTimestampField, SearchValue, SimpleAggregate, StoredSearchAddress, StoredSearchMatch,
    TimeRange,
};
pub use template_catalog::{
    DecomposedMessageAdmission, DecomposedMessageAdmissionOutcome, DecomposedMessageRecord,
    MergedVarsJson, MessageBody, StoredTemplate, TemplateCatalogStore, TemplateFirstSeen,
    TemplateListFilter, TemplateMessageAdmission, TemplateRegistration,
    TemplateRegistrationOutcome, TemplateSummary,
};
pub use tls::{
    PinnedClientVerifier, TlsIdentity, certificate_fingerprint, install_tls_provider,
    normalize_fingerprint,
};
pub use types::{
    AgentId, AgentIdentity, AgentName, ChatId, HostName, IsoTimestamp, ModelName, PaneId, TaskId,
    TeamName, TemplateFrontmatter, TemplateSha,
};
pub use validation::{validate_agent_at_team, validate_path_segment};
