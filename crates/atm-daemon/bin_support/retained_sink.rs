use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use std::{fs, fs::OpenOptions};

use super::{
    DiagnosticSummary, ErrorCode, ErrorContext, LogEvent, RETAINED_LOG_PRUNE_INTERVAL,
    Remediation, SinkHealth, SinkHealthState, SinkName,
};

const RETAINED_LOG_WORKER_QUEUE_CAPACITY: usize = 256;
const RETAINED_LOG_PRUNE_MAX_FILES_PER_PASS: usize = 64;

#[derive(Debug)]
pub(super) struct RetainedJsonlFileSink {
    health: Arc<Mutex<SinkHealth>>,
    worker_tx: SyncSender<WorkerMessage>,
    worker_join_handle: Mutex<Option<thread::JoinHandle<()>>>,
}

enum WorkerMessage {
    Write(Vec<u8>),
    Flush(mpsc::SyncSender<std::io::Result<()>>),
    Sync(mpsc::SyncSender<std::io::Result<()>>),
    Shutdown(mpsc::SyncSender<std::io::Result<()>>),
    #[cfg(test)]
    PruneTick,
}

struct WorkerState {
    path: PathBuf,
    rotation: sc_observability::RotationPolicy,
    retention: sc_observability::RetentionPolicy,
    last_written_file: Option<std::fs::File>,
    last_prune_at: Option<SystemTime>,
}

impl RetainedJsonlFileSink {
    pub(super) fn new(
        path: PathBuf,
        rotation: sc_observability::RotationPolicy,
        retention: sc_observability::RetentionPolicy,
    ) -> Self {
        let (worker_tx, worker_rx) = mpsc::sync_channel(RETAINED_LOG_WORKER_QUEUE_CAPACITY);
        let worker_path = path.clone();
        let health = Arc::new(Mutex::new(SinkHealth {
            name: SinkName::new("jsonl_file_sink").expect("jsonl sink constant is valid"),
            state: SinkHealthState::Healthy,
            last_error: None,
        }));
        let worker_health = Arc::clone(&health);
        let worker_join_handle = thread::Builder::new()
            .name("atm-log-maintenance".to_string())
            .spawn(move || {
                let mut state = WorkerState::new(worker_path, rotation, retention);
                worker_loop(&mut state, worker_rx, worker_health);
            })
            .expect("retained log maintenance worker should spawn");

        Self {
            health,
            worker_tx,
            worker_join_handle: Mutex::new(Some(worker_join_handle)),
        }
    }

    pub(super) fn sync_last_written_file(&self) -> std::io::Result<()> {
        self.request_worker_ack(WorkerMessage::Sync, "retained sink sync request")
    }

    pub(super) fn join_prune_worker_with_timeout(&self, timeout: Duration) -> std::io::Result<()> {
        let Some(worker_handle) = self
            .worker_join_handle
            .lock()
            .map_err(|_| std::io::Error::other("retained sink worker join lock poisoned"))?
            .take()
        else {
            return Ok(());
        };

        self.request_worker_ack_with_timeout(
            WorkerMessage::Shutdown,
            timeout,
            "retained sink shutdown request",
        )?;

        worker_handle
            .join()
            .map_err(|_| std::io::Error::other("retained sink maintenance worker panicked"))
    }

    #[cfg(test)]
    pub(super) fn schedule_prune_old_files(&self) {
        let _ = self.worker_tx.try_send(WorkerMessage::PruneTick);
    }

    fn request_worker_ack(
        &self,
        message: fn(mpsc::SyncSender<std::io::Result<()>>) -> WorkerMessage,
        context: &'static str,
    ) -> std::io::Result<()> {
        self.request_worker_ack_with_timeout(message, Duration::from_secs(5), context)
    }

    fn request_worker_ack_with_timeout(
        &self,
        message: fn(mpsc::SyncSender<std::io::Result<()>>) -> WorkerMessage,
        timeout: Duration,
        context: &'static str,
    ) -> std::io::Result<()> {
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        self.worker_tx
            .send(message(ack_tx))
            .map_err(|_| std::io::Error::other(format!("{context} could not reach retained worker")))?;
        match ack_rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(std::io::Error::other(format!(
                "{context} timed out waiting for retained worker"
            ))),
            Err(RecvTimeoutError::Disconnected) => Err(std::io::Error::other(format!(
                "{context} failed because the retained worker exited early"
            ))),
        }
    }

    fn mark_failure<E>(&self, error: E) -> sc_observability_types::LogSinkError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        let message = error.to_string();
        let diagnostic = ErrorContext::new(
            ErrorCode::new_static("SC_LOGGER_SINK_WRITE_FAILED"),
            "jsonl file sink write failed",
            Remediation::not_recoverable(
                "file sink write failure handling is owned by the logger runtime",
            ),
        )
        .cause(message)
        .source(Box::new(error));
        if let Ok(mut health) = self.health.lock() {
            health.state = SinkHealthState::DegradedDropping;
            health.last_error = Some(DiagnosticSummary::from(diagnostic.diagnostic()));
        } else {
            tracing::warn!("file sink health lock poisoned while recording sink failure");
        }
        sc_observability_types::LogSinkError(Box::new(diagnostic))
    }

    fn mark_healthy(&self) {
        if let Ok(mut health) = self.health.lock() {
            health.state = SinkHealthState::Healthy;
            health.last_error = None;
        }
    }
}

impl sc_observability::LogSink for RetainedJsonlFileSink {
    fn write(&self, event: &LogEvent) -> Result<(), sc_observability_types::LogSinkError> {
        let mut line = serde_json::to_vec(event).map_err(|error| self.mark_failure(error))?;
        line.push(b'\n');
        match self.worker_tx.try_send(WorkerMessage::Write(line)) {
            Ok(()) => {
                self.mark_healthy();
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(self.mark_failure(std::io::Error::other(
                format!(
                    "retained sink queue reached capacity {}",
                    RETAINED_LOG_WORKER_QUEUE_CAPACITY
                ),
            ))),
            Err(TrySendError::Disconnected(_)) => Err(self.mark_failure(std::io::Error::other(
                "retained sink maintenance worker is unavailable",
            ))),
        }
    }

    fn flush(&self) -> Result<(), sc_observability_types::LogSinkError> {
        self.request_worker_ack(WorkerMessage::Flush, "retained sink flush request")
            .map_err(|error| self.mark_failure(error))
    }

    fn health(&self) -> SinkHealth {
        match self.health.lock() {
            Ok(health) => health.clone(),
            Err(_) => {
                tracing::warn!("file sink health lock poisoned; reporting unavailable sink health");
                SinkHealth {
                    name: SinkName::new("jsonl_file_sink").expect("jsonl sink constant is valid"),
                    state: SinkHealthState::Unavailable,
                    last_error: None,
                }
            }
        }
    }
}

impl WorkerState {
    fn new(
        path: PathBuf,
        rotation: sc_observability::RotationPolicy,
        retention: sc_observability::RetentionPolicy,
    ) -> Self {
        Self {
            path,
            rotation,
            retention,
            last_written_file: None,
            last_prune_at: None,
        }
    }

    fn handle_write(&mut self, line: &[u8]) -> std::io::Result<()> {
        self.ensure_parent_dir()?;
        self.rotate_if_needed(line.len() as u64)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line)?;
        self.last_written_file = Some(file);
        self.maybe_prune()?;
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(file) = self.last_written_file.as_mut() {
            file.flush()?;
        }
        Ok(())
    }

    fn sync_last_written_file(&mut self) -> std::io::Result<()> {
        if let Some(file) = self.last_written_file.as_mut() {
            file.sync_all()?;
        }
        Ok(())
    }

    fn finalize(&mut self) -> std::io::Result<()> {
        self.flush()?;
        self.sync_last_written_file()?;
        Ok(())
    }

    fn ensure_parent_dir(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn rotate_if_needed(&mut self, incoming_len: u64) -> std::io::Result<()> {
        if let Ok(metadata) = fs::metadata(&self.path)
            && metadata.len().saturating_add(incoming_len) > self.rotation.max_bytes
        {
            self.last_written_file = None;
            for idx in (1..self.rotation.max_files).rev() {
                let src = self.rotated_path(idx);
                let dest = self.rotated_path(idx + 1);
                let _ = rename_if_present(&src, &dest);
            }
            let rotated = self.rotated_path(1);
            let _ = rename_if_present(&self.path, &rotated);
        }
        Ok(())
    }

    fn maybe_prune(&mut self) -> std::io::Result<()> {
        let now = SystemTime::now();
        if self
            .last_prune_at
            .and_then(|last| now.duration_since(last).ok())
            .is_some_and(|elapsed| elapsed < RETAINED_LOG_PRUNE_INTERVAL)
        {
            return Ok(());
        }
        self.prune_old_files()?;
        self.last_prune_at = Some(now);
        Ok(())
    }

    fn prune_old_files(&self) -> std::io::Result<()> {
        let Some(parent) = self.path.parent() else {
            return Ok(());
        };
        let entries = fs::read_dir(parent)?;
        let retention_cutoff =
            SystemTime::now() - Duration::from_secs(u64::from(self.retention.max_age_days) * 86_400);
        let active_name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        for entry in entries.flatten().take(RETAINED_LOG_PRUNE_MAX_FILES_PER_PASS) {
            let candidate = entry.path();
            let Some(file_name) = candidate.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !file_name.starts_with(active_name) || file_name == active_name {
                continue;
            }
            if let Ok(metadata) = entry.metadata()
                && let Ok(modified) = metadata.modified()
                && modified < retention_cutoff
            {
                let _ = fs::remove_file(candidate);
            }
        }
        Ok(())
    }

    fn rotated_path(&self, index: u32) -> PathBuf {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("atm.log.jsonl");
        parent.join(format!("{file_name}.{index}"))
    }
}

fn worker_loop(
    state: &mut WorkerState,
    worker_rx: Receiver<WorkerMessage>,
    health: Arc<Mutex<SinkHealth>>,
) {
    while let Ok(message) = worker_rx.recv() {
        match message {
            WorkerMessage::Write(line) => {
                if let Err(error) = state.handle_write(&line) {
                    mark_worker_failure(&health, error);
                } else {
                    mark_worker_healthy(&health);
                }
            }
            WorkerMessage::Flush(reply_tx) => {
                let result = state.flush();
                if let Err(error) = &result {
                    mark_worker_failure(&health, std::io::Error::new(error.kind(), error.to_string()));
                }
                let _ = reply_tx.send(result);
            }
            WorkerMessage::Sync(reply_tx) => {
                let result = state.sync_last_written_file();
                if let Err(error) = &result {
                    mark_worker_failure(&health, std::io::Error::new(error.kind(), error.to_string()));
                }
                let _ = reply_tx.send(result);
            }
            WorkerMessage::Shutdown(reply_tx) => {
                let result = state.finalize();
                if let Err(error) = &result {
                    mark_worker_failure(&health, std::io::Error::new(error.kind(), error.to_string()));
                }
                let _ = reply_tx.send(result);
                break;
            }
            #[cfg(test)]
            WorkerMessage::PruneTick => {
                if let Err(error) = state.prune_old_files() {
                    mark_worker_failure(&health, error);
                }
            }
        }
    }
}

fn mark_worker_failure(health: &Arc<Mutex<SinkHealth>>, error: std::io::Error) {
    if let Ok(mut sink_health) = health.lock() {
        let diagnostic = ErrorContext::new(
            ErrorCode::new_static("SC_LOGGER_SINK_WRITE_FAILED"),
            "jsonl file sink write failed",
            Remediation::not_recoverable(
                "file sink write failure handling is owned by the logger runtime",
            ),
        )
        .cause(error.to_string());
        sink_health.state = SinkHealthState::DegradedDropping;
        sink_health.last_error = Some(DiagnosticSummary::from(diagnostic.diagnostic()));
    }
}

fn mark_worker_healthy(health: &Arc<Mutex<SinkHealth>>) {
    if let Ok(mut sink_health) = health.lock() {
        sink_health.state = SinkHealthState::Healthy;
        sink_health.last_error = None;
    }
}

fn rename_if_present(src: &Path, dest: &Path) -> std::io::Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
