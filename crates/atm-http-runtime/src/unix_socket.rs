//! Unix-domain-socket listener setup and owner-safe cleanup.
//!
//! This physical adapter is deliberately separate from runtime composition:
//! it validates, stages, publishes, and tears down only the UDS endpoint.

use std::path::{Path, PathBuf};

use atm_core::error::AtmError;
#[cfg(unix)]
use fs4::fs_std::FileExt;
#[cfg(unix)]
use tokio::net::UnixListener;

use super::UnixSocketConfig;

#[cfg(unix)]
const SOCKET_LIVENESS_PROBE_ATTEMPTS: usize = 5;
#[cfg(unix)]
const SOCKET_LIVENESS_PROBE_BACKOFF: std::time::Duration = std::time::Duration::from_millis(100);
#[cfg(unix)]
const ORPHAN_STAGING_MIN_AGE: std::time::Duration = std::time::Duration::from_secs(300);

// `sockaddr_un::sun_path` includes the trailing NUL required by pathname UDS
// addresses. Keep one byte in reserve and reject inputs before `bind` turns a
// deterministic configuration error into a platform-specific failure.
#[cfg(all(
    unix,
    any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )
))]
const UNIX_SOCKET_PATH_CAPACITY: usize = 104;
#[cfg(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
))]
const UNIX_SOCKET_PATH_CAPACITY: usize = 108;

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl FileIdentity {
    fn of(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;

        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    fn matches(self, metadata: &std::fs::Metadata) -> bool {
        self == Self::of(metadata)
    }
}

#[cfg(unix)]
fn is_owned_by(metadata: &std::fs::Metadata, owner_uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.uid() == owner_uid
}

/// Returns whether a Unix parent grants group or other users write access.
/// Each endpoint publisher owns its error contract, but this physical safety
/// decision has one implementation so UDS and loopback records cannot drift.
#[cfg(unix)]
pub(crate) fn parent_is_writable_by_others(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o022 != 0
}

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
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to open Unix HTTP socket startup lock")
                    .with_cause(source)
            })?;
        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            AtmError::daemon_unavailable("failed to protect Unix HTTP socket startup lock")
                .with_cause(source)
        })?;
        if !file.try_lock_exclusive().map_err(|source| {
            AtmError::daemon_serving_state_rejected(format!(
                "failed to acquire Unix HTTP socket startup lock `{}`: {source}",
                socket.path.display()
            ))
        })? {
            return Err(AtmError::daemon_serving_state_rejected(format!(
                "another daemon is starting the Unix HTTP socket `{}`",
                socket.path.display()
            )));
        }
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

    validate_unix_socket_path_length(&socket.path)?;
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
    if !is_owned_by(&metadata, socket.owner_uid.get()) {
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
    if parent_is_writable_by_others(&metadata) {
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
fn validate_unix_socket_path_length(path: &Path) -> Result<(), AtmError> {
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes().len();
    if bytes >= UNIX_SOCKET_PATH_CAPACITY {
        return Err(AtmError::config(
            "Unix HTTP socket path exceeds the platform sockaddr_un path limit",
        )
        .with_cause(format!(
            "path `{}` is {bytes} bytes; limit is {} bytes plus its terminating NUL",
            path.display(),
            UNIX_SOCKET_PATH_CAPACITY - 1
        )));
    }
    Ok(())
}

#[cfg(unix)]
/// Removes the prior runtime's dead socket pathname, but never replaces a
/// reachable endpoint. A supervisor can restart the replacement daemon after
/// an ungraceful exit, which bypasses `UnixSocketPathGuard::drop`; without
/// this narrowly checked recovery the new Tokio runtime cannot rebind.
pub(super) async fn reclaim_stale_unix_socket(socket: &UnixSocketConfig) -> Result<(), AtmError> {
    let original = inspect_recovery_target(socket.clone()).await?;
    let Some(original) = original else {
        return Ok(());
    };
    let path = socket.path.clone();
    match probe_unix_socket_liveness(&path, original).await {
        Ok(SocketLiveness::Reachable) => Err(AtmError::config(
            "Unix HTTP socket path is already occupied",
        )
        .with_cause(format!(
            "refusing to replace reachable socket `{}`",
            path.display()
        ))),
        Ok(SocketLiveness::Changed) => Err(AtmError::daemon_stale_owner_recovery_failed(
            "Unix HTTP socket path changed while liveness was being observed",
        )
        .with_cause(format!(
            "refusing to replace `{}` after its owner or file identity changed",
            path.display()
        ))),
        Ok(SocketLiveness::Dead) => {
            match recovery_target_observation(path.clone(), original).await? {
                RecoveryTargetObservation::Matches => {
                    remove_stale_socket_path(path, original).await
                }
                RecoveryTargetObservation::Missing => Ok(()),
                RecoveryTargetObservation::Changed => {
                    Err(AtmError::daemon_stale_owner_recovery_failed(
                        "Unix HTTP socket path changed during stale-owner recovery",
                    )
                    .with_cause(format!(
                        "refusing to replace `{}` after identity changed",
                        path.display()
                    )))
                }
            }
        }
        Ok(SocketLiveness::Disappeared) => Ok(()),
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
struct SocketRecoverySnapshot {
    identity: FileIdentity,
    owner_uid: u32,
}

#[cfg(unix)]
async fn inspect_recovery_target(
    socket: UnixSocketConfig,
) -> Result<Option<SocketRecoverySnapshot>, AtmError> {
    tokio::task::spawn_blocking(move || inspect_recovery_target_blocking(&socket))
        .await
        .map_err(|source| {
            AtmError::daemon_stale_owner_recovery_failed(
                "Unix HTTP socket stale-owner inspection task ended unexpectedly",
            )
            .with_cause(source)
        })?
}

#[cfg(unix)]
fn inspect_recovery_target_blocking(
    socket: &UnixSocketConfig,
) -> Result<Option<SocketRecoverySnapshot>, AtmError> {
    use std::fs;
    use std::io::ErrorKind;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    validate_unix_socket_parent(socket)?;
    let metadata = match fs::symlink_metadata(&socket.path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(AtmError::daemon_stale_owner_recovery_failed(
                "cannot inspect Unix HTTP socket path during stale-owner recovery",
            )
            .with_cause(source));
        }
    };
    if !metadata.file_type().is_socket() {
        return Err(AtmError::daemon_stale_owner_recovery_failed(
            "Unix HTTP socket path cannot be recovered because it is not a socket",
        )
        .with_cause(format!(
            "refusing to replace non-socket path `{}`",
            socket.path.display()
        )));
    }
    if !is_owned_by(&metadata, socket.owner_uid.get()) {
        return Err(AtmError::daemon_stale_owner_recovery_failed(
            "Unix HTTP socket path cannot be recovered because its owner differs",
        )
        .with_cause(format!(
            "refusing to replace socket `{}` owned by uid {}",
            socket.path.display(),
            metadata.uid()
        )));
    }
    Ok(Some(SocketRecoverySnapshot {
        identity: FileIdentity::of(&metadata),
        owner_uid: metadata.uid(),
    }))
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryTargetObservation {
    Matches,
    Missing,
    Changed,
}

#[cfg(unix)]
async fn recovery_target_observation(
    path: PathBuf,
    original: SocketRecoverySnapshot,
) -> Result<RecoveryTargetObservation, AtmError> {
    tokio::task::spawn_blocking(move || recovery_target_observation_blocking(&path, original))
        .await
        .map_err(|source| {
            AtmError::daemon_stale_owner_recovery_failed(
                "Unix HTTP socket stale-owner reinspection task ended unexpectedly",
            )
            .with_cause(source)
        })?
}

#[cfg(unix)]
fn recovery_target_observation_blocking(
    path: &Path,
    original: SocketRecoverySnapshot,
) -> Result<RecoveryTargetObservation, AtmError> {
    use std::fs;
    use std::io::ErrorKind;
    use std::os::unix::fs::FileTypeExt;

    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_socket()
                && is_owned_by(&metadata, original.owner_uid)
                && original.identity.matches(&metadata) =>
        {
            Ok(RecoveryTargetObservation::Matches)
        }
        Ok(_) => Ok(RecoveryTargetObservation::Changed),
        Err(source) if source.kind() == ErrorKind::NotFound => {
            Ok(RecoveryTargetObservation::Missing)
        }
        Err(source) => Err(AtmError::daemon_stale_owner_recovery_failed(
            "cannot re-inspect Unix HTTP socket path during stale-owner recovery",
        )
        .with_cause(source)),
    }
}

#[cfg(unix)]
async fn remove_stale_socket_path(
    path: PathBuf,
    original: SocketRecoverySnapshot,
) -> Result<(), AtmError> {
    // Reinspect and unlink in the same blocking operation.  Together with
    // UnixSocketStartupLock this prevents a supported same-user successor
    // from being unlinked in the observation-to-removal gap.
    tokio::task::spawn_blocking(move || {
        match recovery_target_observation_blocking(&path, original)? {
            RecoveryTargetObservation::Matches => std::fs::remove_file(&path).map_err(|source| {
                AtmError::daemon_stale_owner_recovery_failed(
                    "failed to remove stale Unix HTTP socket path",
                )
                .with_cause(source)
            }),
            RecoveryTargetObservation::Missing => Ok(()),
            RecoveryTargetObservation::Changed => {
                Err(AtmError::daemon_stale_owner_recovery_failed(
                    "Unix HTTP socket path changed immediately before stale-owner removal",
                )
                .with_cause(format!(
                    "refusing to unlink `{}` after identity changed",
                    path.display()
                )))
            }
        }
    })
    .await
    .map_err(|source| {
        AtmError::daemon_stale_owner_recovery_failed(
            "Unix HTTP socket stale-owner removal task ended unexpectedly",
        )
        .with_cause(source)
    })?
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketLiveness {
    Reachable,
    Dead,
    Disappeared,
    Changed,
}

#[cfg(unix)]
async fn probe_unix_socket_liveness(
    path: &Path,
    original: SocketRecoverySnapshot,
) -> std::io::Result<SocketLiveness> {
    // A refused AF_UNIX connection can be transient while a live listener's
    // backlog drains. Treat an endpoint as dead only after a bounded
    // observation window has both repeatedly refused connections and retained
    // its exact socket inode and owner throughout that window.
    for attempt in 0..SOCKET_LIVENESS_PROBE_ATTEMPTS {
        match tokio::net::UnixStream::connect(path).await {
            Ok(_) => return Ok(SocketLiveness::Reachable),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                match recovery_target_observation(path.to_path_buf(), original)
                    .await
                    .map_err(std::io::Error::other)?
                {
                    RecoveryTargetObservation::Matches => {}
                    RecoveryTargetObservation::Missing => return Ok(SocketLiveness::Disappeared),
                    RecoveryTargetObservation::Changed => return Ok(SocketLiveness::Changed),
                }
                if attempt + 1 < SOCKET_LIVENESS_PROBE_ATTEMPTS {
                    let multiplier = 1_u32 << attempt;
                    tokio::time::sleep(SOCKET_LIVENESS_PROBE_BACKOFF * multiplier).await;
                }
            }
            Err(error) => {
                match recovery_target_observation(path.to_path_buf(), original)
                    .await
                    .map_err(std::io::Error::other)?
                {
                    RecoveryTargetObservation::Matches => return Err(error),
                    RecoveryTargetObservation::Missing => return Ok(SocketLiveness::Disappeared),
                    RecoveryTargetObservation::Changed => return Ok(SocketLiveness::Changed),
                }
            }
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
    let staging = PrivateStagingDirectory::create(parent, socket.owner_uid.get())?;
    let staged_path = staging.path().join("listener.sock");
    validate_unix_socket_path_length(&staged_path)?;
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
    if !is_owned_by(&metadata, socket.owner_uid.get()) {
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
    identity: FileIdentity,
}

#[cfg(unix)]
impl PrivateStagingDirectory {
    pub(super) fn create(parent: &Path, owner_uid: u32) -> Result<Self, AtmError> {
        use std::fs;
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        reclaim_orphaned_staging_directories(parent, owner_uid)?;
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
            identity: FileIdentity::of(&metadata),
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for PrivateStagingDirectory {
    fn drop(&mut self) {
        remove_owned_path(&self.path, self.identity, |path| {
            std::fs::remove_dir_all(path)
        });
    }
}

/// Removes only the socket inode created by this runtime during shutdown.
#[cfg(unix)]
#[derive(Debug)]
pub(super) struct UnixSocketPathGuard {
    path: PathBuf,
    identity: FileIdentity,
}

#[cfg(unix)]
impl UnixSocketPathGuard {
    fn capture(path: &Path) -> Result<Self, AtmError> {
        use std::fs;
        let metadata = fs::metadata(path).map_err(|source| {
            AtmError::daemon_unavailable("failed to inspect bound Unix HTTP socket")
                .with_cause(source)
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            identity: FileIdentity::of(&metadata),
        })
    }
}

#[cfg(unix)]
impl Drop for UnixSocketPathGuard {
    fn drop(&mut self) {
        remove_owned_path(&self.path, self.identity, |path| std::fs::remove_file(path));
    }
}

#[cfg(unix)]
fn remove_owned_path(
    path: &Path,
    identity: FileIdentity,
    remove: impl FnOnce(&Path) -> std::io::Result<()>,
) {
    if std::fs::metadata(path).is_ok_and(|metadata| identity.matches(&metadata)) {
        let _ = remove(path);
    }
}

#[cfg(unix)]
fn reclaim_orphaned_staging_directories(parent: &Path, owner_uid: u32) -> Result<(), AtmError> {
    use std::fs;
    // Runtime composition holds UnixSocketStartupLock while it calls the
    // listener binder.  Thus a matching old directory cannot belong to a
    // concurrently-starting daemon.  Require both our private name and owner
    // before reclaiming crash leftovers.
    let now = std::time::SystemTime::now();
    for entry in fs::read_dir(parent).map_err(|source| {
        AtmError::daemon_unavailable("failed to scan Unix HTTP socket staging directories")
            .with_cause(source)
    })? {
        let entry = entry.map_err(|source| {
            AtmError::daemon_unavailable("failed to inspect Unix HTTP socket staging directory")
                .with_cause(source)
        })?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(".atm-uds-") {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| {
            AtmError::daemon_unavailable("failed to inspect Unix HTTP socket staging directory")
                .with_cause(source)
        })?;
        if !metadata.file_type().is_dir() || !is_owned_by(&metadata, owner_uid) {
            continue;
        }
        let Ok(age) = now.duration_since(metadata.modified().map_err(|source| {
            AtmError::daemon_unavailable("failed to inspect Unix HTTP socket staging age")
                .with_cause(source)
        })?) else {
            continue;
        };
        if age < ORPHAN_STAGING_MIN_AGE {
            continue;
        }
        fs::remove_dir_all(&path).map_err(|source| {
            AtmError::daemon_unavailable(
                "failed to reclaim orphaned Unix HTTP socket staging directory",
            )
            .with_cause(source)
        })?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::num::NonZeroU32;
    use std::os::unix::fs::MetadataExt;

    use super::{
        ORPHAN_STAGING_MIN_AGE, PrivateStagingDirectory, SocketLiveness, UNIX_SOCKET_PATH_CAPACITY,
        UnixSocketStartupLock, bind_unix_listener, inspect_recovery_target,
        probe_unix_socket_liveness, reclaim_stale_unix_socket, remove_stale_socket_path,
        validate_unix_socket_path_length,
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
        let uid = std::fs::metadata(directory.path())
            .expect("temporary UDS parent metadata")
            .uid();
        let stale = std::os::unix::net::UnixListener::bind(&path).expect("create stale socket");
        drop(stale);
        let snapshot = inspect_recovery_target(socket_config(path.clone(), uid))
            .await
            .expect("inspect stale socket")
            .expect("stale socket exists");

        let probe_path = path.clone();
        let probe =
            tokio::spawn(async move { probe_unix_socket_liveness(&probe_path, snapshot).await });
        tokio::task::yield_now().await;
        assert!(
            !probe.is_finished(),
            "one refusal cannot prove the socket is dead"
        );
        tokio::time::advance(std::time::Duration::from_millis(1_500)).await;
        assert_eq!(
            probe.await.expect("probe joins").expect("probe result"),
            SocketLiveness::Dead
        );
    }

    #[tokio::test(start_paused = true)]
    async fn changed_socket_is_never_classified_as_dead_after_a_refusal() {
        let directory = tempfile::tempdir().expect("temporary UDS parent");
        let path = directory.path().join("atm-daemon.sock");
        let uid = std::fs::metadata(directory.path())
            .expect("temporary UDS parent metadata")
            .uid();
        let stale = std::os::unix::net::UnixListener::bind(&path).expect("create stale socket");
        drop(stale);
        let snapshot = inspect_recovery_target(socket_config(path.clone(), uid))
            .await
            .expect("inspect stale socket")
            .expect("stale socket exists");

        let probe_path = path.clone();
        let probe =
            tokio::spawn(async move { probe_unix_socket_liveness(&probe_path, snapshot).await });
        tokio::task::yield_now().await;
        // Replace atomically: unlinking then writing leaves a real observation
        // window in which the probe can correctly report `Disappeared` instead
        // of the invariant this test is exercising (`Changed`).
        let replacement = directory.path().join("replacement");
        std::fs::write(&replacement, "different owner must be left alone")
            .expect("prepare a different occupied path");
        std::fs::rename(&replacement, &path).expect("atomically replace stale socket path");
        tokio::time::advance(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            probe.await.expect("probe joins").expect("probe result"),
            SocketLiveness::Changed
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn vanished_socket_is_classified_as_already_reclaimed() {
        let directory = tempfile::tempdir().expect("temporary UDS parent");
        let path = directory.path().join("atm-daemon.sock");
        let uid = std::fs::metadata(directory.path())
            .expect("temporary UDS parent metadata")
            .uid();
        let stale = std::os::unix::net::UnixListener::bind(&path).expect("create stale socket");
        drop(stale);
        let snapshot = inspect_recovery_target(socket_config(path.clone(), uid))
            .await
            .expect("inspect stale socket")
            .expect("stale socket exists");

        std::fs::remove_file(&path).expect("simulate concurrent stale-socket cleanup");
        assert_eq!(
            probe_unix_socket_liveness(&path, snapshot)
                .await
                .expect("probe result"),
            SocketLiveness::Disappeared
        );

        reclaim_stale_unix_socket(&socket_config(path.clone(), uid))
            .await
            .expect("an already-removed stale socket is successfully reclaimed");
        assert!(
            !path.exists(),
            "recovery must not recreate or report the vanished socket as occupied"
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

    #[test]
    fn socket_path_length_is_rejected_before_bind() {
        let accepted = std::path::PathBuf::from("x".repeat(UNIX_SOCKET_PATH_CAPACITY - 1));
        validate_unix_socket_path_length(&accepted).expect("last byte before NUL is accepted");

        let rejected = std::path::PathBuf::from("x".repeat(UNIX_SOCKET_PATH_CAPACITY));
        let error = validate_unix_socket_path_length(&rejected)
            .expect_err("socket pathname must leave space for its terminating NUL");
        assert_eq!(error.code().as_str(), "ATM_CONFIG_PARSE_FAILED");
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

    #[tokio::test]
    async fn stale_recovery_never_unlinks_a_replacement_socket() {
        let directory = tempfile::tempdir().expect("temporary UDS parent");
        let path = directory.path().join("atm-daemon.sock");
        let uid = std::fs::metadata(directory.path())
            .expect("temporary UDS parent metadata")
            .uid();
        let stale = std::os::unix::net::UnixListener::bind(&path).expect("create stale socket");
        drop(stale);
        let snapshot = inspect_recovery_target(socket_config(path.clone(), uid))
            .await
            .expect("inspect stale socket")
            .expect("stale socket exists");

        std::fs::remove_file(&path).expect("remove stale path before replacement");
        // A fast filesystem can immediately recycle the old socket inode for
        // the replacement. Reserve recycled inodes before retrying the bind so
        // this test deterministically exercises the changed-identity branch.
        let mut reservations = Vec::new();
        let replacement = (1..=16)
            .find_map(|attempt| {
                let candidate = std::os::unix::net::UnixListener::bind(&path)
                    .expect("create replacement socket");
                let metadata =
                    std::fs::symlink_metadata(&path).expect("replacement socket metadata");
                if !snapshot.identity.matches(&metadata) {
                    return Some(candidate);
                }

                drop(candidate);
                std::fs::remove_file(&path).expect("remove colliding replacement socket");
                let reservation_path = directory
                    .path()
                    .join(format!(".inode-reservation-{attempt}"));
                reservations.push(
                    std::fs::File::create(reservation_path)
                        .expect("reserve a recycled inode before replacement"),
                );
                None
            })
            .expect("replacement bind must obtain an identity distinct from the stale socket");
        drop(replacement);

        let error = remove_stale_socket_path(path.clone(), snapshot)
            .await
            .expect_err("replacement identity must not be unlinked");
        assert_eq!(
            error.code().as_str(),
            "ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED"
        );
        assert!(path.exists(), "replacement socket is retained");
    }

    #[test]
    fn private_staging_creation_reclaims_old_crash_leftovers() {
        let directory = tempfile::tempdir().expect("temporary UDS parent");
        let owner_uid = std::fs::metadata(directory.path())
            .expect("temporary UDS parent metadata")
            .uid();
        let orphan = directory.path().join(".atm-uds-crash-leftover");
        std::fs::create_dir(&orphan).expect("create orphan staging directory");
        let orphan_handle = std::fs::File::open(&orphan).expect("open orphan staging directory");
        orphan_handle
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::SystemTime::now() - ORPHAN_STAGING_MIN_AGE),
            )
            .expect("age orphan staging directory");

        let staging = PrivateStagingDirectory::create(directory.path(), owner_uid)
            .expect("new staging directory reclaims only old leftovers");
        assert!(
            !orphan.exists(),
            "a sufficiently old, owner-owned staging directory left by a crash is reclaimed"
        );
        drop(staging);
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
        assert_eq!(
            std::fs::read_to_string(&path).expect("occupied path remains readable"),
            "must not be unlinked as a socket",
            "safety rejection must preserve the non-socket payload"
        );
    }
}
