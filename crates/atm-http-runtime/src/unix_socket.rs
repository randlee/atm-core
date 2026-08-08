//! Unix-domain-socket listener setup and owner-safe cleanup.
//!
//! This physical adapter is deliberately separate from runtime composition:
//! it validates, stages, publishes, and tears down only the UDS endpoint.

use std::path::{Path, PathBuf};

use atm_core::error::AtmError;
#[cfg(unix)]
use fs2::FileExt;
#[cfg(unix)]
use tokio::net::UnixListener;

use super::UnixSocketConfig;

#[cfg(unix)]
const SOCKET_LIVENESS_PROBE_ATTEMPTS: usize = 3;
#[cfg(unix)]
const SOCKET_LIVENESS_PROBE_BACKOFF: std::time::Duration = std::time::Duration::from_millis(10);

/// A same-user process lock that makes UDS recovery and publication one
/// critical section. The socket pathname alone cannot provide that guarantee:
/// a second daemon could otherwise observe stale state between recovery and
/// publication.
#[cfg(unix)]
#[derive(Debug)]
pub(super) struct UnixSocketStartupLock {
    file: std::fs::File,
}

#[cfg(unix)]
impl UnixSocketStartupLock {
    pub(super) fn acquire(socket: &UnixSocketConfig) -> Result<Self, AtmError> {
        use std::fs::{self, OpenOptions};
        use std::os::unix::fs::PermissionsExt;

        let parent = validate_unix_socket_parent(socket)?;
        let name = socket
            .path
            .file_name()
            .ok_or_else(|| AtmError::config("Unix HTTP socket path must name a socket file"))?;
        let lock_path = parent.join(format!(".{}.startup.lock", name.to_string_lossy()));
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to open Unix HTTP socket startup lock")
                    .with_cause(source)
            })?;
        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            AtmError::daemon_unavailable("failed to protect Unix HTTP socket startup lock")
                .with_cause(source)
        })?;
        file.try_lock_exclusive().map_err(|source| {
            AtmError::daemon_serving_state_rejected(format!(
                "another daemon is starting the Unix HTTP socket `{}`: {source}",
                socket.path.display()
            ))
        })?;
        Ok(Self { file })
    }
}

#[cfg(unix)]
impl Drop for UnixSocketStartupLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(unix)]
pub(super) fn bind_unix_listener(
    socket: &UnixSocketConfig,
) -> Result<(UnixListener, UnixSocketPathGuard), AtmError> {
    let parent = validate_unix_socket_parent(socket)?;
    let (listener, staging) = bind_prepared_unix_socket(socket, parent)?;
    publish_prepared_unix_socket(socket, staging)?;
    let cleanup = UnixSocketPathGuard::capture(&socket.path).inspect_err(|_| {
        let _ = std::fs::remove_file(&socket.path);
    })?;
    Ok((listener, cleanup))
}

#[cfg(unix)]
fn validate_unix_socket_parent(socket: &UnixSocketConfig) -> Result<&Path, AtmError> {
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    let parent = socket
        .path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            AtmError::config("Unix HTTP socket path must have an owner-controlled parent")
        })?;
    let metadata = fs::metadata(parent).map_err(|source| {
        AtmError::config("cannot inspect Unix HTTP socket parent directory").with_cause(source)
    })?;
    if metadata.uid() != socket.owner_uid.get() {
        return Err(
            AtmError::config("Unix HTTP socket parent owner does not match configuration")
                .with_cause(format!(
                    "configured uid {} but parent `{}` is owned by uid {}",
                    socket.owner_uid.get(),
                    parent.display(),
                    metadata.uid()
                )),
        );
    }
    if metadata.mode() & 0o022 != 0 {
        return Err(
            AtmError::config("Unix HTTP socket parent must not be writable by others").with_cause(
                format!(
                    "parent `{}` mode {:o} permits group or other writes",
                    parent.display(),
                    metadata.mode() & 0o777
                ),
            ),
        );
    }
    Ok(parent)
}

#[cfg(unix)]
/// Removes the prior runtime's dead socket pathname, but never replaces a
/// reachable endpoint. A supervisor can restart the replacement daemon after
/// an ungraceful exit, which bypasses `UnixSocketPathGuard::drop`; without
/// this narrowly checked recovery the new Tokio runtime cannot rebind.
pub(super) async fn reclaim_stale_unix_socket(socket: &UnixSocketConfig) -> Result<(), AtmError> {
    validate_unix_socket_parent(socket)?;
    use std::fs;
    use std::io::ErrorKind;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let path = &socket.path;
    let original = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(AtmError::daemon_stale_owner_recovery_failed(
                "cannot inspect Unix HTTP socket path during stale-owner recovery",
            )
            .with_cause(source));
        }
    };
    if !original.file_type().is_socket() {
        return Err(AtmError::daemon_stale_owner_recovery_failed(
            "Unix HTTP socket path cannot be recovered because it is not a socket",
        )
        .with_cause(format!(
            "refusing to replace non-socket path `{}`",
            path.display()
        )));
    }
    if original.uid() != socket.owner_uid.get() {
        return Err(AtmError::daemon_stale_owner_recovery_failed(
            "Unix HTTP socket path cannot be recovered because its owner differs",
        )
        .with_cause(format!(
            "refusing to replace socket `{}` owned by uid {}",
            path.display(),
            original.uid()
        )));
    }
    match probe_unix_socket_liveness(path).await {
        Ok(SocketLiveness::Reachable) => Err(AtmError::config(
            "Unix HTTP socket path is already occupied",
        )
        .with_cause(format!(
            "refusing to replace reachable socket `{}`",
            path.display()
        ))),
        Ok(SocketLiveness::Dead) => {
            let current = fs::symlink_metadata(path).map_err(|source| {
                AtmError::daemon_stale_owner_recovery_failed(
                    "cannot re-inspect Unix HTTP socket path during stale-owner recovery",
                )
                .with_cause(source)
            })?;
            if !current.file_type().is_socket()
                || current.uid() != socket.owner_uid.get()
                || current.dev() != original.dev()
                || current.ino() != original.ino()
            {
                return Err(AtmError::daemon_stale_owner_recovery_failed(
                    "Unix HTTP socket path changed during stale-owner recovery",
                )
                .with_cause(format!(
                    "refusing to replace `{}` after identity changed",
                    path.display()
                )));
            }
            fs::remove_file(path).map_err(|source| {
                AtmError::daemon_stale_owner_recovery_failed(
                    "failed to remove stale Unix HTTP socket path",
                )
                .with_cause(source)
            })
        }
        Err(source) => Err(AtmError::daemon_stale_owner_recovery_failed(
            "cannot determine whether the Unix HTTP socket owner is stale",
        )
        .with_cause(format!(
            "refusing to replace socket `{}` after connection check failed: {source}",
            path.display()
        ))),
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketLiveness {
    Reachable,
    Dead,
}

#[cfg(unix)]
async fn probe_unix_socket_liveness(path: &Path) -> std::io::Result<SocketLiveness> {
    // A refused AF_UNIX connection can be transient while a live listener's
    // backlog drains, so one refusal is insufficient evidence for unlinking
    // its pathname. This remains entirely on the Tokio runtime.
    for attempt in 0..SOCKET_LIVENESS_PROBE_ATTEMPTS {
        match tokio::net::UnixStream::connect(path).await {
            Ok(_) => return Ok(SocketLiveness::Reachable),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                if attempt + 1 < SOCKET_LIVENESS_PROBE_ATTEMPTS {
                    tokio::time::sleep(SOCKET_LIVENESS_PROBE_BACKOFF).await;
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(SocketLiveness::Dead)
}

#[cfg(unix)]
fn bind_prepared_unix_socket(
    socket: &UnixSocketConfig,
    parent: &Path,
) -> Result<(UnixListener, PrivateStagingDirectory), AtmError> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // Bind below a newly-created `0700` staging directory. The socket cannot
    // be connected before its final owner-only mode is verified and atomically
    // published, without changing the process-global umask.
    let staging = PrivateStagingDirectory::create(parent)?;
    let staged_path = staging.path().join("listener.sock");
    let listener = UnixListener::bind(&staged_path).map_err(|source| {
        AtmError::daemon_unavailable("failed to bind replacement Unix HTTP socket")
            .with_cause(source)
    })?;
    fs::set_permissions(&staged_path, fs::Permissions::from_mode(socket.mode.get())).map_err(
        |source| {
            AtmError::daemon_unavailable("failed to set replacement Unix HTTP socket permissions")
                .with_cause(source)
        },
    )?;
    verify_prepared_unix_socket(socket, &staged_path)?;
    Ok((listener, staging))
}

#[cfg(unix)]
fn verify_prepared_unix_socket(socket: &UnixSocketConfig, path: &Path) -> Result<(), AtmError> {
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(path).map_err(|source| {
        AtmError::daemon_unavailable("failed to inspect replacement Unix HTTP socket permissions")
            .with_cause(source)
    })?;
    if metadata.uid() != socket.owner_uid.get() {
        return Err(AtmError::config(
            "replacement Unix HTTP socket owner does not match configuration",
        )
        .with_cause(format!(
            "configured uid {} but bound socket is owned by uid {}",
            socket.owner_uid.get(),
            metadata.uid()
        )));
    }
    if metadata.mode() & 0o777 != socket.mode.get() {
        return Err(AtmError::config(
            "replacement Unix HTTP socket permissions do not match configuration",
        )
        .with_cause(format!(
            "configured mode {:o} but bound socket mode is {:o}",
            socket.mode.get(),
            metadata.mode() & 0o777
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn publish_prepared_unix_socket(
    socket: &UnixSocketConfig,
    staging: PrivateStagingDirectory,
) -> Result<(), AtmError> {
    let staged_path = staging.path().join("listener.sock");
    std::fs::rename(&staged_path, &socket.path).map_err(|source| {
        AtmError::daemon_unavailable("failed to publish replacement Unix HTTP socket")
            .with_cause(source)
    })
}

/// Owner-checked, uniquely named staging directory used only until an already
/// permissioned UDS inode is atomically published at its configured path.
#[cfg(unix)]
#[derive(Debug)]
pub(super) struct PrivateStagingDirectory {
    path: PathBuf,
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl PrivateStagingDirectory {
    pub(super) fn create(parent: &Path) -> Result<Self, AtmError> {
        use std::fs;
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

        let (path, ()) = super::private_staging::allocate(parent, "uds", |path| {
            fs::DirBuilder::new().mode(0o700).create(path)
        })
        .map_err(|source| {
            AtmError::daemon_unavailable(
                "failed to create private Unix HTTP socket staging directory",
            )
            .with_cause(source)
        })?;
        if let Err(source) = fs::set_permissions(&path, fs::Permissions::from_mode(0o700)) {
            let _ = fs::remove_dir(&path);
            return Err(AtmError::daemon_unavailable(
                "failed to protect Unix HTTP socket staging directory",
            )
            .with_cause(source));
        }
        let metadata = fs::metadata(&path).map_err(|source| {
            let _ = fs::remove_dir(&path);
            AtmError::daemon_unavailable("failed to inspect Unix HTTP socket staging directory")
                .with_cause(source)
        })?;
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for PrivateStagingDirectory {
    fn drop(&mut self) {
        use std::fs;
        use std::os::unix::fs::MetadataExt;

        let is_our_directory = fs::metadata(&self.path)
            .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode);
        if is_our_directory {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Removes only the socket inode created by this runtime during shutdown.
#[cfg(unix)]
#[derive(Debug)]
pub(super) struct UnixSocketPathGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl UnixSocketPathGuard {
    fn capture(path: &Path) -> Result<Self, AtmError> {
        use std::fs;
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::metadata(path).map_err(|source| {
            AtmError::daemon_unavailable("failed to inspect bound Unix HTTP socket")
                .with_cause(source)
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

#[cfg(unix)]
impl Drop for UnixSocketPathGuard {
    fn drop(&mut self) {
        use std::fs;
        use std::os::unix::fs::MetadataExt;

        let is_our_socket = fs::metadata(&self.path)
            .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode);
        if is_our_socket {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::num::NonZeroU32;
    use std::os::unix::fs::MetadataExt;

    use super::{
        SocketLiveness, UnixSocketStartupLock, bind_unix_listener, probe_unix_socket_liveness,
        reclaim_stale_unix_socket,
    };
    use crate::{UnixSocketConfig, UnixSocketMode, UnixSocketOwnerUid};

    fn socket_config(path: std::path::PathBuf, uid: u32) -> UnixSocketConfig {
        UnixSocketConfig::new(
            path,
            UnixSocketOwnerUid::new(NonZeroU32::new(uid).expect("test uid is non-zero")),
            UnixSocketMode::new(NonZeroU32::new(0o600).expect("owner-only socket mode")),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn refused_socket_is_retried_before_it_is_declared_dead() {
        let directory = tempfile::tempdir().expect("temporary UDS parent");
        let path = directory.path().join("atm-daemon.sock");
        let stale = std::os::unix::net::UnixListener::bind(&path).expect("create stale socket");
        drop(stale);

        let probe = tokio::spawn(async move { probe_unix_socket_liveness(&path).await });
        tokio::task::yield_now().await;
        assert!(
            !probe.is_finished(),
            "one refusal cannot prove the socket is dead"
        );
        tokio::time::advance(std::time::Duration::from_millis(20)).await;
        assert_eq!(
            probe.await.expect("probe joins").expect("probe result"),
            SocketLiveness::Dead
        );
    }

    #[test]
    fn concurrent_socket_start_is_rejected_before_reclaim_or_publish() {
        let directory = tempfile::tempdir().expect("temporary UDS parent");
        let uid = std::fs::metadata(directory.path())
            .expect("temporary UDS parent metadata")
            .uid();
        let socket = socket_config(directory.path().join("atm-daemon.sock"), uid);
        let _first = UnixSocketStartupLock::acquire(&socket).expect("first startup lock");

        let error = UnixSocketStartupLock::acquire(&socket)
            .expect_err("second same-user start must not enter the recovery critical section");
        assert_eq!(error.code().as_str(), "ATM_DAEMON_SERVING_STATE_REJECTED");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dead_owner_socket_is_reclaimed_before_rebinding() {
        let directory = tempfile::tempdir().expect("temporary UDS parent");
        let path = directory.path().join("atm-daemon.sock");
        let uid = std::fs::metadata(directory.path())
            .expect("temporary UDS parent metadata")
            .uid();
        let stale = std::os::unix::net::UnixListener::bind(&path).expect("create stale socket");
        drop(stale);

        reclaim_stale_unix_socket(&socket_config(path.clone(), uid))
            .await
            .expect("reclaim stale socket");
        let (_listener, cleanup) =
            bind_unix_listener(&socket_config(path.clone(), uid)).expect("rebind replacement");
        assert!(
            path.exists(),
            "the replacement listener owns the published path"
        );
        drop(cleanup);
        assert!(
            !path.exists(),
            "the replacement listener cleans up its own path"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reachable_socket_is_never_replaced() {
        let directory = tempfile::tempdir().expect("temporary UDS parent");
        let path = directory.path().join("atm-daemon.sock");
        let uid = std::fs::metadata(directory.path())
            .expect("temporary UDS parent metadata")
            .uid();
        let _live = std::os::unix::net::UnixListener::bind(&path).expect("create live socket");

        let error = match reclaim_stale_unix_socket(&socket_config(path.clone(), uid)).await {
            Ok(_) => panic!("a live socket must remain in place"),
            Err(error) => error,
        };
        assert!(error.message().contains("already occupied"));
        assert!(path.exists(), "the live socket pathname is retained");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn non_socket_recovery_rejection_uses_stale_owner_error_code() {
        let directory = tempfile::tempdir().expect("temporary UDS parent");
        let path = directory.path().join("atm-daemon.sock");
        let uid = std::fs::metadata(directory.path())
            .expect("temporary UDS parent metadata")
            .uid();
        std::fs::write(&path, "must not be unlinked as a socket")
            .expect("create occupied non-socket path");

        let error = reclaim_stale_unix_socket(&socket_config(path.clone(), uid))
            .await
            .expect_err("non-socket path cannot be reclaimed");
        assert_eq!(
            error.code().as_str(),
            "ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED"
        );
        assert!(
            path.exists(),
            "safety rejection preserves the occupied path"
        );
    }
}
