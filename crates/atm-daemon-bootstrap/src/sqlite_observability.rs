//! Production SQLite diagnostics retained by the AW.1 tracing bridge.

use atm_storage::AtmError;
use atm_storage_rusqlite::{
    SqliteObservability, SqliteObservabilityEvent, SqliteObservabilityOutcome,
};

#[derive(Debug, Default)]
pub(crate) struct DaemonSqliteObservability;

impl SqliteObservability for DaemonSqliteObservability {
    fn emit(&self, event: SqliteObservabilityEvent) -> Result<(), AtmError> {
        let code = diagnostic_code(event.action, event.outcome);
        match event.outcome {
            SqliteObservabilityOutcome::Timeout => tracing::warn!(
                origin = "sqlite",
                code,
                action = event.action,
                outcome = event.outcome.as_str(),
                "SQLite durable-state operation timed out"
            ),
            SqliteObservabilityOutcome::Failed => tracing::error!(
                origin = "sqlite",
                code,
                action = event.action,
                outcome = event.outcome.as_str(),
                "SQLite durable-state operation failed"
            ),
        }
        Ok(())
    }
}

fn diagnostic_code(action: &str, outcome: SqliteObservabilityOutcome) -> &'static str {
    if action.contains("checkpoint") {
        "ATM_SQLITE_WAL_CHECKPOINT_FAILED"
    } else if action.contains("queue")
        || action.contains("submit") && matches!(outcome, SqliteObservabilityOutcome::Timeout)
    {
        "ATM_SQLITE_QUEUE_SATURATED"
    } else if matches!(outcome, SqliteObservabilityOutcome::Timeout) {
        "ATM_SQLITE_WRITER_TIMEOUT"
    } else {
        "ATM_SQLITE_WRITE_FAILED"
    }
}

#[cfg(test)]
mod tests {
    use super::{DaemonSqliteObservability, diagnostic_code};
    use atm_storage_rusqlite::SqliteObservabilityOutcome;

    #[test]
    fn maps_writer_and_checkpoint_failures_to_stable_codes() {
        assert_eq!(
            diagnostic_code("writer_reply", SqliteObservabilityOutcome::Timeout),
            "ATM_SQLITE_WRITER_TIMEOUT"
        );
        assert_eq!(
            diagnostic_code(
                "writer_shutdown_checkpoint",
                SqliteObservabilityOutcome::Failed
            ),
            "ATM_SQLITE_WAL_CHECKPOINT_FAILED"
        );
        let _ = DaemonSqliteObservability;
    }
}
