//! Unix-domain-socket listener setup and owner-safe cleanup.
//!
//! This physical adapter is deliberately separate from runtime composition:
//! it validates, stages, publishes, and tears down only the UDS endpoint.

use std::path::{Path, PathBuf};

use atm_core::error::AtmError;
#[cfg(unix)]
use tokio::net::UnixListener;

use super::UnixSocketConfig;

#[cfg(unix)]
pub(super) fn bind_unix_listener(
    socket: &UnixSocketConfig,
) -> Result<(UnixListener, UnixSocketPathGuard), AtmError> {
    let parent = validate_unix_socket_parent(socket)?;
    ensure_unix_socket_path_unoccupied(&socket.path)?;
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
fn ensure_unix_socket_path_unoccupied(path: &Path) -> Result<(), AtmError> {
    use std::fs;
    use std::io::ErrorKind;

    match fs::symlink_metadata(path) {
        Ok(_) => Err(AtmError::config("Unix HTTP socket path is already occupied").with_cause(
            format!(
                "refusing to replace existing path `{}`; remove only the stale owner-owned socket before retrying",
                path.display()
            ),
        )),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => {
            Err(AtmError::config("cannot inspect Unix HTTP socket path").with_cause(source))
        }
    }
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
