use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fmt, thread};

use atm_core::error::AtmError;
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
        self.ensure_daemon_available_with_timeout(
            try_connect,
            AUTO_START_PUBLISH_TIMEOUT,
            Duration::from_millis(25),
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
        self.ensure_daemon_available_with_lock_path(
            try_connect,
            publish_timeout,
            poll_interval,
            atm_core::home::host_runtime_lock_path(HOST_RUNTIME_LAUNCH_LOCK_FILE)?,
        )
    }

    pub fn ensure_daemon_available_with_lock_path<F>(
        &self,
        mut try_connect: F,
        publish_timeout: Duration,
        poll_interval: Duration,
        launch_lock_path: PathBuf,
    ) -> Result<(), AtmError>
    where
        F: FnMut() -> Result<(), AtmError>,
    {
        if try_connect().is_ok() {
            return Ok(());
        }
        let deadline = Instant::now() + publish_timeout;
        loop {
            if try_connect().is_ok() {
                return Ok(());
            }
            if let Some(_guard) = LaunchGateGuard::try_acquire_at(launch_lock_path.clone())? {
                if try_connect().is_ok() {
                    return Ok(());
                }
                self.spawn_daemon()?;
                while Instant::now() < deadline {
                    if try_connect().is_ok() {
                        return Ok(());
                    }
                    thread::sleep(poll_interval);
                }
                return Err(AtmError::daemon_auto_start_failed(format!(
                    "failed to connect to daemon local IPC endpoint at {} after auto-start",
                    self.endpoint.display()
                )));
            }
            if Instant::now() >= deadline {
                return Err(LaunchGateGuard::rejected_error(&self.endpoint));
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
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
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
