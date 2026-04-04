use serde::Serialize;

use crate::error::AtmError;
use crate::schema::LegacyMessageId;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommandEvent {
    pub command: &'static str,
    pub action: &'static str,
    pub outcome: &'static str,
    pub team: String,
    pub agent: String,
    pub sender: String,
    pub message_id: Option<LegacyMessageId>,
    pub requires_ack: bool,
    pub dry_run: bool,
    pub task_id: Option<String>,
}

pub trait ObservabilityPort {
    fn emit_command_event(&self, event: CommandEvent) -> Result<(), AtmError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NullObservability;

impl ObservabilityPort for NullObservability {
    fn emit_command_event(&self, _event: CommandEvent) -> Result<(), AtmError> {
        Ok(())
    }
}
