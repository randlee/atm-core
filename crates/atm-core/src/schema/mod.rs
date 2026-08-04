//! Shared schema DTOs for Claude inbox compatibility, team config, and roster members.

pub mod agent_member;
pub mod inbox_message;
pub mod permissions;
pub mod settings;
pub mod team_config;

pub use agent_member::{
    AgentMember, HOME_DIR_METADATA_KEY, HomeDirPath, WORKSPACE_ROOT_METADATA_KEY,
    canonical_graft_root, canonical_home_dir, compatible_home_dir,
};
pub use atm_storage::contract::AgentType;
pub(crate) use inbox_message::{
    AckIntentFields, authenticated_source_host, clear_transport_delivery_metadata,
    peer_outbound_host, peer_reply_host, set_authenticated_source_host, set_peer_outbound_write,
    set_peer_reply_host,
};
pub use inbox_message::{
    AlertKind, AtmMessageId, InboxMessage, PendingAck, ThreadMode, display_inbound_sender,
};
pub use team_config::TeamConfig;
