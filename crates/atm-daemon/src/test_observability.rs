use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort,
};

use crate::DaemonRuntimeObservability;

#[derive(Debug)]
pub(crate) struct TestDaemonObservability {
    active_log_path: PathBuf,
    detail: Mutex<Option<String>>,
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
            detail: Mutex::new(None),
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
        })
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
        let detail = self
            .detail
            .lock()
            .expect("test observability detail")
            .clone();
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
    fn emit_runtime_event(
        &self,
        action: &'static str,
        outcome: &'static str,
        message: &'static str,
    ) -> Result<(), AtmError> {
        self.append_message(format!(
            "{{\"action\":\"{action}\",\"outcome\":\"{outcome}\",\"message\":\"{message}\"}}"
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
