use anyhow::Result;
use atm_core::error::AtmError;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, CommandEvent, LogTailSession,
    ObservabilityPort,
};
use tracing::info;

#[derive(Debug, Default, Clone, Copy)]
pub struct CliObservability;

pub fn init() -> Result<CliObservability> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .try_init();
    Ok(CliObservability)
}

impl ObservabilityPort for CliObservability {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
        let message_id = event.message_id.map(|value| value.to_string());
        info!(
            command = event.command,
            action = event.action,
            outcome = event.outcome,
            team = event.team,
            agent = event.agent,
            sender = event.sender,
            message_id = message_id.as_deref().unwrap_or(""),
            requires_ack = event.requires_ack,
            dry_run = event.dry_run,
            task_id = event.task_id.as_deref().unwrap_or(""),
            "atm command event"
        );
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Err(AtmError::observability_query(
            "shared observability query adapter is not wired yet",
        ))
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Err(AtmError::observability_follow(
            "shared observability follow adapter is not wired yet",
        ))
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        Err(AtmError::observability_health(
            "shared observability health adapter is not wired yet",
        ))
    }
}
