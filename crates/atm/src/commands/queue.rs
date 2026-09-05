use anyhow::Result;
use atm_core::send::NudgeMode;
use clap::Args;

use crate::commands::send::SendCommand;
use crate::observability::CliObservability;

/// Queue one ATM mailbox message for deferred recipient notification.
#[derive(Debug, Args)]
pub struct QueueCommand {
    #[command(flatten)]
    pub(crate) inner: SendCommand,
}

impl QueueCommand {
    /// Execute the shared send surface with deferred nudge delivery.
    pub(crate) async fn run(self, observability: &CliObservability) -> Result<()> {
        self.inner
            .run_with_mode(observability, NudgeMode::Deferred)
            .await
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn queue_inherits_the_task_completion_flag_from_send() {
        crate::commands::Cli::try_parse_from([
            "atm",
            "queue",
            "cipher",
            "--task-complete",
            "t-42",
            "--stdin",
        ])
        .expect("queue inherits flattened send flags");
    }
}
