use std::sync::Arc;

use atm_core::error::AtmError;
#[cfg(test)]
use atm_core::error::AtmErrorCode;
#[cfg(test)]
use atm_runtime::RuntimeSqliteOutcome;
use atm_runtime::{RuntimeSqliteEvent, RuntimeSqliteObserver};

use crate::DaemonSubsystem;
use crate::daemon_runtime_observability::{
    DaemonActionName, DaemonOutcomeLabel, DaemonRuntimeObservability,
};

#[derive(Clone)]
pub(crate) struct DaemonRuntimeSqliteObserver {
    observability: Arc<dyn DaemonRuntimeObservability>,
}

impl std::fmt::Debug for DaemonRuntimeSqliteObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonRuntimeSqliteObserver")
            .finish_non_exhaustive()
    }
}

impl DaemonRuntimeSqliteObserver {
    pub(crate) fn new(observability: Arc<dyn DaemonRuntimeObservability>) -> Self {
        Self { observability }
    }
}

impl RuntimeSqliteObserver for DaemonRuntimeSqliteObserver {
    fn emit_sqlite_event(&self, event: RuntimeSqliteEvent) -> Result<(), AtmError> {
        let action = DaemonActionName::new(event.action()).map_err(|source| {
            AtmError::observability_emit("failed to validate ATM daemon sqlite subsystem action")
                .with_source(source)
        })?;
        let outcome = DaemonOutcomeLabel::new(event.outcome().as_str()).map_err(|source| {
            AtmError::observability_emit("failed to validate ATM daemon sqlite subsystem outcome")
                .with_source(source)
        })?;
        self.observability.emit_subsystem_event(
            DaemonSubsystem::Composition,
            &action,
            &outcome,
            event.message(),
            event.error_code(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_failure_updates_retained_log() {
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&atm_home).expect("atm home");
        let log_dir = atm_core::home::host_log_dir_from_home(&atm_home);
        let observability = Arc::new(
            crate::test_observability::TestDaemonObservability::new(log_dir)
                .expect("test observability"),
        );
        let sqlite_observer = DaemonRuntimeSqliteObserver::new(observability.clone());

        sqlite_observer
            .emit_sqlite_event(RuntimeSqliteEvent::new(
                "writer_submit",
                RuntimeSqliteOutcome::Timeout,
                "sqlite writer submission queue did not accept a write within 10s",
                Some(AtmErrorCode::DaemonUnavailable),
            ))
            .expect("emit sqlite event");

        observability
            .wait_for_message_contains(
                "\"subsystem\":\"composition\"",
                std::time::Duration::from_secs(1),
            )
            .expect("retained log composition message");
        observability
            .wait_for_message_contains("\"outcome\":\"timeout\"", std::time::Duration::from_secs(1))
            .expect("retained log sqlite timeout outcome");
        observability
            .wait_for_message_contains(
                "sqlite writer submission queue did not accept a write",
                std::time::Duration::from_secs(1),
            )
            .expect("retained log sqlite detail");
    }
}
