//! Shared audited storage contract and canonical storage-facing domain types
//! for ATM backends and their callers.

pub mod contract;
pub mod error;
pub mod error_codes;
pub mod schema;
pub mod types;
mod validation;

// Protocol role identity for worker agents used in shared storage fixtures.
pub const ROLE_WORKER: &str = "worker";
pub use contract::{
    AckRequirementState, AckTransition, AddPeerInterfaceCommand, AgentType, AllowHostCommand,
    AllowedHostName, AllowedHostRow, AllowedHostStore, BuiltInNudgeTemplateKind, MailMessageState,
    Message, MessageFingerprint, MessageKey, MessageQuery, MessageReceivedEvent, MessageStore,
    NudgeTemplateOverrideStore, PeerInterfaceBindingUpdate, PeerInterfaceConfigStore,
    PeerInterfaceKey, PeerInterfaceKind, PeerInterfaceRow, RosterChangedEvent, RosterHarness,
    RosterMember, RosterMemberKind, RosterSnapshot, RosterStore, StorageNotifier, TaskState,
    TeamNudgeTemplateOverrideMode, TeamNudgeTemplateOverrideRow, UpdatePeerInterfaceCommand,
    derive_ack_requirement,
};
pub use error::{AtmError, AtmErrorKind};
pub use error_codes::AtmErrorCode;
pub use schema::{AlertKind, AtmMessageId, InboxMessage, MessageEnvelope, PendingAck, ThreadMode};
pub use types::{AgentId, AgentName, IsoTimestamp, ModelName, PaneId, TaskId, TeamName};
