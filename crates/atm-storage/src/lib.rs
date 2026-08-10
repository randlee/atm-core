//! Shared audited storage contract and canonical storage-facing domain types
//! for ATM backends and their callers.

pub mod contract;
pub mod error;
mod error_catalog;
pub mod error_codes;
pub mod factory;
pub mod schema;
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
    MessageReceivedEvent, MessageStore, NudgeTemplateOverrideStore, OutboundMessageQuery,
    PeerConfigStore, PrivateKeyRef, RosterChangedEvent, RosterHarness, RosterMember,
    RosterMemberKind, RosterSnapshot, RosterStore, StorageNotifier, StoredPeerWrite, TaskState,
    TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow, TrustedPeer,
    derive_ack_requirement,
};
pub use error::AtmError;
pub use error_codes::AtmErrorCode;
pub use factory::{StorageFactory, StorageHandles};
pub use schema::{AlertKind, AtmMessageId, InboxMessage, MessageEnvelope, PendingAck, ThreadMode};
pub use tls::{
    PinnedClientVerifier, TlsIdentity, certificate_fingerprint, install_tls_provider,
    normalize_fingerprint,
};
pub use types::{
    AgentId, AgentIdentity, AgentName, ChatId, HostName, IsoTimestamp, ModelName, PaneId, TaskId,
    TeamName, TemplateFrontmatter, TemplateSha,
};
pub use validation::{validate_agent_at_team, validate_path_segment};
