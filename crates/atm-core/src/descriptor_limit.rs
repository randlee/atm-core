//! Process descriptor-limit bootstrap for the daemon runtime.
//!
//! A daemon started by a service manager inherits that manager's soft
//! `RLIMIT_NOFILE`, which on macOS launchd is 256. The SQLite boundary alone
//! needs a descriptor per open connection plus its `-wal` sidecar, and the
//! HTTP listeners need one per accepted connection, so a soft limit inherited
//! from the service manager can be exhausted by ordinary admission load. The
//! result is `SQLITE_CANTOPEN` both at connection open and, because the WAL
//! sidecars are opened lazily, at query time on an already-open connection.
//!
//! Raising the soft limit to the hard limit at startup costs nothing and is
//! never a silent degrade: the outcome is returned so the caller reports it.

/// Result of the startup attempt to raise the process descriptor soft limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorLimitOutcome {
    /// The soft limit was raised from `previous` to `current`.
    Raised { previous: u64, current: u64 },
    /// The soft limit already matched the permitted maximum.
    AlreadyAtMaximum { current: u64 },
    /// The platform does not expose a per-process descriptor soft limit.
    Unsupported,
    /// The limit could not be read or raised; `current` is the observed soft
    /// limit when one could still be read.
    Failed { current: Option<u64>, errno: i32 },
}

/// Descending soft-limit candidates for hosts whose hard limit is
/// `RLIM_INFINITY`. macOS rejects an infinite soft `RLIMIT_NOFILE` and caps it
/// at `kern.maxfilesperproc`, so the first accepted candidate is used.
#[cfg(unix)]
const UNBOUNDED_HARD_LIMIT_CANDIDATES: [u64; 4] = [65_536, 16_384, 4_096, 1_024];

/// Raises this process's descriptor soft limit to the permitted maximum.
///
/// This is a startup-only operation and is idempotent.
#[cfg(unix)]
#[must_use]
pub fn raise_descriptor_soft_limit() -> DescriptorLimitOutcome {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `getrlimit` only reads the process limit into the local struct.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) } != 0 {
        return DescriptorLimitOutcome::Failed {
            current: None,
            errno: last_errno(),
        };
    }
    let previous = limit.rlim_cur;
    let hard = limit.rlim_max;
    let mut candidates = Vec::new();
    if hard == libc::RLIM_INFINITY {
        candidates.extend(UNBOUNDED_HARD_LIMIT_CANDIDATES);
    } else {
        candidates.push(hard);
        candidates.extend(
            UNBOUNDED_HARD_LIMIT_CANDIDATES
                .into_iter()
                .filter(|candidate| *candidate < hard),
        );
    }
    let mut errno = 0;
    for candidate in candidates {
        if candidate <= previous {
            break;
        }
        let raised = libc::rlimit {
            rlim_cur: candidate,
            rlim_max: hard,
        };
        // SAFETY: `setrlimit` reads the local struct and never retains it.
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raw const raised) } == 0 {
            return DescriptorLimitOutcome::Raised {
                previous,
                current: candidate,
            };
        }
        errno = last_errno();
    }
    if errno == 0 {
        DescriptorLimitOutcome::AlreadyAtMaximum { current: previous }
    } else {
        DescriptorLimitOutcome::Failed {
            current: Some(previous),
            errno,
        }
    }
}

#[cfg(unix)]
fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Windows has no per-process descriptor soft limit to raise; socket and
/// handle capacity is bounded by the kernel object quota instead.
#[cfg(windows)]
#[must_use]
pub fn raise_descriptor_soft_limit() -> DescriptorLimitOutcome {
    DescriptorLimitOutcome::Unsupported
}

#[cfg(test)]
mod tests {
    use super::{DescriptorLimitOutcome, raise_descriptor_soft_limit};

    #[test]
    fn raising_the_descriptor_limit_reports_an_outcome_and_is_idempotent() {
        let first = raise_descriptor_soft_limit();
        let second = raise_descriptor_soft_limit();
        assert!(
            !matches!(first, DescriptorLimitOutcome::Failed { .. }),
            "the startup descriptor-limit raise must not fail on a supported host: {first:?}"
        );
        if let DescriptorLimitOutcome::Raised { current, .. } = first {
            assert_eq!(
                second,
                DescriptorLimitOutcome::AlreadyAtMaximum { current },
                "a second raise must observe the already-raised soft limit"
            );
        }
    }
}
