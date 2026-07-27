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

use crate::daemon_runtime_observability::{DaemonActionName, DaemonOutcomeLabel};
use crate::{DaemonEvent, DaemonRuntimeObservability, DaemonSubsystem};

#[derive(Debug)]
pub(crate) struct TestDaemonObservability {
    active_log_path: PathBuf,
    detail: Option<String>,
    // append_message() pushes under the Mutex while wait_for_message_contains()
    // re-acquires it on each Condvar wake, so the pair stays co-located to keep
    // the shared lock/wake discipline explicit in test support.
    recorded_messages: (Mutex<Vec<String>>, Condvar),
}

impl TestDaemonObservability {
    pub(crate) fn new(log_dir: PathBuf) -> Result<Self, AtmError> {
        fs::create_dir_all(&log_dir).map_err(|_source| {
            AtmError::observability_bootstrap(format!(
                "failed to create retained log directory {}",
                log_dir.display()
            ))
        })?;
        let active_log_path = log_dir.join("atm.log.jsonl");
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active_log_path)
            .map_err(|_source| {
                AtmError::observability_bootstrap(format!(
                    "failed to open retained log file {} during startup",
                    active_log_path.display()
                ))
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
            .map_err(|_source| {
                AtmError::observability_emit(format!(
                    "failed to open retained test log file {}",
                    self.active_log_path.display()
                ))
            })?;
        writeln!(file, "{message}").map_err(|_source| {
            AtmError::observability_emit(format!(
                "failed to append retained test log file {}",
                self.active_log_path.display()
            ))
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
                return Err(AtmError::observability_health(format!(
                    "timed out waiting for retained test log message containing {needle:?}; last_seen={last_seen}"
                )));
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
        Err(AtmError::observability_query(
            "daemon retained-log query is unavailable from the test daemon logger adapter",
        ))
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Err(AtmError::observability_follow(
            "daemon retained-log follow is unavailable from the test daemon logger adapter",
        ))
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        let detail = self.detail.clone();
        match OpenOptions::new().append(true).open(&self.active_log_path) {
            Ok(_) => Ok(AtmObservabilityHealth {
                active_log_path: Some(self.active_log_path.clone()),
                logging_state: AtmObservabilityHealthState::Healthy,
                query_state: None,
                maintenance: None,
                diagnostic: None,
                detail,
            }),
            Err(source) => Ok(AtmObservabilityHealth {
                active_log_path: Some(self.active_log_path.clone()),
                logging_state: AtmObservabilityHealthState::Unavailable,
                query_state: None,
                maintenance: None,
                diagnostic: None,
                detail: Some(format!("{source}")),
            }),
        }
    }
}

impl DaemonRuntimeObservability for TestDaemonObservability {
    fn best_effort_preflush_blocking(&self) -> Result<(), AtmError> {
        self.best_effort_flush_blocking()
    }

    fn emit_daemon_event(&self, event: DaemonEvent) -> Result<(), AtmError> {
        self.append_message(format!(
            "{{\"subsystem\":\"{}\",\"action\":\"{}\",\"outcome\":\"{}\",\"message\":\"{}\"}}",
            event.subsystem.as_str(),
            event.action.as_str(),
            event.outcome.as_str(),
            event.detail
        ))
    }

    fn emit_subsystem_event(
        &self,
        subsystem: DaemonSubsystem,
        action: &DaemonActionName,
        outcome: &DaemonOutcomeLabel,
        message: &str,
        error_code: Option<AtmErrorCode>,
    ) -> Result<(), AtmError> {
        self.append_message(format!(
            "{{\"subsystem\":\"{subsystem}\",\"action\":\"{action}\",\"outcome\":\"{outcome}\",\"message\":\"{message}\",\"error_code\":{:?}}}",
            error_code.map(|value| value.to_string()),
            subsystem = subsystem.as_str(),
            action = action.as_str(),
            outcome = outcome.as_str()
        ))
    }

    fn best_effort_flush_blocking(&self) -> Result<(), AtmError> {
        match OpenOptions::new().append(true).open(&self.active_log_path) {
            Ok(file) => file.sync_all().map_err(|_source| {
                AtmError::observability_health(format!(
                    "failed to sync retained test log file at {}",
                    self.active_log_path.display()
                ))
            }),
            Err(source) if source.kind() == ErrorKind::NotFound => Ok(()),
            Err(_source) => Err(AtmError::observability_health(format!(
                "failed to open retained test log file at {} for best-effort flush",
                self.active_log_path.display()
            ))),
        }
    }
}
