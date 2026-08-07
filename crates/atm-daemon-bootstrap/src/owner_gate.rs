//! Minimal process-lifetime singleton guard for the replacement daemon.
//!
//! The operating system releases an advisory file lock when its owning process
//! exits. That lets the Tokio replacement avoid the frozen daemon's polling
//! stale-owner recovery thread while retaining the owner-record schema that
//! binds `local-http.json` to the active daemon instance.

use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;

use atm_core::error::AtmError;
use fs2::FileExt;
use ulid::Ulid;

#[derive(Debug)]
pub struct DaemonOwnerGuard {
    lock_file: File,
    lock_path: PathBuf,
    instance_id: Ulid,
}

impl DaemonOwnerGuard {
    /// Acquires the OS-user-scoped owner lock before a listener can bind.
    pub fn acquire_at(lock_path: PathBuf) -> Result<Self, AtmError> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                AtmError::daemon_unavailable("failed to create daemon owner-lock directory")
                    .with_cause(source)
            })?;
        }
        let mut lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to open daemon owner lock").with_cause(source)
            })?;
        lock_file.try_lock_exclusive().map_err(|source| {
            AtmError::daemon_serving_state_rejected(format!(
                "an ATM daemon already owns {}: {source}",
                lock_path.display()
            ))
        })?;
        let instance_id = Ulid::new();
        let token = Ulid::new();
        let record = format!("{}:{token}:{instance_id}\n", std::process::id());
        lock_file.set_len(0).map_err(|source| {
            AtmError::daemon_unavailable("failed to reset daemon owner record").with_cause(source)
        })?;
        lock_file.seek(SeekFrom::Start(0)).map_err(|source| {
            AtmError::daemon_unavailable("failed to seek daemon owner record").with_cause(source)
        })?;
        lock_file.write_all(record.as_bytes()).map_err(|source| {
            AtmError::daemon_unavailable("failed to write daemon owner record").with_cause(source)
        })?;
        lock_file.sync_data().map_err(|source| {
            AtmError::daemon_unavailable("failed to commit daemon owner record").with_cause(source)
        })?;
        Ok(Self {
            lock_file,
            lock_path,
            instance_id,
        })
    }

    #[must_use]
    pub const fn instance_id(&self) -> Ulid {
        self.instance_id
    }
}

impl Drop for DaemonOwnerGuard {
    fn drop(&mut self) {
        let _ = self.lock_file.set_len(0);
        let _ = self.lock_file.seek(SeekFrom::Start(0));
        let _ = self.lock_file.sync_data();
        let _ = self.lock_file.unlock();
        let _ = &self.lock_path;
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};

    use super::DaemonOwnerGuard;

    #[test]
    fn owner_record_uses_the_local_http_instance_schema() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let lock = temporary_directory.path().join("owner.lock");
        let mut guard = DaemonOwnerGuard::acquire_at(lock).expect("acquire owner");
        guard
            .lock_file
            .seek(SeekFrom::Start(0))
            .expect("seek owner record");
        let mut record = String::new();
        guard
            .lock_file
            .read_to_string(&mut record)
            .expect("read owner record through the owner handle");
        assert!(record.contains(&guard.instance_id().to_string()));
        assert_eq!(record.trim().split(':').count(), 3);
    }

    #[test]
    fn a_second_replacement_owner_cannot_acquire_the_live_lock() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let lock = temporary_directory.path().join("owner.lock");
        let _first = DaemonOwnerGuard::acquire_at(lock.clone()).expect("first owner acquires");
        let error = DaemonOwnerGuard::acquire_at(lock).expect_err("second owner is rejected");
        assert_eq!(error.code().as_str(), "ATM_DAEMON_SERVING_STATE_REJECTED");
    }
}
