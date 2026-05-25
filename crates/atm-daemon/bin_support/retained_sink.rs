use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use std::{fs, fs::OpenOptions};

use super::{
    DiagnosticSummary, ErrorCode, ErrorContext, LogEvent, RETAINED_LOG_PRUNE_INTERVAL,
    RETAINED_LOG_PRUNE_JOIN_POLL_INTERVAL, Remediation, SinkHealth, SinkHealthState, SinkName,
};

const RETAINED_LOG_PRUNE_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Debug)]
// The retained sink keeps three separate mutexes because the health state is read and updated on
// the hot path, while the last-written file handle and prune timestamp are touched far less often.
// Keeping them independent avoids serializing unrelated reads/writes behind one coarse lock.
// Lock order matters when more than one is needed: write() updates last_written_file before health.
pub(super) struct RetainedJsonlFileSink {
    path: PathBuf,
    rotation: sc_observability::RotationPolicy,
    retention: sc_observability::RetentionPolicy,
    health: Mutex<SinkHealth>,
    last_written_file: Mutex<Option<std::fs::File>>,
    prune_in_progress: Arc<AtomicBool>,
    prune_join_handle: Mutex<Option<thread::JoinHandle<()>>>,
    last_prune_request_at: Mutex<Option<SystemTime>>,
}

impl RetainedJsonlFileSink {
    pub(super) fn new(
        path: PathBuf,
        rotation: sc_observability::RotationPolicy,
        retention: sc_observability::RetentionPolicy,
    ) -> Self {
        Self {
            path,
            rotation,
            retention,
            health: Mutex::new(SinkHealth {
                name: SinkName::new("jsonl_file_sink").expect("jsonl sink constant is valid"),
                state: SinkHealthState::Healthy,
                last_error: None,
            }),
            last_written_file: Mutex::new(None),
            prune_in_progress: Arc::new(AtomicBool::new(false)),
            prune_join_handle: Mutex::new(None),
            last_prune_request_at: Mutex::new(None),
        }
    }

    pub(super) fn sync_last_written_file(&self) -> std::io::Result<()> {
        let last_written = self
            .last_written_file
            .lock()
            .map_err(|_| std::io::Error::other("retained sink sync handle lock poisoned"))?;
        if let Some(file) = last_written.as_ref() {
            file.sync_all()?;
        }
        Ok(())
    }

    pub(super) fn join_prune_worker_with_timeout(&self, timeout: Duration) -> std::io::Result<()> {
        let mut join_handle = self
            .prune_join_handle
            .lock()
            .map_err(|_| std::io::Error::other("retained sink prune join lock poisoned"))?;
        let Some(handle) = join_handle.take() else {
            return Ok(());
        };
        if !wait_for_thread_completion(&handle, timeout) {
            *join_handle = Some(handle);
            return Ok(());
        }
        drop(join_handle);
        handle
            .join()
            .map_err(|_| std::io::Error::other("retained sink prune worker panicked"))
    }

    pub(super) fn schedule_prune_old_files(&self) {
        let _ = self.join_prune_worker_with_timeout(Duration::ZERO);
        let now = SystemTime::now();
        {
            let Ok(mut last_request_at) = self.last_prune_request_at.lock() else {
                tracing::warn!(
                    "retained sink prune request lock poisoned; skipping one prune scheduling attempt"
                );
                return;
            };
            if let Some(last_request_at) = *last_request_at
                && now
                    .duration_since(last_request_at)
                    .is_ok_and(|elapsed| elapsed < RETAINED_LOG_PRUNE_INTERVAL)
            {
                return;
            }
            *last_request_at = Some(now);
        }

        if self
            .prune_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let path = self.path.clone();
        let retention = self.retention;
        let prune_in_progress = Arc::clone(&self.prune_in_progress);
        match thread::Builder::new()
            .name("atm-log-prune".to_string())
            .spawn(move || {
                prune_old_files_at_path(&path, retention);
                prune_in_progress.store(false, Ordering::Release);
            })
        {
            Ok(handle) => {
                if let Ok(mut prune_join_handle) = self.prune_join_handle.lock() {
                    *prune_join_handle = Some(handle);
                } else {
                    self.prune_in_progress.store(false, Ordering::Release);
                }
            }
            Err(_) => {
                self.prune_in_progress.store(false, Ordering::Release);
            }
        }
    }

    fn rotate_if_needed(&self, incoming_len: u64) {
        if let Ok(metadata) = fs::metadata(&self.path)
            && metadata.len().saturating_add(incoming_len) > self.rotation.max_bytes
        {
            for idx in (1..self.rotation.max_files).rev() {
                let src = self.rotated_path(idx);
                let dest = self.rotated_path(idx + 1);
                let _ = rename_if_present(&src, &dest);
            }
            let rotated = self.rotated_path(1);
            let _ = rename_if_present(&self.path, &rotated);
        }
        self.schedule_prune_old_files();
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
}

impl sc_observability::LogSink for RetainedJsonlFileSink {
    fn write(&self, event: &LogEvent) -> Result<(), sc_observability_types::LogSinkError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| self.mark_failure(error))?;
        }
        let mut line = serde_json::to_vec(event).map_err(|error| self.mark_failure(error))?;
        line.push(b'\n');
        self.rotate_if_needed(line.len() as u64);
        // ADR-011: retained daemon writes stay on the logger-owned blocking path,
        // not on an async executor worker, so this synchronous file append is an
        // intentional OS-thread tradeoff rather than an executor stall hazard.
        // Reopen per append intentionally: retained daemon events prioritize append-safety across
        // rotation/replacement over holding one long-lived write handle open on the hot path.
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| self.mark_failure(error))?;
        file.write_all(&line)
            .and_then(|()| file.flush())
            .map_err(|error| self.mark_failure(error))?;
        // LOCK-ORDER: when write() needs both locks, update last_written_file
        // before health so it matches the struct-level invariant.
        *self.last_written_file.lock().map_err(|_| {
            self.mark_failure(std::io::Error::other(
                "retained sink file handle lock poisoned",
            ))
        })? = Some(file);
        // LOCK-ORDER: acquire health only after last_written_file in write().
        let mut health = self.health.lock().map_err(|_| {
            self.mark_failure(std::io::Error::other("file sink health lock poisoned"))
        })?;
        health.state = SinkHealthState::Healthy;
        Ok(())
    }

    fn flush(&self) -> Result<(), sc_observability_types::LogSinkError> {
        let mut last_written = self.last_written_file.lock().map_err(|_| {
            self.mark_failure(std::io::Error::other(
                "retained sink file handle lock poisoned",
            ))
        })?;
        if let Some(file) = last_written.as_mut() {
            file.flush().map_err(|error| self.mark_failure(error))?;
        }
        Ok(())
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

fn prune_old_files_at_path(path: &Path, retention: sc_observability::RetentionPolicy) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    let prune_deadline = SystemTime::now() + RETAINED_LOG_PRUNE_DEADLINE;
    let retention_cutoff =
        SystemTime::now() - Duration::from_secs(u64::from(retention.max_age_days) * 86_400);
    let active_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    for entry in entries.flatten() {
        if SystemTime::now() >= prune_deadline {
            break;
        }
        let candidate = entry.path();
        let Some(file_name) = candidate.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with(active_name) || file_name == active_name {
            continue;
        };
        if let Ok(metadata) = entry.metadata()
            && let Ok(modified) = metadata.modified()
            && modified < retention_cutoff
        {
            let _ = fs::remove_file(candidate);
        }
    }
}

fn wait_for_thread_completion(handle: &thread::JoinHandle<()>, timeout: Duration) -> bool {
    let deadline = SystemTime::now() + timeout;
    loop {
        if handle.is_finished() {
            return true;
        }
        if SystemTime::now() >= deadline {
            return false;
        }
        thread::sleep(RETAINED_LOG_PRUNE_JOIN_POLL_INTERVAL);
    }
}

fn rename_if_present(src: &Path, dest: &Path) -> std::io::Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
