use super::MAX_PROJECTION_WRITE_JOURNAL_ENTRIES;
use atm_core::error::AtmError;
use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(super) type ProjectionWriteJournal = Arc<Mutex<HashMap<ProjectionWriteJournalKey, usize>>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ProjectionWriteJournalKey {
    path: PathBuf,
    digest: u64,
}

pub(super) fn config_document_digest(path: &Path) -> Result<u64, AtmError> {
    let bytes = fs::read(path).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "reconcile runtime could not read Claude config digest from {}",
            path.display()
        ))
        .with_recovery(
            "Verify the Claude team config path remains readable before retrying watcher-owned ingest.",
        )
        .with_source(error)
    })?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(hasher.finish())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn remember_projected_config_write(
    journal: &ProjectionWriteJournal,
    path: &Path,
    digest: u64,
) -> Result<(), AtmError> {
    let key = ProjectionWriteJournalKey {
        path: canonical_projection_path(path),
        digest,
    };
    let mut entries = journal.lock().map_err(|_| {
        AtmError::daemon_unavailable("reconcile projection-write journal lock poisoned")
            .with_recovery(
                "Restart atm-daemon; the reconcile projection suppression journal can no longer be trusted.",
            )
    })?;
    if entries.len() >= MAX_PROJECTION_WRITE_JOURNAL_ENTRIES && !entries.contains_key(&key) {
        return Ok(());
    }
    *entries.entry(key).or_insert(0) += 1;
    Ok(())
}

pub(super) fn consume_projected_config_write(
    journal: &ProjectionWriteJournal,
    path: &Path,
    digest: u64,
) -> Result<bool, AtmError> {
    let key = ProjectionWriteJournalKey {
        path: canonical_projection_path(path),
        digest,
    };
    let mut entries = journal.lock().map_err(|_| {
        AtmError::daemon_unavailable("reconcile projection-write journal lock poisoned")
            .with_recovery(
                "Restart atm-daemon; the reconcile projection suppression journal can no longer be trusted.",
            )
    })?;
    let Some(count) = entries.get_mut(&key) else {
        return Ok(false);
    };
    if *count <= 1 {
        entries.remove(&key);
    } else {
        *count -= 1;
    }
    Ok(true)
}

fn canonical_projection_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
