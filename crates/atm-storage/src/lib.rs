//! Shared audited storage contract and canonical storage-facing domain types
//! for ATM backends and their callers.

pub mod contract;
pub mod error;
mod error_catalog;
pub mod error_codes;
pub mod factory;
pub mod schema;
pub mod types;
mod validation;

// Protocol role identity for worker agents used in shared storage fixtures.
pub const ROLE_WORKER: &str = "worker";
pub use contract::{
    AckRequirementState, AckTransition, AgentType, BuiltInNudgeTemplateKind,
    CertificateFingerprint, HttpsInterface, LocalCertificate, MAX_PEER_SYNC_BATCH_MESSAGES,
    MAX_PEER_SYNC_MESSAGE_AGE, MailMessageState, MailboxBucketCounts, Message, MessageFingerprint,
    MessageKey, MessageQuery, MessageReceivedEvent, MessageStore, NudgeTemplateOverrideStore,
    OutboundMessageQuery, PeerConfigStore, PeerSyncPolicy, PrivateKeyRef, RosterChangedEvent,
    RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot, RosterStore, StorageNotifier,
    StoredPeerWrite, TaskState, TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow,
    TrustedPeer, derive_ack_requirement,
};
pub use error::AtmError;
pub use error_codes::AtmErrorCode;
pub use factory::{StorageFactory, StorageHandles};
pub use schema::{AlertKind, AtmMessageId, InboxMessage, MessageEnvelope, PendingAck, ThreadMode};
pub use types::{
    AgentId, AgentIdentity, AgentName, ChatId, HostName, IsoTimestamp, ModelName, PaneId, TaskId,
    TeamName,
};
pub use validation::{validate_agent_at_team, validate_path_segment};
