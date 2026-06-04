use std::path::PathBuf;
use std::sync::Arc;

use super::DaemonRequestDispatcher;
use crate::notification_runtime::NotificationRuntime;
use crate::runtime_status_cache::{RuntimeStatusCache, build_runtime_status_cache_state};
use crate::sqlite_observability::DaemonSqliteObservability;
use atm_core::{LocalFileNonClaudeOutbound, LocalFileNotificationSink};
use atm_runtime::{RuntimeAssemblyInputs, assemble_sqlite_runtime};

impl DaemonRequestDispatcher {
    pub(crate) fn new_for_test(
        home_dir: PathBuf,
        status_cache: RuntimeStatusCache,
        roster_db_path: PathBuf,
    ) -> Self {
        let observability = Arc::new(
            crate::test_observability::TestDaemonObservability::new(
                atm_core::home::host_log_dir_from_home(&home_dir),
            )
            .expect("daemon test observability"),
        );
        let runtime_observability: Arc<dyn crate::DaemonRuntimeObservability> =
            observability.clone();
        let sqlite_observability: Arc<dyn atm_rusqlite::SqliteObservability> =
            Arc::new(DaemonSqliteObservability::new(
                Arc::clone(&runtime_observability),
                status_cache.clone(),
            ));
        let sqlite_boundary = match atm_rusqlite::assemble_boundary_with_observability(
            &roster_db_path,
            Arc::clone(&sqlite_observability),
        ) {
            Ok(boundary) => {
                match build_runtime_status_cache_state(None, boundary.roster_store()) {
                    Ok(state) => status_cache.publish_state(state),
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            "failed to hydrate test runtime status cache from sqlite roster state"
                        );
                        status_cache.mark_sqlite_unavailable();
                    }
                }
                Some(boundary)
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    path = %roster_db_path.display(),
                    "failed to assemble sqlite boundary for test daemon runtime health"
                );
                status_cache.mark_sqlite_unavailable();
                None
            }
        };
        let runtime_assembly = sqlite_boundary.as_ref().and_then(|_| {
            assemble_sqlite_runtime(RuntimeAssemblyInputs {
                sqlite_db_path: roster_db_path.clone(),
                sqlite_observability: Arc::clone(&sqlite_observability),
                non_claude_outbound: Arc::new(LocalFileNonClaudeOutbound::new()),
                notification_sink: Arc::new(LocalFileNotificationSink::at_path(
                    home_dir.join("notifications.jsonl"),
                )),
            })
            .ok()
        });
        let advisory_runtime_observability = crate::SubsystemObservability::new(
            crate::DaemonSubsystem::AdvisoryRuntime,
            Arc::clone(&runtime_observability),
        );
        let runtime_health_observability = crate::SubsystemObservability::new(
            crate::DaemonSubsystem::RuntimeHealth,
            Arc::clone(&runtime_observability),
        );
        let notification_runtime = NotificationRuntime::new_with_observability(
            crate::SubsystemObservability::disabled(crate::DaemonSubsystem::NotificationRuntime),
        );
        notification_runtime.set_liveness_override_for_test(Some(
            crate::notification_runtime::NotificationWorkerLiveness::Live,
        ));
        Self {
            home_dir: crate::AtmHomeDir::from_path_for_test(home_dir.clone()),
            observability: runtime_observability,
            advisory_runtime_observability: advisory_runtime_observability.clone(),
            runtime_health_observability,
            status_cache,
            roster_store: runtime_assembly
                .as_ref()
                .map(|assembly| assembly.runtime_bundle.roster_store.clone()),
            storage_finalizer: runtime_assembly
                .as_ref()
                .map(|assembly| assembly.storage_finalizer.clone()),
            notification_runtime,
            advisory_runtime: crate::advisory_runtime::AdvisoryRuntime::new_with_observability(
                advisory_runtime_observability,
            ),
        }
    }
}
