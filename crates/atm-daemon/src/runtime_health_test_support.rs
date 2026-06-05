use std::path::PathBuf;
use std::sync::Arc;

use super::DaemonRequestDispatcher;
use crate::notification_runtime::NotificationRuntime;
use crate::runtime_sqlite_observer::DaemonRuntimeSqliteObserver;
use crate::runtime_status_cache::{RuntimeStatusCache, build_runtime_status_cache_state};
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
        let sqlite_observer: Arc<dyn atm_runtime::RuntimeSqliteObserver> =
            Arc::new(DaemonRuntimeSqliteObserver::new(
                Arc::clone(&runtime_observability),
                status_cache.clone(),
            ));
        let runtime_assembly = assemble_sqlite_runtime(RuntimeAssemblyInputs {
            sqlite_db_path: roster_db_path.clone(),
            config_current_dir: home_dir.clone(),
            sqlite_observer: Arc::clone(&sqlite_observer),
            non_claude_outbound: Arc::new(LocalFileNonClaudeOutbound::new()),
            notification_sink: Arc::new(LocalFileNotificationSink::at_path(
                home_dir.join("notifications.jsonl"),
            )),
        })
        .unwrap_or_else(|error| {
            panic!(
                "failed to assemble sqlite runtime for test daemon runtime health at {}: {error}",
                roster_db_path.display()
            )
        });
        match build_runtime_status_cache_state(
            None,
            runtime_assembly.runtime_bundle.roster_store.as_ref(),
        ) {
            Ok(state) => status_cache.publish_state(state),
            Err(error) => {
                tracing::warn!(
                    %error,
                    "failed to hydrate test runtime status cache from runtime-bound roster state"
                );
            }
        }
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
            service_runtime: runtime_assembly.service_runtime.clone(),
            runtime_bundle: runtime_assembly.runtime_bundle.clone(),
            roster_store: Some(runtime_assembly.runtime_bundle.roster_store.clone()),
            storage_finalizer: Some(runtime_assembly.storage_finalizer.clone()),
            notification_runtime,
            advisory_runtime: crate::advisory_runtime::AdvisoryRuntime::new_with_observability(
                advisory_runtime_observability,
            ),
        }
    }
}
