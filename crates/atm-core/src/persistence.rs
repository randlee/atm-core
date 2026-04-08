use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use chrono::Utc;

use crate::error::{AtmError, AtmErrorKind};

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

    let temp_path = path.with_file_name(format!(
        ".{}.tmp.{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(label),
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));

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
    Ok(())
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
