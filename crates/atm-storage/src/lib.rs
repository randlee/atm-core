pub mod contract;
pub mod error;
pub mod error_codes;
pub mod schema;
pub mod types;
mod validation;

// Protocol role identity for the team-lead agent.
pub const ROLE_TEAM_LEAD: &str = "team-lead";

pub use contract::{
    AckTransition, Message, MessageKey, MessageQuery, MessageReceivedEvent, MessageStore,
    RosterChangedEvent, RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot, RosterStore,
    StorageNotifier, TaskState,
};
pub use error::{AtmError, AtmErrorKind};
pub use error_codes::AtmErrorCode;
pub use schema::{AlertKind, AtmMessageId, MessageEnvelope, PendingAck, ThreadMode};
pub use types::{AgentId, AgentName, IsoTimestamp, ModelName, PaneId, TaskId, TeamName};
