//! Mailbox owner-layer write boundaries for the Claude-owned inbox surface.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AtmError;
use crate::mailbox::atomic;
use crate::mailbox::lock;
use crate::mailbox::source::{
    SourceFile, discover_source_paths, load_source_files, rediscover_and_validate_source_paths,
};
use crate::schema::MessageEnvelope;

#[derive(Debug)]
pub(crate) struct SourceMutation<T> {
    pub output: T,
    pub changed: bool,
}

/// Commit one mailbox file through the mailbox-layer write boundary.
///
/// The mailbox layer owns writes to the Claude-owned inbox compatibility
/// surface. Callers should express mailbox intent here instead of reaching
/// down to low-level atomic replacement directly.
pub(crate) fn commit_mailbox_state(
    path: &Path,
    messages: &[MessageEnvelope],
) -> Result<(), AtmError> {
    atomic::write_messages(path, messages)
}

/// Commit one already-loaded multi-source mailbox set through the mailbox layer.
pub(crate) fn commit_source_files(source_files: &[SourceFile]) -> Result<(), AtmError> {
    for source in source_files {
        commit_mailbox_state(&source.path, &source.messages)?;
    }
    Ok(())
}

/// Load the current mailbox source set without taking any mailbox locks.
pub(crate) fn observe_source_files(
    home_dir: &Path,
    team: &str,
    agent: &str,
) -> Result<Vec<SourceFile>, AtmError> {
    let source_paths = discover_source_paths(home_dir, team, agent)?;
    load_source_files(&source_paths)
}

/// Reload one mailbox source set under the deterministic mailbox lock plan and
/// commit only if the mutation closure reports a change.
pub(crate) fn commit_source_mutation<T, I, F>(
    home_dir: &Path,
    team: &str,
    agent: &str,
    extra_write_paths: I,
    timeout: Duration,
    mutate: F,
) -> Result<T, AtmError>
where
    I: IntoIterator<Item = PathBuf>,
    F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<SourceMutation<T>, AtmError>,
{
    let source_paths = discover_source_paths(home_dir, team, agent)?;
    let mut write_paths = source_paths.clone();
    write_paths.extend(extra_write_paths);
    let _locks = lock::acquire_many_sorted(write_paths, timeout)?;
    let source_paths = rediscover_and_validate_source_paths(&source_paths, home_dir, team, agent)?;
    #[cfg(test)]
    maybe_remove_locked_source_file_for_test(&source_paths)?;
    let mut source_files = load_source_files(&source_paths)?;
    let mutation = mutate(&source_paths, &mut source_files)?;
    if mutation.changed {
        commit_source_files(&source_files)?;
    }
    Ok(mutation.output)
}

#[cfg(test)]
fn maybe_remove_locked_source_file_for_test(source_paths: &[PathBuf]) -> Result<(), AtmError> {
    if std::env::var_os("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD").is_none() {
        return Ok(());
    }

    let Some(path) = source_paths.first() else {
        return Ok(());
    };

    std::fs::remove_file(path).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to remove locked inbox {} during test injection: {error}",
            path.display()
        ))
        .with_recovery(
            "Clear ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD or restore the missing inbox file before retrying the injected test path.",
        )
        .with_source(error)
    })
}
