use atm_core::boundary::{ReconcileRequest, ReplaySource, RosterStore, WatchEventBatch};
use atm_core::error::AtmError;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(crate) const MAX_PROJECTION_WRITE_JOURNAL_ENTRIES: usize = 256;

#[derive(Clone, Default)]
pub(crate) struct ProjectionWriteJournal {
    inner: Arc<Mutex<ProjectionWriteJournalState>>,
}

#[derive(Default)]
struct ProjectionWriteJournalState {
    entries: HashMap<ProjectionWriteJournalKey, usize>,
    order: VecDeque<ProjectionWriteJournalKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProjectionWriteJournalKey {
    path: PathBuf,
    digest: u64,
}

impl ProjectionWriteJournal {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records one daemon-authored Claude config projection write. External
    /// CLI/team-admin writes are intentionally treated as ordinary idempotent
    /// ingress events and do not record suppression entries here.
    #[allow(dead_code)]
    pub(crate) fn remember_projected_config_write(
        &self,
        path: &Path,
        digest: u64,
    ) -> Result<(), AtmError> {
        let key = ProjectionWriteJournalKey {
            path: canonical_projection_path(path),
            digest,
        };
        let mut state = self.inner.lock().map_err(|_| {
            AtmError::daemon_unavailable("reconcile projection-write journal lock poisoned")
                .with_recovery(
                    "Restart atm-daemon; the reconcile projection suppression journal can no longer be trusted.",
                )
        })?;
        if !state.entries.contains_key(&key) {
            evict_oldest_entry_if_full(&mut state);
            state.order.push_back(key.clone());
        }
        *state.entries.entry(key).or_insert(0) += 1;
        Ok(())
    }

    pub(crate) fn consume_projected_config_write(
        &self,
        path: &Path,
        digest: u64,
    ) -> Result<bool, AtmError> {
        let key = ProjectionWriteJournalKey {
            path: canonical_projection_path(path),
            digest,
        };
        let mut state = self.inner.lock().map_err(|_| {
            AtmError::daemon_unavailable("reconcile projection-write journal lock poisoned")
                .with_recovery(
                    "Restart atm-daemon; the reconcile projection suppression journal can no longer be trusted.",
                )
        })?;
        let Some(count) = state.entries.get_mut(&key) else {
            return Ok(false);
        };
        if *count <= 1 {
            state.entries.remove(&key);
        } else {
            *count -= 1;
        }
        Ok(true)
    }
}

pub(crate) fn config_document_digest(path: &Path) -> Result<u64, AtmError> {
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
    Ok(stable_projection_digest(&bytes))
}

pub(crate) fn ingest_claude_team_config_from_watch_batch(
    request: &ReconcileRequest,
    batch: &WatchEventBatch,
    roster_store: &dyn RosterStore,
    projection_write_journal: &ProjectionWriteJournal,
) -> Result<(), AtmError> {
    let team_dir = atm_core::home::team_dir_from_home(&request.home_dir, &request.team).map_err(
        |error| {
            AtmError::daemon_unavailable(format!(
                "reconcile runtime could not resolve team {} from {} for Claude config ingest",
                request.team,
                request.home_dir.display()
            ))
            .with_recovery(
                "Verify the ATM home directory and Claude team layout before retrying reconcile ingest.",
            )
            .with_source(error)
        },
    )?;
    let config_path = team_dir.join("config.json");
    if !batch.paths.iter().any(|path| path == &config_path) || !config_path.is_file() {
        return Ok(());
    }

    let digest = config_document_digest(&config_path)?;
    if projection_write_journal.consume_projected_config_write(&config_path, digest)? {
        return Ok(());
    }

    let team_config = atm_core::load_claude_team_config_document(&team_dir).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "reconcile runtime could not load Claude team config from {}",
            config_path.display()
        ))
        .with_recovery(
            "Repair the Claude team config document before retrying watcher-owned roster ingest.",
        )
        .with_source(error)
    })?;
    let members = team_config
        .members
        .into_iter()
        .map(|member| {
            atm_core::boundary::roster_member_record_from_claude_code_member(
                request.team.clone(),
                member,
            )
        })
        .collect::<Vec<_>>();
    roster_store
        .replace_roster(
            &request.team,
            &members,
            Some(&replay_source_static("watcher-config-ingress")),
        )
        .map_err(|error| {
            AtmError::daemon_unavailable(format!(
                "reconcile runtime could not replace canonical ATM roster state from {}",
                config_path.display()
            ))
            .with_recovery(
                "Repair the ATM roster store or Claude config document before retrying watcher-owned ingest.",
            )
            .with_source(error)
        })?;
    Ok(())
}

fn canonical_projection_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn evict_oldest_entry_if_full(state: &mut ProjectionWriteJournalState) {
    while state.entries.len() >= MAX_PROJECTION_WRITE_JOURNAL_ENTRIES {
        let Some(oldest) = state.order.pop_front() else {
            return;
        };
        if state.entries.remove(&oldest).is_some() {
            return;
        }
    }
}

fn replay_source_static(label: &'static str) -> ReplaySource {
    ReplaySource::new(label).unwrap_or_else(|_| unreachable!("static replay source must validate"))
}

fn stable_projection_digest(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    bytes.iter().fold(FNV_OFFSET_BASIS, |digest, byte| {
        (digest ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

#[cfg(test)]
mod tests {
    use super::{MAX_PROJECTION_WRITE_JOURNAL_ENTRIES, ProjectionWriteJournal};
    use atm_core::error::AtmError;
    use std::path::PathBuf;

    fn projection_test_path(suffix: usize) -> PathBuf {
        std::env::temp_dir().join(format!("projection-{suffix}.json"))
    }

    fn remember_with_suffix(
        journal: &ProjectionWriteJournal,
        suffix: usize,
    ) -> Result<(), AtmError> {
        let path = projection_test_path(suffix);
        journal.remember_projected_config_write(&path, suffix as u64)
    }

    #[test]
    fn evicts_oldest_entry_at_capacity() {
        let journal = ProjectionWriteJournal::new();
        for index in 0..MAX_PROJECTION_WRITE_JOURNAL_ENTRIES {
            remember_with_suffix(&journal, index).expect("seed journal");
        }

        remember_with_suffix(&journal, MAX_PROJECTION_WRITE_JOURNAL_ENTRIES).expect("evict oldest");

        assert!(
            !journal
                .consume_projected_config_write(projection_test_path(0).as_path(), 0)
                .expect("consume oldest"),
            "oldest entry should have been evicted once the journal hit its max"
        );
        assert!(
            journal
                .consume_projected_config_write(
                    projection_test_path(MAX_PROJECTION_WRITE_JOURNAL_ENTRIES).as_path(),
                    MAX_PROJECTION_WRITE_JOURNAL_ENTRIES as u64,
                )
                .expect("consume newest"),
            "newest entry should remain present after eviction"
        );
    }
}
