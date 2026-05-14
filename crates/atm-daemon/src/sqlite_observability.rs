use std::sync::Arc;

use atm_core::error::AtmError;
use atm_rusqlite::{SqliteObservability, SqliteObservabilityEvent, SqliteObservabilityOutcome};

use crate::DaemonRuntimeObservability;
use crate::runtime_status_cache::RuntimeStatusCache;

#[derive(Clone)]
pub(crate) struct DaemonSqliteObservability {
    observability: Arc<dyn DaemonRuntimeObservability>,
    status_cache: RuntimeStatusCache,
}

impl std::fmt::Debug for DaemonSqliteObservability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonSqliteObservability")
            .field("status_cache", &self.status_cache)
            .finish_non_exhaustive()
    }
}

impl DaemonSqliteObservability {
    pub(crate) fn new(
        observability: Arc<dyn DaemonRuntimeObservability>,
        status_cache: RuntimeStatusCache,
    ) -> Self {
        Self {
            observability,
            status_cache,
        }
    }
}

impl SqliteObservability for DaemonSqliteObservability {
    fn emit(&self, event: SqliteObservabilityEvent) -> Result<(), AtmError> {
        self.observability.emit_subsystem_event(
            "sqlite",
            event.action,
            event.outcome.as_str(),
            &event.message,
            event.error_code,
        )?;
        match event.outcome {
            SqliteObservabilityOutcome::Ok => {}
            SqliteObservabilityOutcome::Failed | SqliteObservabilityOutcome::Timeout => {
                self.status_cache
                    .mark_sqlite_unavailable_with_detail(format!(
                        "[{}] {}",
                        event.action, event.message
                    ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::doctor::DoctorSeverity;
    use atm_core::error_codes::AtmErrorCode;

    #[test]
    fn sqlite_failure_updates_runtime_status_and_retained_log() {
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        let atm_home = tempdir.path().join("atm-home");
        std::fs::create_dir_all(&atm_home).expect("atm home");
        let log_dir = atm_core::home::host_log_dir_from_home(&atm_home);
        let observability = Arc::new(
            crate::test_observability::TestDaemonObservability::new(log_dir)
                .expect("test observability"),
        );
        let status_cache = RuntimeStatusCache::new();
        let sqlite_observability =
            DaemonSqliteObservability::new(observability.clone(), status_cache.clone());

        sqlite_observability
            .emit(SqliteObservabilityEvent::new(
                "writer_submit",
                SqliteObservabilityOutcome::Timeout,
                "sqlite writer submission queue did not accept a write within 10s",
                Some(AtmErrorCode::DaemonUnavailable),
            ))
            .expect("emit sqlite event");

        observability
            .wait_for_message_contains(
                "\"subsystem\":\"sqlite\"",
                std::time::Duration::from_secs(1),
            )
            .expect("retained log sqlite message");
        let snapshot = status_cache.snapshot().expect("snapshot");
        assert!(!snapshot.sqlite_ready);
        assert!(
            snapshot
                .detail
                .as_ref()
                .expect("sqlite detail")
                .contains("[writer_submit] sqlite writer submission queue did not accept a write")
        );

        let finding = crate::runtime_status_cache::runtime_status_finding(&snapshot);
        assert_eq!(finding.severity, DoctorSeverity::Warning);
    }
}
