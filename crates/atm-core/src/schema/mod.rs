//! Shared schema DTOs for Claude inbox compatibility, team config, and roster members.

pub mod agent_member;
pub mod inbox_message;
pub mod permissions;
pub mod settings;
pub mod team_config;

pub use agent_member::{AgentMember, HOME_DIR_METADATA_KEY};
pub use atm_storage::contract::AgentType;
pub use inbox_message::{AlertKind, AtmMessageId, InboxMessage, PendingAck, ThreadMode};
pub use team_config::TeamConfig;
