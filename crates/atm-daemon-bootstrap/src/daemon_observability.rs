use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::error::AtmError;
use atm_core::home;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, CommandEvent, LogTailSession,
    ObservabilityPort,
};
use atm_observability::{
    RetainedCommandEvent, RetainedLogOffer, RetainedLogPolicy, RetainedLogger,
    build_retained_logger_from_env,
};

const ATM_SERVICE_NAME: &str = "atm";
const ATM_DAEMON_TARGET: &str = "atm.daemon";
const RETAINED_LOG_ROTATION_MAX_BYTES: u64 = 10 * 1024 * 1024;
const RETAINED_LOG_ROTATION_MAX_FILES: usize = 5;
const RETAINED_LOG_RETENTION_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const RETAINED_LOG_MAINTENANCE_CADENCE: Duration = Duration::from_secs(60);
// Allow one bounded maintenance join during shutdown without turning routine
// daemon stop into a long blocking operation. This stays below the outer 2s
// graceful drain budget so retained-log shutdown cannot consume the entire
// daemon stop window by itself.
const RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

struct LoggerLifecycle(Arc<RetainedLogger>);

impl LoggerLifecycle {
    fn health(&self, active_log_path: PathBuf) -> Result<AtmObservabilityHealth, AtmError> {
        self.0.health_at(active_log_path)
    }
}

pub(crate) struct DaemonObservability {
    // Keep one shared logger lifecycle behind a mutex so emit/health paths and
    // shutdown can coordinate a single transition into the stopped state.
    logger: Arc<Mutex<LoggerLifecycle>>,
    active_log_path: PathBuf,
}

impl std::fmt::Debug for DaemonObservability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonObservability")
            .field("active_log_path", &self.active_log_path)
            .finish_non_exhaustive()
    }
}

impl Clone for DaemonObservability {
    fn clone(&self) -> Self {
        Self {
            logger: Arc::clone(&self.logger),
            active_log_path: self.active_log_path.clone(),
        }
    }
}

impl DaemonObservability {
    pub(crate) fn bootstrap() -> Result<Self, AtmError> {
        Self::bootstrap_at_log_dir(home::host_log_dir()?)
    }

    pub(crate) fn install_tracing_bridge(&self) -> Result<(), AtmError> {
        // The replacement daemon deliberately owns this process-global
        // subscriber. A pre-installed subscriber is a bootstrap configuration
        // error, so fail closed instead of silently dropping retained events
        // into an unknown logging pipeline.
        let logger = self.logger.lock().map_err(|_| {
            AtmError::observability_bootstrap(
                "failed to install tracing bridge because the logger lock was poisoned",
            )
        })?;
        atm_observability::TracingBridgeLayer::install(Arc::clone(&logger.0))
            .map(|bridge| {
                crate::diagnostic_timeline::register_bridge(bridge);
            })
            .map_err(|error| match error {
                atm_observability::BridgeError::AlreadyInstalled => {
                    AtmError::observability_bootstrap(
                        "tracing bridge is already installed for this process",
                    )
                }
            })
    }

    fn bootstrap_at_log_dir(log_dir: PathBuf) -> Result<Self, AtmError> {
        Self::bootstrap_at_log_dir_with_rotation(log_dir, RETAINED_LOG_ROTATION_MAX_BYTES)
    }

    fn bootstrap_at_log_dir_with_rotation(
        log_dir: PathBuf,
        rotation_max_bytes: u64,
    ) -> Result<Self, AtmError> {
        let (logger, active_log_path) =
            build_logger(&log_dir, retained_log_policy(rotation_max_bytes))?;
        Ok(Self {
            logger: Arc::new(Mutex::new(LoggerLifecycle(Arc::new(logger)))),
            active_log_path,
        })
    }

    #[cfg(test)]
    fn bootstrap_at_log_dir_with_policy_for_test(
        log_dir: PathBuf,
        retained_log_policy: RetainedLogPolicy,
    ) -> Result<Self, AtmError> {
        let (logger, active_log_path) = build_logger(&log_dir, retained_log_policy)?;
        Ok(Self {
            logger: Arc::new(Mutex::new(LoggerLifecycle(Arc::new(logger)))),
            active_log_path,
        })
    }
}

impl atm_core::boundary::sealed::Sealed for DaemonObservability {}

impl ObservabilityPort for DaemonObservability {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
        let logger = self.logger.lock().map_err(|_| {
            AtmError::observability_emit(
                "shared daemon observability emit failed because the logger lock was poisoned",
            )
        })?;
        match logger.0.try_log_command(RetainedCommandEvent {
            target: ATM_DAEMON_TARGET,
            action: event.action.as_str(),
            outcome: event.outcome.as_str(),
            code: event.error_code.map(|code| code.as_str()),
        })? {
            RetainedLogOffer::Accepted | RetainedLogOffer::QueueFull => Ok(()),
            RetainedLogOffer::Rejected { diagnostic_code } => Err(AtmError::observability_emit(
                format!("shared daemon observability log admission failed ({diagnostic_code})"),
            )),
        }
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Err(AtmError::observability_query(
            "daemon retained-log query is unavailable from the exact-path daemon logger adapter",
        ))
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Err(AtmError::observability_follow(
            "daemon retained-log follow is unavailable from the exact-path daemon logger adapter",
        ))
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        let logger = self.logger.lock().map_err(|_| {
            AtmError::observability_health(
                "failed to read daemon observability health because the logger lock was poisoned",
            )
        })?;
        logger.health(self.active_log_path.clone())
    }
}

fn build_logger(
    log_dir: &Path,
    retained_log_policy: RetainedLogPolicy,
) -> Result<(RetainedLogger, PathBuf), AtmError> {
    let active_log_path = log_dir.join(atm_observability::CANONICAL_LOG_FILE_NAME);
    let logger = build_retained_logger_from_env(ATM_SERVICE_NAME, log_dir, retained_log_policy)?;
    Ok((logger, active_log_path))
}

fn retained_log_policy(rotation_max_bytes: u64) -> RetainedLogPolicy {
    RetainedLogPolicy {
        rotation_max_bytes,
        rotation_max_files: RETAINED_LOG_ROTATION_MAX_FILES,
        retention_max_age: RETAINED_LOG_RETENTION_MAX_AGE,
        maintenance_cadence: RETAINED_LOG_MAINTENANCE_CADENCE,
        writer_shutdown_timeout: RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT,
        maintenance_max_work_per_pass: Some(RETAINED_LOG_ROTATION_MAX_FILES),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use atm_core::observability::ObservabilityPort;
    use atm_core::test_support::EnvGuard;
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{DaemonObservability, RetainedLogPolicy};

    #[test]
    #[serial]
    fn bootstrap_fails_closed_when_atm_log_dir_is_invalid() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_many([
            ("ATM_LOG_DIR", Some("relative/logs")),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
            ("USERPROFILE", None),
            ("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", None),
        ]);

        let error = DaemonObservability::bootstrap().expect_err("invalid ATM_LOG_DIR");
        assert!(error.is_config());
        assert!(error.message().contains("absolute path"));
    }

    #[test]
    #[serial]
    fn bootstrap_fails_closed_when_retained_log_dir_cannot_be_created() {
        let tempdir = TempDir::new().expect("tempdir");
        let blocked_parent = tempdir.path().join("blocked-parent");
        std::fs::write(&blocked_parent, "not a directory").expect("blocked parent");
        let blocked_log_dir = blocked_parent.join("logs");
        let expected = std::fs::create_dir_all(&blocked_log_dir)
            .expect_err("child of a regular file must not be a directory")
            .to_string();
        let _env = EnvGuard::set_many([
            (
                "ATM_LOG_DIR",
                Some(blocked_log_dir.to_str().expect("utf8 blocked log dir")),
            ),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
            ("USERPROFILE", None),
            ("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", None),
        ]);

        let error =
            DaemonObservability::bootstrap().expect_err("retained log dir create should fail");
        assert!(error.is_observability_bootstrap());
        assert!(
            error
                .message()
                .contains(&blocked_log_dir.display().to_string())
        );
        assert!(error.message().contains(&expected), "{error}");
    }

    #[test]
    #[serial]
    fn bootstrap_fails_closed_when_retained_log_file_is_not_appendable() {
        let tempdir = TempDir::new().expect("tempdir");
        let blocked_log_dir = tempdir.path().join("logs");
        std::fs::create_dir_all(&blocked_log_dir).expect("blocked log dir");
        std::fs::create_dir(blocked_log_dir.join("atm.log.jsonl"))
            .expect("non-appendable log path");
        let active_log_path = blocked_log_dir.join("atm.log.jsonl");
        let expected = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active_log_path)
            .expect_err("directory must not be appendable")
            .to_string();
        let _env = EnvGuard::set_many([
            (
                "ATM_LOG_DIR",
                Some(blocked_log_dir.to_str().expect("utf8 blocked log dir")),
            ),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
            ("USERPROFILE", None),
            ("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", None),
        ]);

        let error =
            DaemonObservability::bootstrap().expect_err("retained log file open should fail");
        assert!(error.is_observability_bootstrap());
        assert!(error.message().contains("atm.log.jsonl"));
        assert!(
            error
                .message()
                .contains(&active_log_path.display().to_string())
        );
        assert!(error.message().contains(&expected), "{error}");
    }

    #[test]
    fn retained_log_prune_runs_on_a_background_worker() {
        let tempdir = TempDir::new().expect("tempdir");
        let log_dir = tempdir.path().join("logs");
        std::fs::create_dir_all(&log_dir).expect("log dir");
        let active_log_path = log_dir.join("atm.log.jsonl");
        std::fs::write(&active_log_path, "active\n").expect("active log");
        let rotated_log_path = log_dir.join("atm.log.jsonl.1");
        std::fs::write(&rotated_log_path, "stale\n").expect("rotated log");

        let policy = RetainedLogPolicy {
            rotation_max_bytes: 1024,
            rotation_max_files: 5,
            retention_max_age: Duration::from_millis(1),
            maintenance_cadence: Duration::from_millis(10),
            writer_shutdown_timeout: Duration::from_secs(1),
            maintenance_max_work_per_pass: Some(5),
        };
        let observability =
            super::DaemonObservability::bootstrap_at_log_dir_with_policy_for_test(log_dir, policy)
                .expect("bootstrap");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut health = observability.health().expect("initial health");
        while Instant::now() < deadline
            && (rotated_log_path.exists()
                || health
                    .maintenance
                    .as_ref()
                    .is_none_or(|report| report.pruned_files_total < 1))
        {
            health = observability.health().expect("health during prune wait");
            std::thread::yield_now();
        }

        assert!(
            !rotated_log_path.exists(),
            "background prune worker should remove expired rotated files"
        );

        assert!(
            health
                .maintenance
                .as_ref()
                .is_some_and(|report| report.pruned_files_total >= 1),
            "maintenance stats should record the background prune pass"
        );
    }
}
