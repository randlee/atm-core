//! Rust-owned diagnostics for native graft calls.
//!
//! This module deliberately owns a satellite file. It never opens the
//! daemon's canonical `atm.log.jsonl` file and it only serializes the fields
//! named by `atm-observability`'s retained allowlist.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use atm_observability::{RETAINED_FIELD_ALLOWLIST, graft_fallback_log_path};

pub const GRAFT_FALLBACK_MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const GRAFT_FALLBACK_KEEP_FILES: usize = 3;
pub const GRAFT_FALLBACK_QUEUE: usize = 256;

const WORKER_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ObservabilityStatus {
    pub fallback_write_failed: bool,
    pub code: Option<String>,
}

impl ObservabilityStatus {
    fn failed(code: impl Into<String>) -> Self {
        Self {
            fallback_write_failed: true,
            code: Some(code.into()),
        }
    }

    pub(crate) fn merge(&mut self, other: Self) {
        if other.fallback_write_failed {
            self.fallback_write_failed = true;
            if self.code.is_none() {
                self.code = other.code;
            }
        }
    }
}

#[derive(Debug)]
struct FallbackEvent {
    fields: Vec<(&'static str, String)>,
    result: SyncSender<Result<(), String>>,
}

/// Bounded, non-panicking writer for graft fallback diagnostics.
#[derive(Clone, Debug)]
pub(crate) struct GraftFallbackLogger {
    sender: SyncSender<FallbackEvent>,
}

impl GraftFallbackLogger {
    pub(crate) fn new(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::sync_channel(GRAFT_FALLBACK_QUEUE);
        let _worker = thread::Builder::new()
            .name("atm-graft-fallback-log".to_owned())
            .spawn(move || worker(receiver, path));
        // If the process has exhausted its thread resources, the sender is
        // disconnected and record() reports a structured failure.
        Self { sender }
    }

    pub(crate) fn record(
        &self,
        code: &'static str,
        fields: impl IntoIterator<Item = (&'static str, String)>,
    ) -> ObservabilityStatus {
        let mut fields: Vec<_> = fields
            .into_iter()
            .filter(|(key, _)| *key != "code" && RETAINED_FIELD_ALLOWLIST.contains(key))
            .collect();
        fields.insert(0, ("code", code.to_owned()));
        let (result, response) = mpsc::sync_channel(0);
        let event = FallbackEvent { fields, result };
        if let Err(error) = self.sender.try_send(event) {
            return ObservabilityStatus::failed(match error {
                TrySendError::Full(_) => "ATM_GRAFT_FALLBACK_QUEUE_FULL",
                TrySendError::Disconnected(_) => "ATM_GRAFT_FALLBACK_WORKER_STOPPED",
            });
        }
        match response.recv_timeout(WORKER_RESPONSE_TIMEOUT) {
            Ok(Ok(())) => ObservabilityStatus::default(),
            Ok(Err(_)) | Err(_) => ObservabilityStatus::failed("ATM_GRAFT_FALLBACK_WRITE_FAILED"),
        }
    }
}

fn worker(receiver: Receiver<FallbackEvent>, path: PathBuf) {
    for event in receiver {
        let result = append_event(&path, &event.fields);
        let _ = event.result.send(result);
    }
}

fn append_event(path: &Path, fields: &[(&'static str, String)]) -> Result<(), String> {
    let line = render_event(fields);
    let parent = path
        .parent()
        .ok_or_else(|| "fallback log path has no parent".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    rotate_if_needed(path, line.len() as u64).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(line.as_bytes())
        .and_then(|_| file.flush())
        .map_err(|error| error.to_string())
}

fn rotate_if_needed(path: &Path, incoming_bytes: u64) -> std::io::Result<()> {
    let current_bytes = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if current_bytes == 0
        || current_bytes.saturating_add(incoming_bytes) <= GRAFT_FALLBACK_MAX_BYTES
    {
        return Ok(());
    }
    for index in (1..GRAFT_FALLBACK_KEEP_FILES - 1).rev() {
        let source = rotated_path(path, index);
        let target = rotated_path(path, index + 1);
        if source.exists() {
            let _ = fs::remove_file(&target);
            fs::rename(source, target)?;
        }
    }
    let first = rotated_path(path, 1);
    if path.exists() {
        let _ = fs::remove_file(&first);
        fs::rename(path, first)?;
    }
    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.{}", path.display(), index))
}

fn render_event(fields: &[(&'static str, String)]) -> String {
    let mut output = String::from("{");
    let mut first = true;
    let mut append = |key: &str, value: &str| {
        if !first {
            output.push(',');
        }
        first = false;
        output.push('"');
        escape_into(&mut output, key);
        output.push_str("\":\"");
        escape_into(&mut output, value);
        output.push('"');
    };
    append("origin", "graft");
    append("ts", &unix_millis().to_string());
    append("level", "warn");
    for (key, value) in fields {
        append(key, value);
    }
    output.push_str("}\n");
    output
}

fn escape_into(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => output.push(character),
        }
    }
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn fallback_path(log_dir: &Path) -> PathBuf {
    graft_fallback_log_path(log_dir)
}

pub(crate) fn correlation_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn new_logger(log_dir: &Path) -> Arc<GraftFallbackLogger> {
    Arc::new(GraftFallbackLogger::new(fallback_path(log_dir)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    use super::{GRAFT_FALLBACK_KEEP_FILES, GRAFT_FALLBACK_MAX_BYTES, GraftFallbackLogger};

    #[test]
    fn writes_allowlisted_origin_and_escapes_values() {
        let tempdir = TempDir::new().expect("tempdir");
        let logger = GraftFallbackLogger::new(tempdir.path().join("atm-graft-fallback.jsonl"));
        let status = logger.record(
            "ATM_GRAFT_DAEMON_UNAVAILABLE",
            [
                ("code", "ATM_GRAFT_DAEMON_UNAVAILABLE".to_owned()),
                ("detail", "safe \"detail\"".to_owned()),
                ("body", "must not appear".to_owned()),
            ],
        );
        assert_eq!(status, super::ObservabilityStatus::default());
        let text =
            fs::read_to_string(tempdir.path().join("atm-graft-fallback.jsonl")).expect("log");
        assert!(text.contains("\"origin\":\"graft\""));
        assert!(text.contains("safe \\\"detail\\\""));
        assert!(!text.contains("must not appear"));
    }

    #[test]
    fn rotates_only_the_satellite_file() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("atm-graft-fallback.jsonl");
        let logger = GraftFallbackLogger::new(path.clone());
        let detail = "x".repeat(GRAFT_FALLBACK_MAX_BYTES as usize / 2);
        for _ in 0..8 {
            let _ = logger.record("TEST", [("detail", detail.clone())]);
        }
        let files = (0..GRAFT_FALLBACK_KEEP_FILES)
            .map(|index| {
                if index == 0 {
                    path.clone()
                } else {
                    PathBuf::from(format!("{}.{}", path.display(), index))
                }
            })
            .filter(|path| path.exists())
            .collect::<Vec<_>>();
        assert!(files.len() <= GRAFT_FALLBACK_KEEP_FILES);
        assert!(!tempdir.path().join("atm.log.jsonl").exists());
    }
}
