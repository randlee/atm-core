//! Private, collision-safe staging allocation shared by runtime-owned files.
//!
//! Each caller supplies its platform-specific creation operation (a `0600`
//! file or `0700` directory). This owner supplies the only retry/collision
//! algorithm, so endpoint-record and Unix-socket publication cannot drift.

use std::io;
use std::path::{Path, PathBuf};
const ALLOCATION_ATTEMPTS: u64 = 64;

pub(crate) fn allocate<T>(
    parent: &Path,
    kind: &str,
    mut create: impl FnMut(&Path) -> io::Result<T>,
) -> io::Result<(PathBuf, T)> {
    for _ in 0..ALLOCATION_ATTEMPTS {
        // UDS paths are subject to the platform's small `sockaddr_un` path
        // budget, so keep this private name deliberately compact.
        // A process id plus a process-local counter is predictable to another
        // local user.  The owner-controlled parent prevents replacement after
        // creation, while this 60-bit prefix of a ULID makes pre-creation
        // collision/DoS attacks infeasible without exhausting the UDS path
        // budget on macOS.
        let id = ulid::Ulid::new().to_string();
        let path = parent.join(format!(".atm-{kind}-{}", &id[..12]));
        match create(&path) {
            Ok(value) => return Ok((path, value)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique private runtime staging path",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;

    use super::allocate;

    #[test]
    fn allocator_returns_distinct_new_paths() {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let create =
            |path: &std::path::Path| OpenOptions::new().write(true).create_new(true).open(path);
        let (first, _) = allocate(temporary_directory.path(), "test", create)
            .expect("first private staging path");
        let (second, _) = allocate(temporary_directory.path(), "test", create)
            .expect("second private staging path");
        assert_ne!(first, second);
    }
}
