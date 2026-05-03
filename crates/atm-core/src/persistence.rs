use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{AtmError, AtmErrorKind};
use uuid::Uuid;

/// Atomically replace one shared mutable ATM-owned state file.
///
/// ATM always fsyncs the temporary file before the final rename. On Linux and
/// macOS, ATM also fsyncs the parent directory after the rename so a successful
/// return means both the contents and the directory entry update were durably
/// published as far as the host platform can guarantee. On Windows, the Rust
/// standard library does not expose a portable directory-sync operation, so the
/// helper returns `Ok(())` after the temp-file fsync plus rename without an
/// additional parent-directory sync.
pub(crate) fn atomic_write_bytes(
    path: &Path,
    bytes: &[u8],
    kind: AtmErrorKind,
    label: &str,
    recovery: &str,
) -> Result<(), AtmError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::new(
                kind,
                format!(
                    "failed to create parent directory {}: {error}",
                    parent.display()
                ),
            )
            .with_source(error)
            .with_recovery(recovery)
        })?;
    }

    let temp_path = temp_path_for_atomic_write(path, label);

    {
        let mut file = File::create(&temp_path).map_err(|error| {
            AtmError::new(
                kind,
                format!(
                    "failed to create {label} temp file {}: {error}",
                    temp_path.display()
                ),
            )
            .with_source(error)
            .with_recovery(recovery)
        })?;
        file.write_all(bytes).map_err(|error| {
            AtmError::new(
                kind,
                format!(
                    "failed to write {label} temp file {}: {error}",
                    temp_path.display()
                ),
            )
            .with_source(error)
            .with_recovery(recovery)
        })?;
        file.sync_all().map_err(|error| {
            AtmError::new(
                kind,
                format!(
                    "failed to sync {label} temp file {}: {error}",
                    temp_path.display()
                ),
            )
            .with_source(error)
            .with_recovery(recovery)
        })?;
    }

    fs::rename(&temp_path, path).map_err(|error| {
        AtmError::new(
            kind,
            format!("failed to replace {}: {error}", path.display()),
        )
        .with_source(error)
        .with_recovery(recovery)
    })?;
    sync_parent_directory(path, kind, label, recovery)?;
    Ok(())
}

fn temp_path_for_atomic_write(path: &Path, label: &str) -> PathBuf {
    path.with_file_name(format!(
        ".{}.tmp.{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(label),
        std::process::id(),
        Uuid::new_v4()
    ))
}

pub(crate) fn atomic_write_string(
    path: &Path,
    contents: &str,
    kind: AtmErrorKind,
    label: &str,
    recovery: &str,
) -> Result<(), AtmError> {
    atomic_write_bytes(path, contents.as_bytes(), kind, label, recovery)
}

#[cfg(unix)]
fn sync_parent_directory(
    path: &Path,
    kind: AtmErrorKind,
    label: &str,
    recovery: &str,
) -> Result<(), AtmError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    #[cfg(test)]
    if tests::forced_parent_sync_failure() {
        return Err(
            AtmError::new(
                kind,
                format!(
                    "failed to sync parent directory {} after replacing {}: synthetic parent-directory sync failure",
                    parent.display(),
                    path.display()
                ),
            )
            .with_source(std::io::Error::other("synthetic parent-directory sync failure"))
            .with_recovery(recovery),
        );
    }

    let directory = File::open(parent).map_err(|error| {
        AtmError::new(
            kind,
            format!(
                "failed to open parent directory {} for {} durability sync: {error}",
                parent.display(),
                label
            ),
        )
        .with_source(error)
        .with_recovery(recovery)
    })?;
    directory.sync_all().map_err(|error| {
        AtmError::new(
            kind,
            format!(
                "failed to sync parent directory {} after replacing {}: {error}",
                parent.display(),
                path.display()
            ),
        )
        .with_source(error)
        .with_recovery(recovery)
    })
}

#[cfg(not(unix))]
fn sync_parent_directory(
    _path: &Path,
    _kind: AtmErrorKind,
    _label: &str,
    _recovery: &str,
) -> Result<(), AtmError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::{Mutex, OnceLock};

    use serial_test::serial;
    use tempfile::tempdir;

    use super::{atomic_write_bytes, temp_path_for_atomic_write};
    use crate::error::AtmErrorKind;

    // Serializes process-environment mutation inside this test module. This is
    // process-local only; it does not coordinate with other test processes.
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    thread_local! {
        static FORCE_PARENT_SYNC_FAILURE: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn forced_parent_sync_failure() -> bool {
        FORCE_PARENT_SYNC_FAILURE.with(Cell::get)
    }

    struct ParentSyncFailureGuard;

    impl ParentSyncFailureGuard {
        fn enable() -> Self {
            FORCE_PARENT_SYNC_FAILURE.with(|value| value.set(true));
            Self
        }
    }

    impl Drop for ParentSyncFailureGuard {
        fn drop(&mut self) {
            FORCE_PARENT_SYNC_FAILURE.with(|value| value.set(false));
        }
    }

    #[test]
    fn atomic_write_bytes_replaces_existing_contents() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join("state.json");

        atomic_write_bytes(
            &path,
            br#"{"value":1}"#,
            AtmErrorKind::MailboxWrite,
            "state file",
            "retry after fixing the state file path",
        )
        .expect("first write");
        atomic_write_bytes(
            &path,
            br#"{"value":2}"#,
            AtmErrorKind::MailboxWrite,
            "state file",
            "retry after fixing the state file path",
        )
        .expect("second write");

        assert_eq!(
            std::fs::read_to_string(&path).expect("state file"),
            r#"{"value":2}"#
        );
    }

    #[test]
    fn atomic_write_temp_paths_are_unique_across_rapid_writes() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join("state.json");

        let first = temp_path_for_atomic_write(&path, "state file");
        let second = temp_path_for_atomic_write(&path, "state file");

        assert_ne!(first, second);
        assert!(
            first
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".tmp.")
        );
        assert!(
            second
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".tmp.")
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn atomic_write_bytes_reports_parent_sync_failure_via_deterministic_hook() {
        let _env_lock = env_lock().lock().expect("env lock");
        let _fault = ParentSyncFailureGuard::enable();
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join("state.json");

        let error = atomic_write_bytes(
            &path,
            br#"{"value":1}"#,
            AtmErrorKind::MailboxWrite,
            "state file",
            "retry after fixing the state file path",
        )
        .expect_err("parent sync failure");

        assert!(error.is_mailbox_write());
        assert!(error.message.contains("failed to sync parent directory"));
    }

    #[cfg(not(unix))]
    #[test]
    fn atomic_write_bytes_succeeds_without_parent_directory_sync() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join("state.json");

        atomic_write_bytes(
            &path,
            br#"{"value":1}"#,
            AtmErrorKind::MailboxWrite,
            "state file",
            "retry after fixing the state file path",
        )
        .expect("write without parent sync");

        assert_eq!(
            std::fs::read_to_string(&path).expect("state file"),
            r#"{"value":1}"#
        );
    }
}
