use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort,
};

use crate::{DaemonEvent, DaemonRuntimeObservability};

#[derive(Debug)]
pub(crate) struct TestDaemonObservability {
    active_log_path: PathBuf,
    detail: Option<String>,
    recorded_messages: (Mutex<Vec<String>>, Condvar),
}

impl TestDaemonObservability {
    pub(crate) fn new(log_dir: PathBuf) -> Result<Self, AtmError> {
        fs::create_dir_all(&log_dir).map_err(|source| {
            AtmError::observability_bootstrap(format!(
                "failed to create retained log directory {}",
                log_dir.display()
            ))
            .with_source(source)
        })?;
        let active_log_path = log_dir.join("atm.log.jsonl");
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active_log_path)
            .map_err(|source| {
                AtmError::observability_bootstrap(format!(
                    "failed to open retained log file {} during startup",
                    active_log_path.display()
                ))
                .with_source(source)
            })?;
        Ok(Self {
            active_log_path,
            detail: None,
            recorded_messages: (Mutex::new(Vec::new()), Condvar::new()),
        })
    }

    fn append_message(&self, message: String) -> Result<(), AtmError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.active_log_path)
            .map_err(|source| {
                AtmError::observability_emit(format!(
                    "failed to open retained test log file {}",
                    self.active_log_path.display()
                ))
                .with_source(source)
            })?;
        writeln!(file, "{message}").map_err(|source| {
            AtmError::observability_emit(format!(
                "failed to append retained test log file {}",
                self.active_log_path.display()
            ))
            .with_source(source)
        })?;
        let (recorded, wake) = &self.recorded_messages;
        let mut recorded = recorded.lock().expect("test observability messages");
        recorded.push(message);
        drop(recorded);
        wake.notify_all();
        Ok(())
    }

    pub(crate) fn wait_for_message_contains(
        &self,
        needle: &str,
        timeout: Duration,
    ) -> Result<(), AtmError> {
        let deadline = Instant::now() + timeout;
        let (recorded, wake) = &self.recorded_messages;
        let mut recorded = recorded.lock().expect("test observability messages");
        loop {
            if recorded.iter().any(|entry| entry.contains(needle)) {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                let last_seen = if recorded.is_empty() {
                    "<no retained test messages recorded>".to_string()
                } else {
                    recorded
                        .iter()
                        .rev()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" | ")
                };
                return Err(
                    AtmError::observability_health(format!(
                        "timed out waiting for retained test log message containing {needle:?}; last_seen={last_seen}"
                    ))
                    .with_recovery(
                        "Retry the daemon observability test after verifying the retained-log adapter emitted the expected startup event.",
                    ),
                );
            }
            let wait = wake
                .wait_timeout(recorded, deadline.saturating_duration_since(now))
                .expect("test observability message wait");
            recorded = wait.0;
        }
    }
}

impl boundary::sealed::Sealed for TestDaemonObservability {}

impl ObservabilityPort for TestDaemonObservability {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
        self.append_message(format!(
            "{{\"command\":\"{}\",\"action\":\"{}\",\"outcome\":\"{}\"}}",
            event.command, event.action, event.outcome
        ))
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Err(
            AtmError::observability_query(
                "daemon retained-log query is unavailable from the test daemon logger adapter",
            )
            .with_recovery(
                "Use the CLI-owned retained log query surface for historical log reads until daemon query support is explicitly extracted.",
            ),
        )
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Err(
            AtmError::observability_follow(
                "daemon retained-log follow is unavailable from the test daemon logger adapter",
            )
            .with_recovery(
                "Use the CLI-owned retained log follow surface until daemon follow support is explicitly extracted.",
            ),
        )
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        let detail = self.detail.clone();
        match OpenOptions::new().append(true).open(&self.active_log_path) {
            Ok(_) => Ok(AtmObservabilityHealth {
                active_log_path: Some(self.active_log_path.clone()),
                logging_state: AtmObservabilityHealthState::Healthy,
                query_state: None,
                detail,
            }),
            Err(source) => Ok(AtmObservabilityHealth {
                active_log_path: Some(self.active_log_path.clone()),
                logging_state: AtmObservabilityHealthState::Unavailable,
                query_state: None,
                detail: Some(format!("{source}")),
            }),
        }
    }
}

impl DaemonRuntimeObservability for TestDaemonObservability {
    fn emit_daemon_event(&self, event: DaemonEvent) -> Result<(), AtmError> {
        self.append_message(format!(
            "{{\"subsystem\":\"{}\",\"action\":\"{}\",\"outcome\":\"{}\",\"message\":\"{}\"}}",
            event.subsystem.as_str(),
            event.action,
            event.outcome,
            event.detail
        ))
    }

    fn emit_subsystem_event(
        &self,
        subsystem: &'static str,
        action: &'static str,
        outcome: &'static str,
        message: &str,
        error_code: Option<AtmErrorCode>,
    ) -> Result<(), AtmError> {
        self.append_message(format!(
            "{{\"subsystem\":\"{subsystem}\",\"action\":\"{action}\",\"outcome\":\"{outcome}\",\"message\":\"{message}\",\"error_code\":{:?}}}",
            error_code.map(|value| value.to_string())
        ))
    }

    fn best_effort_flush_blocking(&self) -> Result<(), AtmError> {
        match OpenOptions::new().append(true).open(&self.active_log_path) {
            Ok(file) => file.sync_all().map_err(|source| {
                AtmError::observability_health(format!(
                    "failed to sync retained test log file at {}",
                    self.active_log_path.display()
                ))
                .with_source(source)
            }),
            Err(source) if source.kind() == ErrorKind::NotFound => Ok(()),
            Err(source) => Err(AtmError::observability_health(format!(
                "failed to open retained test log file at {} for best-effort flush",
                self.active_log_path.display()
            ))
            .with_source(source)),
        }
    }
}
