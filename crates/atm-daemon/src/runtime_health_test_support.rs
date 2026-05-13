use std::path::PathBuf;
use std::sync::Arc;

use super::DaemonRequestDispatcher;
use crate::runtime_status_cache::{RuntimeStatusCache, build_runtime_status_cache_state};

impl DaemonRequestDispatcher {
    pub(crate) fn new_for_test(
        home_dir: PathBuf,
        status_cache: RuntimeStatusCache,
        roster_db_path: PathBuf,
    ) -> Self {
        let sqlite_boundary = match atm_rusqlite::assemble_boundary(&roster_db_path) {
            Ok(boundary) => {
                if let Err(error) =
                    build_runtime_status_cache_state(None, &home_dir, boundary.roster_store())
                        .and_then(|state| status_cache.replace_state(state))
                {
                    tracing::warn!(
                        %error,
                        "failed to hydrate test runtime status cache from sqlite roster state"
                    );
                    status_cache.mark_sqlite_unavailable();
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
        let observability = Arc::new(
            crate::test_observability::TestDaemonObservability::new(
                atm_core::home::host_log_dir_from_home(&home_dir),
            )
            .expect("daemon test observability"),
        );
        let advisory_runtime_observability = crate::SubsystemObservability::new(
            crate::DaemonSubsystem::AdvisoryRuntime,
            Arc::clone(&observability) as Arc<dyn crate::DaemonRuntimeObservability>,
        );
        let runtime_health_observability = crate::SubsystemObservability::new(
            crate::DaemonSubsystem::RuntimeHealth,
            Arc::clone(&observability) as Arc<dyn crate::DaemonRuntimeObservability>,
        );
        Self {
            home_dir: home_dir.clone(),
            observability,
            advisory_runtime_observability: advisory_runtime_observability.clone(),
            runtime_health_observability,
            status_cache,
            sqlite_boundary,
            advisory_runtime: crate::advisory_runtime::AdvisoryRuntime::new(),
        }
    }
}
