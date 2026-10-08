use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use atm_core::atm_temp::ProcessEnvSource;
use atm_core::error::AtmError;
use atm_core::home;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmTelemetryExportFailure,
    AtmTelemetryExportHealth, AtmTelemetryExportState, CommandEvent, LogTailSession,
    ObservabilityPort,
};
use atm_core::{LogDestination, TelemetryExportConfig, TelemetryExportProtocol};
use atm_observability::{
    ExportDiagnostics, RetainedCommandEvent, RetainedLogOffer, RetainedLogPolicy, RetainedLogger,
    build_routed_retained_logger, logger_level_override,
};
use atm_runtime::{
    TaskTelemetryDiagnostics, TaskTelemetrySetup, WorkflowTelemetryDiagnostics,
    WorkflowTelemetrySetup,
};
use opentelemetry::logs::LoggerProvider;
use opentelemetry_sdk::error::OTelSdkError;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tokio::sync::watch;
use tokio::time::Instant;

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
/// Upper bound on the exporter step of daemon shutdown. The bd-5 production
/// limits need at most two 400ms exports per quiescent signal.
const EXPORT_SHUTDOWN_BOUND: Duration = Duration::from_secs(1);

struct LoggerLifecycle(Arc<RetainedLogger>);

impl LoggerLifecycle {
    fn health(&self, active_log_path: PathBuf) -> Result<AtmObservabilityHealth, AtmError> {
        self.0.health_at(active_log_path)
    }
}

/// Resolved export selection. Only validated endpoints are retained.
enum ExportSelection {
    Absent,
    Invalid,
    Configured { endpoint: String },
}

type Providers = (SdkTracerProvider, SdkLoggerProvider, SdkMeterProvider);

/// Export state owned by the process lifecycle: the standard SDK providers
/// (retained only for shutdown), the setups handed once to runtime assembly,
/// and the runtime counters attached after assembly.
struct Export {
    selection: ExportSelection,
    diagnostics: Arc<ExportDiagnostics>,
    // MUTEX: runtime assembly takes the setups once; shutdown takes providers once.
    // Each guards a single Option that is only ever read or `take`n, so a
    // panic while it is held cannot leave it half-updated: poison is recovered
    // with `into_inner` so telemetry setup and shutdown still run.
    setups: Mutex<Option<(TaskTelemetrySetup, WorkflowTelemetrySetup)>>,
    providers: Mutex<Option<Providers>>,
    runtime: OnceLock<(
        Arc<TaskTelemetryDiagnostics>,
        Arc<WorkflowTelemetryDiagnostics>,
    )>,
    shutdown: OnceLock<watch::Receiver<bool>>,
}

impl Export {
    fn inert() -> Self {
        Self::new(ExportSelection::Absent, Arc::default(), None)
    }

    fn new(
        selection: ExportSelection,
        diagnostics: Arc<ExportDiagnostics>,
        setup: Option<atm_observability::TelemetrySetup>,
    ) -> Self {
        let (setups, providers) = match setup {
            Some((task, workflow, tracer, logger, meter)) => {
                (Some((task, workflow)), Some((tracer, logger, meter)))
            }
            None => (None, None),
        };
        Self {
            selection,
            diagnostics,
            setups: Mutex::new(setups),
            providers: Mutex::new(providers),
            runtime: OnceLock::new(),
            shutdown: OnceLock::new(),
        }
    }

    fn health(&self) -> AtmTelemetryExportHealth {
        let mut health = AtmTelemetryExportHealth {
            state: AtmTelemetryExportState::Inert,
            endpoint: None,
            protocol: None,
            emitted: 0,
            dropped_full: 0,
            dropped_timeout: 0,
            dropped_failure: 0,
            dropped_shutdown: 0,
            last_failure: None,
        };
        match &self.selection {
            ExportSelection::Absent => return health,
            ExportSelection::Invalid => {
                health.state = AtmTelemetryExportState::Unavailable;
                health.last_failure = Some(AtmTelemetryExportFailure::ConfigInvalid);
                return health;
            }
            ExportSelection::Configured { endpoint } => {
                health.state = AtmTelemetryExportState::Healthy;
                health.endpoint = Some(endpoint.clone());
                health.protocol = Some(TelemetryExportProtocol::Grpc);
            }
        }
        // Task and workflow admission queues are separate, so their known
        // runtime losses are disjoint and add without double counting.
        if let Some((task, workflow)) = self.runtime.get() {
            use std::sync::atomic::Ordering::Relaxed;
            let task = task.snapshot();
            health.emitted = task.emitted;
            health.dropped_full = task.dropped_full + workflow.dropped_full.load(Relaxed);
            health.dropped_timeout = task.dropped_timeout + workflow.dropped_timeout.load(Relaxed);
            health.dropped_failure = task.dropped_failure + workflow.dropped_failure.load(Relaxed);
            health.dropped_shutdown =
                task.dropped_shutdown + workflow.dropped_shutdown.load(Relaxed);
            if health.dropped_full + health.dropped_timeout + health.dropped_failure > 0 {
                health.state = AtmTelemetryExportState::Degraded;
            }
        }
        // Observable SDK transport/lifecycle failures; never SDK loss counts.
        self.diagnostics.project(&mut health);
        health
    }
}

/// The daemon process's observability owner: the retained logger and the
/// standard SDK providers. The entrypoint bootstraps it once and passes it
/// into daemon composition, which drains and shuts it down.
pub struct DaemonObservability {
    // Keep one shared logger lifecycle behind a mutex so emit/health paths and
    // shutdown can coordinate a single transition into the stopped state.
    logger: Arc<Mutex<LoggerLifecycle>>,
    active_log_path: PathBuf,
    export: Arc<Export>,
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
            export: Arc::clone(&self.export),
        }
    }
}

impl DaemonObservability {
    /// Resolves export configuration, constructs the standard SDK providers on
    /// the current Tokio runtime, then builds the routed retained logger on the
    /// blocking pool. Invalid export configuration never fails the daemon: it
    /// selects file logging and reports `Unavailable`/`ConfigInvalid` health.
    pub(crate) async fn bootstrap() -> Result<Self, AtmError> {
        Self::bootstrap_from(&ProcessEnvSource, home::host_log_dir()?).await
    }

    /// [`Self::bootstrap`] over an explicit environment and log directory.
    pub(crate) async fn bootstrap_from(
        env: &dyn atm_core::atm_temp::EnvSource,
        log_dir: PathBuf,
    ) -> Result<Self, AtmError> {
        let export = resolve_export(env);
        let destination = export.destination;
        let otel_logger = export
            .export
            .providers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|p| p.1.logger(ATM_SERVICE_NAME));
        let (logger, active_log_path) = tokio::task::spawn_blocking(move || {
            let level = logger_level_override()?;
            let active_log_path = log_dir.join(atm_observability::CANONICAL_LOG_FILE_NAME);
            let logger = build_routed_retained_logger(
                ATM_SERVICE_NAME,
                &log_dir,
                retained_log_policy(RETAINED_LOG_ROTATION_MAX_BYTES),
                level,
                destination,
                otel_logger,
            )?;
            Ok::<_, AtmError>((logger, active_log_path))
        })
        .await
        .map_err(|source| {
            AtmError::observability_bootstrap(format!(
                "daemon observability bootstrap worker did not complete: {source}"
            ))
        })??;
        Ok(Self {
            logger: Arc::new(Mutex::new(LoggerLifecycle(Arc::new(logger)))),
            active_log_path,
            export: Arc::new(export.export),
        })
    }

    /// Hands the existing task/workflow setups to runtime assembly once.
    pub(crate) fn take_telemetry_setups(
        &self,
    ) -> (Option<TaskTelemetrySetup>, Option<WorkflowTelemetrySetup>) {
        self.export
            .setups
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .map_or((None, None), |(task, workflow)| {
                (Some(task), Some(workflow))
            })
    }

    /// Attaches the assembled runtimes' known-loss counters to doctor health.
    pub(crate) fn attach_runtime_telemetry(
        &self,
        task: Arc<TaskTelemetryDiagnostics>,
        workflow: Arc<WorkflowTelemetryDiagnostics>,
    ) {
        self.export.runtime.get_or_init(|| (task, workflow));
    }

    /// Shuts the three standard providers down concurrently on the blocking
    /// pool, bounded by `min(1s, deadline)`. The first call owns the work in a
    /// spawned task, so a cancelled caller cannot lose the outcome; every
    /// caller awaits the same completion until its own deadline. A timeout
    /// abandons the wait, not the blocking call: runtime teardown drops the
    /// SDK tasks and releases those calls.
    pub(crate) async fn shutdown_export(&self, deadline: Instant) {
        let export = Arc::clone(&self.export);
        let mut done = self
            .export
            .shutdown
            .get_or_init(|| {
                let (sender, receiver) = watch::channel(false);
                let bound = deadline.min(Instant::now() + EXPORT_SHUTDOWN_BOUND);
                tokio::spawn(async move {
                    let providers = export
                        .providers
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take();
                    if let Some(providers) = providers {
                        shutdown_providers(providers, bound, &export.diagnostics).await;
                    }
                    sender.send_replace(true);
                });
                receiver
            })
            .clone();
        // Past its own deadline a caller stops waiting; the owner keeps the outcome.
        drop(tokio::time::timeout_at(deadline, done.wait_for(|done| *done)).await);
    }

    /// Flushes every retained event admitted before the call, file and routed
    /// OpenTelemetry sinks, through the existing canonical `flush` on the
    /// blocking pool. The wait is bounded by `min(1s, deadline)` and consumes
    /// the caller's cumulative deadline; past it nothing is attempted. The
    /// canonical flush keeps its own configured 1s bound, so a timeout
    /// abandons the wait, not the call. The logger stays live: this does not
    /// stop the writer thread, which process exit ends.
    pub(crate) async fn flush_logger(&self, deadline: Instant) {
        let bound = deadline.min(Instant::now() + RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT);
        if bound <= Instant::now() {
            return;
        }
        // The guarded value is an immutable `Arc`, so a poisoned lock still
        // holds a valid logger to flush.
        let logger = Arc::clone(&self.logger.lock().unwrap_or_else(PoisonError::into_inner).0);
        let flush = tokio::task::spawn_blocking(move || logger.flush());
        // Sink flush failures are recorded in logger health by the canonical
        // logger; an abandoned wait leaves nothing further to report.
        drop(tokio::time::timeout_at(bound, flush).await);
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
                bridge.set_export_diagnostics(Arc::clone(&self.export.diagnostics));
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

    /// Reports an invalid export configuration once the bridge exists. Only
    /// the stable code is logged: rejected values may carry credentials.
    pub(crate) fn report_export_config(&self) {
        if matches!(self.export.selection, ExportSelection::Invalid)
            || self.export.health().last_failure == Some(AtmTelemetryExportFailure::ConfigInvalid)
        {
            tracing::warn!(
                target: "atm_daemon_bootstrap::observability",
                code = "ATM_TELEMETRY_EXPORT_CONFIG_INVALID",
                "OpenTelemetry export disabled: invalid ATM_OTEL_* or ATM_LOG_DESTINATION configuration; the daemon continues with file logging"
            );
        }
    }

    #[cfg(test)]
    fn bootstrap_at_log_dir_with_policy_for_test(
        log_dir: PathBuf,
        retained_log_policy: RetainedLogPolicy,
    ) -> Result<Self, AtmError> {
        let (logger, active_log_path) = build_file_logger(&log_dir, retained_log_policy)?;
        Ok(Self {
            logger: Arc::new(Mutex::new(LoggerLifecycle(Arc::new(logger)))),
            active_log_path,
            export: Arc::new(Export::inert()),
        })
    }

    #[cfg(test)]
    pub(crate) fn export_providers_present_for_test(&self) -> bool {
        self.export
            .providers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    #[cfg(test)]
    pub(crate) fn export_health_for_test(&self) -> AtmTelemetryExportHealth {
        self.export.health()
    }

    /// The same bridge `install_tracing_bridge` installs process-wide, for a
    /// test-scoped dispatcher.
    #[cfg(test)]
    pub(crate) fn tracing_bridge_for_test(&self) -> atm_observability::TracingBridgeLayer {
        let logger = self.logger.lock().expect("logger lock");
        let bridge = atm_observability::TracingBridgeLayer::new(Arc::clone(&logger.0));
        bridge.set_export_diagnostics(Arc::clone(&self.export.diagnostics));
        bridge
    }

    #[cfg(test)]
    fn shutdown_for_test(self) {
        let logger = match Arc::try_unwrap(self.logger) {
            Ok(logger) => logger,
            Err(_) => panic!("test observability logger must have one owner"),
        };
        let lifecycle = match logger.into_inner() {
            Ok(lifecycle) => lifecycle,
            Err(_) => panic!("test observability logger lock must not be poisoned"),
        };
        let retained_logger = match Arc::try_unwrap(lifecycle.0) {
            Ok(retained_logger) => retained_logger,
            Err(_) => panic!("test retained logger must have one owner"),
        };
        drop(retained_logger.shutdown());
    }

    #[cfg(test)]
    fn flush_for_test(&self) {
        let logger = self.logger.lock().expect("test observability logger lock");
        logger.0.flush().expect("test observability logger flush");
    }
}

async fn shutdown_providers(
    (tracer, logger, meter): Providers,
    bound: Instant,
    diagnostics: &ExportDiagnostics,
) {
    let all = async {
        tokio::join!(
            tokio::task::spawn_blocking(move || tracer.shutdown()),
            tokio::task::spawn_blocking(move || logger.shutdown()),
            tokio::task::spawn_blocking(move || meter.shutdown()),
        )
    };
    match tokio::time::timeout_at(bound, all).await {
        Ok((trace, log, metric)) => {
            for result in [trace, log, metric] {
                diagnostics.observe_result(result.unwrap_or_else(|_| {
                    Err(OTelSdkError::InternalFailure(
                        "provider shutdown worker panicked".to_owned(),
                    ))
                }));
            }
        }
        Err(_) => diagnostics.shutdown_wait_timed_out(),
    }
}

struct ResolvedExport {
    export: Export,
    destination: LogDestination,
}

/// Absent endpoint: no exporter, worker or provider and `Inert` health.
/// Invalid configuration (including `http/json`) or failed SDK setup keeps the
/// daemon operational with file logging and `ConfigInvalid` diagnostics.
fn resolve_export(env: &dyn atm_core::atm_temp::EnvSource) -> ResolvedExport {
    let diagnostics = Arc::new(ExportDiagnostics::default());
    let invalid = || ResolvedExport {
        export: Export::new(ExportSelection::Invalid, Arc::default(), None),
        destination: LogDestination::File,
    };
    let Ok(destination) = LogDestination::from_env(env) else {
        return invalid();
    };
    let config = match TelemetryExportConfig::from_env(env) {
        Ok(Some(config)) => config,
        Ok(None) => {
            return ResolvedExport {
                export: Export::inert(),
                destination,
            };
        }
        Err(_) => return invalid(),
    };
    let selection = ExportSelection::Configured {
        endpoint: config.endpoint().to_owned(),
    };
    match atm_observability::setup_telemetry(&config, &diagnostics) {
        Ok(setup) => ResolvedExport {
            export: Export::new(selection, diagnostics, Some(setup)),
            destination,
        },
        Err(_) => ResolvedExport {
            export: Export::new(selection, diagnostics, None),
            destination: LogDestination::File,
        },
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
        let mut health = logger.health(self.active_log_path.clone())?;
        health.export = Some(self.export.health());
        Ok(health)
    }
}

#[cfg(test)]
fn build_file_logger(
    log_dir: &std::path::Path,
    retained_log_policy: RetainedLogPolicy,
) -> Result<(RetainedLogger, PathBuf), AtmError> {
    let active_log_path = log_dir.join(atm_observability::CANONICAL_LOG_FILE_NAME);
    let logger = atm_observability::build_retained_logger(
        ATM_SERVICE_NAME,
        log_dir,
        retained_log_policy,
        None,
    )?;
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
    use std::path::Path;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, SystemTime};

    use atm_core::test_support::EnvGuard;
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{DaemonObservability, RetainedLogPolicy};

    /// Makes a rotated log unambiguously older than the configured retention
    /// age without relying on scheduler delay or filesystem timestamp
    /// resolution.
    fn age_rotated_log(path: &Path, retention_max_age: Duration) {
        let expired_at = SystemTime::now()
            .checked_sub(retention_max_age + Duration::from_secs(1))
            .expect("test retention age must not underflow SystemTime");
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open rotated log for mtime rewrite")
            .set_times(std::fs::FileTimes::new().set_modified(expired_at))
            .expect("backdate rotated log");
    }

    #[tokio::test]
    #[serial]
    async fn bootstrap_fails_closed_when_atm_log_dir_is_invalid() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_many([
            ("ATM_LOG_DIR", Some("relative/logs")),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
            ("USERPROFILE", None),
            ("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", None),
        ]);

        let error = DaemonObservability::bootstrap()
            .await
            .expect_err("invalid ATM_LOG_DIR");
        assert!(error.is_config());
        assert!(error.message().contains("absolute path"));
    }

    #[tokio::test]
    #[serial]
    async fn bootstrap_fails_closed_when_retained_log_dir_cannot_be_created() {
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

        let error = DaemonObservability::bootstrap()
            .await
            .expect_err("retained log dir create should fail");
        assert!(error.is_observability_bootstrap());
        assert!(
            error
                .message()
                .contains(&blocked_log_dir.display().to_string())
        );
        assert!(error.message().contains(&expected), "{error}");
    }

    #[tokio::test]
    #[serial]
    async fn bootstrap_fails_closed_when_retained_log_file_is_not_appendable() {
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

        let error = DaemonObservability::bootstrap()
            .await
            .expect_err("retained log file open should fail");
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
        let retention_max_age = Duration::from_millis(1);
        age_rotated_log(&rotated_log_path, retention_max_age);

        let policy = RetainedLogPolicy {
            rotation_max_bytes: 1024,
            rotation_max_files: 5,
            retention_max_age,
            maintenance_cadence: Duration::from_nanos(1),
            writer_shutdown_timeout: Duration::from_secs(1),
            maintenance_max_work_per_pass: Some(5),
        };
        let observability =
            super::DaemonObservability::bootstrap_at_log_dir_with_policy_for_test(log_dir, policy)
                .expect("bootstrap");
        observability.flush_for_test();

        let completion = Arc::new((Mutex::new(None::<()>), Condvar::new()));
        let completion_worker = Arc::clone(&completion);
        let shutdown_worker = std::thread::spawn(move || {
            observability.shutdown_for_test();
            let (state, changed) = &*completion_worker;
            *state.lock().expect("completion state") = Some(());
            changed.notify_one();
        });

        let (state, changed) = &*completion;
        let state = state.lock().expect("completion state");
        let (mut state, timeout) = changed
            .wait_timeout_while(state, Duration::from_secs(2), |health| health.is_none())
            .expect("completion wait");
        assert!(state.is_some(), "background writer shutdown must complete");
        assert!(
            !timeout.timed_out(),
            "background writer shutdown exceeded the hard test bound"
        );
        state.take().expect("completion completion");
        shutdown_worker.join().expect("shutdown worker");

        assert!(
            !rotated_log_path.exists(),
            "background prune worker should remove expired rotated files"
        );
    }

    /// Positive: through the daemon's own bootstrap, each of
    /// `ATM_LOG_DESTINATION=file|otel|both` delivers one record per selected
    /// destination for a CLI-startup port record, a direct `sc` macro record,
    /// and a tracing-origin record, and
    /// the standard logger provider still exports after the retained logger
    /// is shut down.
    /// Negative: a below-threshold record and a secret field value reach
    /// neither destination, and SDK diagnostics never recurse into export.
    /// Query/follow and CLI-error behavior remain covered by
    /// `concrete_adapter_emits_queries_follows_and_reports_health` and
    /// `run_snapshot_surfaces_observability_query_error`; rotation retention
    /// remains covered by `retained_log_prune_runs_on_a_background_worker`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn daemon_log_destinations_route_once_and_keep_the_provider() {
        use crate::telemetry_lifecycle_tests::receiver::Receiver;
        use atm_core::observability::{
            CommandEvent, ObservabilityPort, action_name, outcome_label,
        };
        use opentelemetry::logs::{LogRecord as _, Logger as _, LoggerProvider as _};
        use tracing_subscriber::layer::SubscriberExt as _;

        fn count(records: &[String], needle: &str) -> usize {
            records
                .iter()
                .filter(|record| record.contains(needle))
                .count()
        }

        for destination in ["file", "otel", "both"] {
            let receiver = Receiver::start(false).await;
            let root = TempDir::new().expect("tempdir");
            let log_dir = root.path().join("logs");
            let env = atm_core::test_support::FakeEnvSource::new([
                ("ATM_OTEL_ENDPOINT", Some(receiver.endpoint.as_str())),
                ("ATM_LOG_DESTINATION", Some(destination)),
            ]);
            let observability = DaemonObservability::bootstrap_from(&env, log_dir.clone())
                .await
                .expect("bootstrap");
            let mut direct_sc_attachment = observability
                .logger
                .lock()
                .expect("logger lock")
                .0
                .attach_sc_log_facade()
                .expect("attach direct sc logger");
            let dispatch = tracing::Dispatch::new(
                tracing_subscriber::registry().with(observability.tracing_bridge_for_test()),
            );
            let subscriber = tracing::dispatcher::set_default(&dispatch);
            observability
                .emit(CommandEvent {
                    command: "atm",
                    action: action_name("cli_startup"),
                    outcome: outcome_label("ok"),
                    team: "bd6-team".parse().expect("team"),
                    agent: "sender".parse().expect("agent"),
                    sender: "sender".parse().expect("agent"),
                    message_id: None,
                    requires_ack: false,
                    dry_run: false,
                    task_id: None,
                    error_code: None,
                    error_message: None,
                })
                .expect("CLI startup record");
            sc_observability_log::event!(
                name: "bd6.direct_sc_macro",
                target: "atm_daemon_bootstrap::bd6_direct_sc_macro",
                sc_observability_log::Level::INFO,
                "bd6 direct sc macro record"
            );
            tracing::warn!(target: "atm_daemon_bootstrap::bd6_tracing_record", token = "raw-secret", "bd6 tracing record");
            tracing::debug!(target: "atm_daemon_bootstrap::bd6_below_threshold", "bd6 below threshold");
            observability.flush_for_test();
            let exported = |receiver: &Receiver| -> Vec<String> {
                receiver
                    .capture
                    .logs
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|record| format!("{record:?}"))
                    .collect()
            };
            if destination != "file" {
                receiver
                    .capture
                    .wait(Duration::from_secs(15), "both exported records", || {
                        let logs = exported(&receiver);
                        count(&logs, "cli_startup") == 1
                            && count(&logs, "bd6.direct_sc_macro") == 1
                            && count(&logs, "bd6_tracing_record") == 1
                    })
                    .await;
                let logs = exported(&receiver);
                assert_eq!(logs.len(), 3, "{destination}: no recursion: {logs:#?}");
                assert_eq!(count(&logs, "raw-secret"), 0);
                assert_eq!(count(&logs, "below_threshold"), 0);
            } else {
                // Negative control: file-only owns the retained JSONL sink,
                // so every collector record would be an unintended export.
                assert!(
                    exported(&receiver).is_empty(),
                    "file-only must not export OTLP logs: {:#?}",
                    exported(&receiver)
                );
            }
            let file = log_dir.join(atm_observability::CANONICAL_LOG_FILE_NAME);
            if destination == "otel" {
                assert!(!file.exists(), "otel-only writes no JSONL");
            } else {
                let lines: Vec<String> = std::fs::read_to_string(&file)
                    .expect("jsonl")
                    .lines()
                    .map(str::to_owned)
                    .collect();
                assert_eq!(count(&lines, "cli_startup"), 1, "{lines:#?}");
                assert_eq!(count(&lines, "bd6.direct_sc_macro"), 1, "{lines:#?}");
                assert_eq!(count(&lines, "bd6_tracing_record"), 1, "{lines:#?}");
                assert_eq!(count(&lines, "raw-secret"), 0);
                assert_eq!(count(&lines, "below_threshold"), 0);
            }
            drop(subscriber);
            drop(dispatch);
            direct_sc_attachment
                .detach(Duration::from_secs(1))
                .expect("detach direct sc logger");
            let export = Arc::clone(&observability.export);
            let caller = export
                .providers
                .lock()
                .unwrap()
                .as_ref()
                .expect("configured providers")
                .1
                .logger("bd6-caller");
            observability.shutdown_for_test();
            let mut record = caller.create_log_record();
            record.set_body("bd6 after retained logger shutdown".into());
            caller.emit(record);
            drop(caller);
            let providers = export.providers.lock().unwrap().take().expect("providers");
            super::shutdown_providers(
                providers,
                tokio::time::Instant::now() + Duration::from_secs(1),
                &export.diagnostics,
            )
            .await;
            assert_eq!(
                count(&exported(&receiver), "bd6 after retained logger shutdown"),
                1,
                "{destination}: the provider outlives the retained logger"
            );
            receiver.stop().await;
        }
    }

    /// Runs `hold`, which locks a mutex and panics while holding the guard,
    /// the way a panicking holder poisons it.
    fn poison(hold: impl FnOnce() + Send + 'static) {
        std::thread::spawn(hold)
            .join()
            .expect_err("the holder panicked");
    }

    /// A bootstrapped owner with a configured exporter (lazy connection, no
    /// collector needed) so setups and providers are present.
    async fn configured() -> (TempDir, DaemonObservability) {
        let root = TempDir::new().expect("tempdir");
        let env = atm_core::test_support::FakeEnvSource::new([(
            "ATM_OTEL_ENDPOINT",
            Some("http://127.0.0.1:4317"),
        )]);
        let observability = DaemonObservability::bootstrap_from(&env, root.path().join("logs"))
            .await
            .expect("bootstrap");
        (root, observability)
    }

    /// Positive: a poisoned setups lock still hands the setups to runtime
    /// assembly. Negative: it does not read as "no setups", which would
    /// silently disable telemetry.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn poisoned_setups_lock_still_hands_over_the_setups() {
        let (_root, observability) = configured().await;
        let export = Arc::clone(&observability.export);
        poison(move || {
            let _guard = export.setups.lock().expect("lock before poisoning");
            panic!("deliberate poison");
        });
        assert!(observability.export.setups.is_poisoned());
        let (task, workflow) = observability.take_telemetry_setups();
        assert!(task.is_some() && workflow.is_some());
        let (again, _) = observability.take_telemetry_setups();
        assert!(again.is_none(), "the setups are handed over once");
    }

    /// Positive: a poisoned providers lock still shuts the providers down.
    /// Negative: they are not left installed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn poisoned_providers_lock_still_shuts_the_providers_down() {
        let (_root, observability) = configured().await;
        assert!(observability.export_providers_present_for_test());
        let export = Arc::clone(&observability.export);
        poison(move || {
            let _guard = export.providers.lock().expect("lock before poisoning");
            panic!("deliberate poison");
        });
        assert!(observability.export.providers.is_poisoned());
        observability
            .shutdown_export(tokio::time::Instant::now() + Duration::from_secs(5))
            .await;
        assert!(!observability.export_providers_present_for_test());
    }

    /// Positive: a poisoned logger lock still drains the retained logger: every
    /// event admitted before the call is on disk when `flush_logger` returns.
    /// Negative control (scheduler-dependent, not deterministic): no writer
    /// barrier exists, so a burst this size is usually still queued behind the
    /// writer when the last `emit` returns and a skipped flush then leaves
    /// lines missing (0 of 30 mutant runs passed). A schedule where the writer
    /// drains first would let the mutant pass; the setups and providers
    /// regressions are the deterministic controls.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn poisoned_logger_lock_still_drains_the_retained_logger() {
        use atm_core::observability::{
            CommandEvent, ObservabilityPort, action_name, outcome_label,
        };
        const BURST: usize = 64;
        let (_root, observability) = configured().await;
        for _ in 0..BURST {
            observability
                .emit(CommandEvent {
                    command: "atm",
                    action: action_name("poison_drain"),
                    outcome: outcome_label("ok"),
                    team: "bd6-team".parse().expect("team"),
                    agent: "sender".parse().expect("agent"),
                    sender: "sender".parse().expect("agent"),
                    message_id: None,
                    requires_ack: false,
                    dry_run: false,
                    task_id: None,
                    error_code: None,
                    error_message: None,
                })
                .expect("admitted before the poison");
        }
        let logger = Arc::clone(&observability.logger);
        poison(move || {
            let _guard = logger.lock().expect("lock before poisoning");
            panic!("deliberate poison");
        });
        assert!(observability.logger.is_poisoned());
        observability
            .flush_logger(tokio::time::Instant::now() + Duration::from_secs(5))
            .await;
        let written = std::fs::read_to_string(&observability.active_log_path)
            .expect("log file")
            .matches("poison_drain")
            .count();
        assert_eq!(written, BURST, "the flush drained every admitted event");
    }
}
