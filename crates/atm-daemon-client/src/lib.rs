use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fmt, thread};

use atm_core::error::AtmError;
use atm_core::observability::{CommandEvent, ObservabilityPort};
use atm_core::types::{AgentName, TeamName};
use fs2::FileExt;

pub const AUTO_START_PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);
pub const HOST_RUNTIME_LAUNCH_LOCK_FILE: &str = "launch.lock";

#[derive(Debug, Clone)]
pub struct DaemonLocalIpcEndpoint(PathBuf);

impl DaemonLocalIpcEndpoint {
    pub fn new(path: PathBuf) -> Result<Self, AtmError> {
        validate_daemon_path("daemon local IPC endpoint", &path)?;
        Ok(Self(path))
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for DaemonLocalIpcEndpoint {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct DaemonBinaryPath(PathBuf);

impl DaemonBinaryPath {
    pub fn new(path: PathBuf) -> Result<Self, AtmError> {
        validate_daemon_path("daemon binary path", &path)?;
        Ok(Self(path))
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for DaemonBinaryPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

fn validate_daemon_path(label: &str, path: &Path) -> Result<(), AtmError> {
    if path.as_os_str().is_empty() {
        return Err(AtmError::validation(format!("{label} must not be empty")).with_recovery(
            "Set ATM_DAEMON_SOCKET to a non-empty UTF-8 daemon local IPC endpoint before invoking the same-host ATM daemon client.",
        ));
    }
    if path.to_str().is_none() {
        return Err(AtmError::validation(format!(
            "{label} must be valid UTF-8 at the ATM boundary"
        ))
        .with_recovery(
            "Set ATM_DAEMON_SOCKET to a non-empty UTF-8 daemon local IPC endpoint before invoking the same-host ATM daemon client.",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub struct DaemonSupervisor {
    endpoint: DaemonLocalIpcEndpoint,
    daemon_bin: DaemonBinaryPath,
}

struct BootstrapTraceability<'a> {
    command: &'static str,
    observability: &'a (dyn ObservabilityPort + Send + Sync),
    team: TeamName,
    agent: AgentName,
}

impl<'a> BootstrapTraceability<'a> {
    fn new(
        command: &'static str,
        observability: &'a (dyn ObservabilityPort + Send + Sync),
    ) -> Result<Self, AtmError> {
        Ok(Self {
            command,
            observability,
            team: parse_bootstrap_team()?,
            agent: parse_bootstrap_agent()?,
        })
    }

    fn emit(&self, action: &'static str, outcome: &'static str, error: Option<&AtmError>) {
        let event = CommandEvent {
            command: self.command,
            action,
            outcome,
            team: self.team.clone(),
            agent: self.agent.clone(),
            sender: self.agent.clone(),
            message_id: None,
            requires_ack: false,
            dry_run: false,
            task_id: None,
            error_code: error.map(|error| error.code),
            error_message: error.map(ToString::to_string),
        };
        if let Err(emit_error) = self.observability.emit(event) {
            tracing::warn!(
                command = self.command,
                action,
                outcome,
                error = ?emit_error,
                "emit failed"
            );
        }
    }
}

impl DaemonSupervisor {
    pub fn new(endpoint: DaemonLocalIpcEndpoint, daemon_bin: DaemonBinaryPath) -> Self {
        Self {
            endpoint,
            daemon_bin,
        }
    }

    pub fn ensure_daemon_available<F>(&self, try_connect: F) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_timeout_impl(
            try_connect,
            AUTO_START_PUBLISH_TIMEOUT,
            Duration::from_millis(25),
            None,
        )
    }

    pub fn ensure_daemon_available_with_traceability<F>(
        &self,
        command: &'static str,
        observability: &(dyn ObservabilityPort + Send + Sync),
        try_connect: F,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        let traceability = BootstrapTraceability::new(command, observability)?;
        self.ensure_daemon_available_with_timeout_impl(
            try_connect,
            AUTO_START_PUBLISH_TIMEOUT,
            Duration::from_millis(25),
            Some(&traceability),
        )
    }

    pub fn ensure_daemon_available_with_timeout<F>(
        &self,
        try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_timeout_impl(
            try_connect,
            publish_timeout,
            poll_interval,
            None,
        )
    }

    fn ensure_daemon_available_with_timeout_impl<F>(
        &self,
        try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
        traceability: Option<&BootstrapTraceability<'_>>,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_lock_path_impl(
            try_connect,
            publish_timeout,
            poll_interval,
            atm_core::home::host_runtime_lock_path(HOST_RUNTIME_LAUNCH_LOCK_FILE)?,
            traceability,
        )
    }

    pub fn ensure_daemon_available_with_lock_path<F>(
        &self,
        try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
        launch_lock_path: PathBuf,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        self.ensure_daemon_available_with_lock_path_impl(
            try_connect,
            publish_timeout,
            poll_interval,
            launch_lock_path,
            None,
        )
    }

    fn ensure_daemon_available_with_lock_path_impl<F>(
        &self,
        mut try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
        launch_lock_path: PathBuf,
        traceability: Option<&BootstrapTraceability<'_>>,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        if try_connect().is_ok() {
            if let Some(traceability) = traceability {
                traceability.emit("daemon_connect", "connected", None);
            }
            return Ok(());
        }
        if let Some(traceability) = traceability {
            traceability.emit("daemon_connect", "pending", None);
        }
        let deadline = Instant::now() + publish_timeout;
        let mut gate_contention_reported = false;
        loop {
            if try_connect().is_ok() {
                if let Some(traceability) = traceability {
                    traceability.emit("daemon_connect", "connected", None);
                }
                return Ok(());
            }
            let launch_gate = match LaunchGateGuard::try_acquire_at(launch_lock_path.clone()) {
                Ok(launch_gate) => launch_gate,
                Err(error) => {
                    if let Some(traceability) = traceability {
                        traceability.emit("daemon_connect", "error", Some(&error));
                    }
                    return Err(error);
                }
            };
            if let Some(_guard) = launch_gate {
                if let Some(traceability) = traceability {
                    traceability.emit("daemon_auto_start", "launch_gate_acquired", None);
                }
                if try_connect().is_ok() {
                    if let Some(traceability) = traceability {
                        traceability.emit("daemon_connect", "connected", None);
                    }
                    return Ok(());
                }
                if let Some(traceability) = traceability {
                    traceability.emit("daemon_auto_start", "spawn_requested", None);
                }
                if let Err(error) = self.spawn_daemon() {
                    if let Some(traceability) = traceability {
                        traceability.emit("daemon_auto_start", "error", Some(&error));
                    }
                    return Err(error);
                }
                if let Some(traceability) = traceability {
                    traceability.emit("daemon_auto_start", "publish_wait_started", None);
                }
                while Instant::now() < deadline {
                    if try_connect().is_ok() {
                        if let Some(traceability) = traceability {
                            traceability.emit("daemon_connect", "connected", None);
                        }
                        return Ok(());
                    }
                    thread::sleep(poll_interval);
                }
                let error = AtmError::daemon_auto_start_failed(format!(
                    "failed to connect to daemon local IPC endpoint at {} after auto-start",
                    self.endpoint.display()
                ));
                if let Some(traceability) = traceability {
                    traceability.emit("daemon_auto_start", "error", Some(&error));
                }
                return Err(error);
            }
            if !gate_contention_reported {
                if let Some(traceability) = traceability {
                    traceability.emit("daemon_auto_start", "launch_gate_contended", None);
                }
                gate_contention_reported = true;
            }
            if Instant::now() >= deadline {
                let error = LaunchGateGuard::rejected_error(&self.endpoint);
                if let Some(traceability) = traceability {
                    traceability.emit("daemon_auto_start", "error", Some(&error));
                }
                return Err(error);
            }
            thread::sleep(poll_interval);
        }
    }

    fn spawn_daemon(&self) -> Result<(), AtmError> {
        if !self.daemon_bin.as_ref().is_file() {
            return Err(
                AtmError::daemon_unavailable(format!(
                    "daemon binary is missing at {}",
                    self.daemon_bin.display()
                ))
                .with_recovery(
                    "Build or install atm-daemon, or set ATM_DAEMON_BIN to the correct executable before retrying.",
                ),
            );
        }

        let mut command = Command::new(self.daemon_bin.as_ref());
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .env("ATM_DAEMON_SOCKET", self.endpoint.as_ref());
        command.spawn().map_err(|source| {
            AtmError::daemon_auto_start_failed(format!(
                "failed to spawn daemon binary at {}",
                self.daemon_bin.display()
            ))
            .with_source(source)
        })?;
        Ok(())
    }
}

pub struct LaunchGateGuard {
    file: File,
}

impl fmt::Debug for LaunchGateGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaunchGateGuard").finish_non_exhaustive()
    }
}

impl LaunchGateGuard {
    pub fn rejected_error(endpoint: &DaemonLocalIpcEndpoint) -> AtmError {
        AtmError::daemon_launch_gate_rejected(format!(
            "daemon launch gate remained owned while connecting to {}",
            endpoint.display()
        ))
    }

    pub fn try_acquire_at(lock_path: PathBuf) -> Result<Option<Self>, AtmError> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to create daemon launch lock directory at {}",
                    parent.display()
                ))
                .with_source(source)
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| {
                AtmError::daemon_unavailable(format!(
                    "failed to open daemon launch gate at {}",
                    lock_path.display()
                ))
                .with_source(source)
            })?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error) if is_launch_gate_contention_error(&error) => Ok(None),
            Err(source) => Err(AtmError::daemon_unavailable(format!(
                "failed to acquire daemon launch gate at {}",
                lock_path.display()
            ))
            .with_source(source)),
        }
    }
}

impl Drop for LaunchGateGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn is_launch_gate_contention_error(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }

    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(32 | 33))
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn parse_bootstrap_agent() -> Result<AgentName, AtmError> {
    std::env::var("ATM_IDENTITY")
        .unwrap_or_else(|_| "unknown".to_string())
        .parse()
}

fn parse_bootstrap_team() -> Result<TeamName, AtmError> {
    std::env::var("ATM_TEAM")
        .unwrap_or_else(|_| "unknown".to_string())
        .parse()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
    use std::time::Duration;

    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    };
    use tempfile::TempDir;

    use super::{
        DaemonBinaryPath, DaemonLocalIpcEndpoint, DaemonSupervisor, HOST_RUNTIME_LAUNCH_LOCK_FILE,
        LaunchGateGuard, parse_bootstrap_agent, parse_bootstrap_team,
    };

    #[derive(Debug, Default)]
    struct RecordingObservability {
        events: Mutex<Vec<CommandEvent>>,
    }

    impl RecordingObservability {
        fn events(&self) -> Vec<CommandEvent> {
            self.events.lock().expect("events lock").clone()
        }
    }

    impl atm_core::boundary::sealed::Sealed for RecordingObservability {}

    impl ObservabilityPort for RecordingObservability {
        fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
            self.events.lock().expect("events lock").push(event);
            Ok(())
        }

        fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
            Ok(AtmLogSnapshot::default())
        }

        fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
            Ok(LogTailSession::empty())
        }

        fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
            Ok(AtmObservabilityHealth {
                active_log_path: None,
                logging_state: AtmObservabilityHealthState::Healthy,
                query_state: Some(AtmObservabilityHealthState::Healthy),
                detail: None,
            })
        }
    }

    struct EnvGuard {
        restorations: Vec<EnvRestore>,
        _guard: MutexGuard<'static, ()>,
    }

    struct EnvRestore {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvGuard {
        fn set_many<const N: usize>(changes: [(&'static str, Option<&str>); N]) -> Self {
            let guard = env_lock().lock().expect("env lock");
            Self {
                restorations: changes
                    .into_iter()
                    .map(|(key, value)| {
                        let original = std::env::var_os(key);
                        match value {
                            Some(value) => unsafe { std::env::set_var(key, value) },
                            None => unsafe { std::env::remove_var(key) },
                        }
                        EnvRestore { key, original }
                    })
                    .collect(),
                _guard: guard,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for restore in self.restorations.iter_mut().rev() {
                match restore.original.take() {
                    Some(value) => unsafe { std::env::set_var(restore.key, value) },
                    None => unsafe { std::env::remove_var(restore.key) },
                }
            }
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn supervisor(tempdir: &TempDir) -> DaemonSupervisor {
        DaemonSupervisor::new(
            DaemonLocalIpcEndpoint::new(tempdir.path().join("daemon.sock")).expect("endpoint"),
            DaemonBinaryPath::new(tempdir.path().join("atm-daemon")).expect("daemon path"),
        )
    }

    fn launch_lock_path(tempdir: &TempDir) -> PathBuf {
        tempdir.path().join(HOST_RUNTIME_LAUNCH_LOCK_FILE)
    }

    #[test]
    fn bootstrap_traceability_uses_unknown_identity_defaults() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", None), ("ATM_TEAM", None)]);

        assert_eq!(parse_bootstrap_agent().expect("agent").as_str(), "unknown");
        assert_eq!(parse_bootstrap_team().expect("team").as_str(), "unknown");
    }

    #[test]
    fn traceability_emits_pending_and_connected_for_retry_success() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("trace-agent")),
            ("ATM_TEAM", Some("trace-team")),
        ]);
        let tempdir = TempDir::new().expect("tempdir");
        let supervisor = supervisor(&tempdir);
        let observability = RecordingObservability::default();
        let attempts = Arc::new(AtomicUsize::new(0));
        let try_connect_attempts = Arc::clone(&attempts);
        let traceability =
            super::BootstrapTraceability::new("send", &observability).expect("traceability");

        supervisor
            .ensure_daemon_available_with_lock_path_impl(
                move || {
                    if try_connect_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(AtmError::daemon_unavailable("not ready"))
                    } else {
                        Ok(())
                    }
                },
                Duration::from_millis(5),
                Duration::from_millis(1),
                launch_lock_path(&tempdir),
                Some(&traceability),
            )
            .expect("daemon available");

        let events = observability.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].command, "send");
        assert_eq!(events[0].action, "daemon_connect");
        assert_eq!(events[0].outcome, "pending");
        assert_eq!(events[1].action, "daemon_connect");
        assert_eq!(events[1].outcome, "connected");
        assert_eq!(events[1].team.as_str(), "trace-team");
        assert_eq!(events[1].agent.as_str(), "trace-agent");
    }

    #[test]
    fn traceability_emits_spawn_failure_error() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("trace-agent")),
            ("ATM_TEAM", Some("trace-team")),
        ]);
        let tempdir = TempDir::new().expect("tempdir");
        let supervisor = supervisor(&tempdir);
        let observability = RecordingObservability::default();
        let traceability =
            super::BootstrapTraceability::new("doctor", &observability).expect("traceability");

        let error = supervisor
            .ensure_daemon_available_with_lock_path_impl(
                || Err(AtmError::daemon_unavailable("not ready")),
                Duration::from_millis(5),
                Duration::from_millis(1),
                launch_lock_path(&tempdir),
                Some(&traceability),
            )
            .expect_err("spawn failure");

        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        let events = observability.events();
        assert!(
            events
                .iter()
                .any(|event| event.action == "daemon_auto_start"
                    && event.outcome == "spawn_requested")
        );
        let error_event = events
            .iter()
            .find(|event| event.action == "daemon_auto_start" && event.outcome == "error")
            .expect("error event");
        assert_eq!(error_event.command, "doctor");
        assert_eq!(
            error_event.error_code,
            Some(AtmErrorCode::DaemonUnavailable)
        );
        assert!(
            error_event
                .error_message
                .as_deref()
                .expect("error message")
                .contains("daemon binary is missing")
        );
    }

    #[test]
    fn launch_gate_rejected_error_uses_daemon_launch_gate_code() {
        let tempdir = TempDir::new().expect("tempdir");
        let endpoint =
            DaemonLocalIpcEndpoint::new(tempdir.path().join("daemon.sock")).expect("endpoint");

        let error = LaunchGateGuard::rejected_error(&endpoint);
        assert_eq!(error.code, AtmErrorCode::DaemonLaunchGateRejected);
    }
}
