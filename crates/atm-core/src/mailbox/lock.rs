use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;

use crate::error::AtmError;

pub(crate) const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub(crate) struct MailboxLockGuard {
    #[allow(dead_code)]
    target_path: PathBuf,
    #[allow(dead_code)]
    lock_path: PathBuf,
    file: File,
}

impl Drop for MailboxLockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub(crate) fn sentinel_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".lock");
    PathBuf::from(os)
}

pub(crate) fn sort_unique_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_by_key(|path| path.to_string_lossy().into_owned());
    paths.dedup_by(|left, right| left == right);
    paths
}

pub(crate) fn acquire(path: &Path, timeout: Duration) -> Result<MailboxLockGuard, AtmError> {
    let lock_path = sentinel_path(path);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::mailbox_lock(format!(
                "failed to create mailbox lock directory {}: {error}",
                parent.display()
            ))
            .with_source(error)
        })?;
    }

    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| {
            AtmError::mailbox_lock(format!(
                "failed to open mailbox lock {}: {error}",
                lock_path.display()
            ))
            .with_source(error)
        })?;

    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => {
                return Ok(MailboxLockGuard {
                    target_path: path.to_path_buf(),
                    lock_path,
                    file,
                });
            }
            Err(error) if Instant::now() >= deadline => {
                return Err(AtmError::mailbox_lock_timeout(path).with_source(error));
            }
            Err(_) => {
                thread::sleep(RETRY_INTERVAL);
            }
        }
    }
}

pub(crate) fn acquire_many_sorted(
    paths: impl IntoIterator<Item = PathBuf>,
    timeout: Duration,
) -> Result<Vec<MailboxLockGuard>, AtmError> {
    let paths = sort_unique_paths(paths);
    let deadline = Instant::now() + timeout;
    let mut guards = Vec::with_capacity(paths.len());

    for path in paths {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AtmError::mailbox_lock_timeout(&path));
        }
        guards.push(acquire(&path, remaining)?);
    }

    Ok(guards)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{DEFAULT_LOCK_TIMEOUT, acquire, acquire_many_sorted, sentinel_path};

    #[test]
    fn sentinel_path_appends_lock_suffix() {
        let path = PathBuf::from("team-lead.json");
        assert_eq!(sentinel_path(&path), PathBuf::from("team-lead.json.lock"));
    }

    #[test]
    fn acquire_creates_sentinel_file() {
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");

        let _guard = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock");

        assert!(sentinel_path(&inbox).exists());
    }

    #[test]
    fn acquire_many_sorted_dedupes_and_sorts_paths() {
        let tempdir = tempdir().expect("tempdir");
        let a = tempdir.path().join("b.json");
        let b = tempdir.path().join("a.json");

        let guards =
            acquire_many_sorted(vec![a.clone(), b.clone(), a.clone()], DEFAULT_LOCK_TIMEOUT)
                .expect("locks");

        assert_eq!(guards.len(), 2);
    }

    use std::path::PathBuf;
}
