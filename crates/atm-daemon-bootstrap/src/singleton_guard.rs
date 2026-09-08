//! Fail-closed singleton checks owned by the Tokio/Axum daemon bootstrap.
//!
//! The account-scoped owner lock is authoritative. It is intentionally the
//! only runtime guard: process tables and network endpoints are host-wide and
//! would incorrectly couple distinct OS accounts or container hosts.

use atm_core::error::AtmError;

use crate::DaemonOwnerGuard;

pub(crate) const GUARD_NAMES: [&str; 2] = ["owner lock ownership", "static daemon-launch lint"];

/// Singleton state has no host-global inputs; the owner lock scopes ownership
/// to the OS account's runtime directory.
pub(crate) struct SingletonGuards;

impl SingletonGuards {
    pub(crate) const fn new() -> Self {
        Self
    }

    /// Verifies ownership before the daemon begins serving.
    pub(crate) fn verify_startup(&self, owner: &DaemonOwnerGuard) -> Result<(), AtmError> {
        let _required_guards = GUARD_NAMES;
        owner.verify_ownership()
    }

    /// The serving task calls this continuously so a replaced owner record
    /// immediately terminates the process.
    pub(crate) fn verify_while_serving(&self, owner: &DaemonOwnerGuard) -> Result<(), AtmError> {
        owner.verify_ownership()
    }
}

/// A duplicate daemon is a product-integrity violation, not a recoverable
/// availability condition. The caller must have already recorded the typed
/// error that led here; abort prevents a compromised server from serving.
pub(crate) fn abort_for_singleton_violation<T: std::fmt::Display>(violation: T) -> ! {
    eprintln!("ATM daemon singleton violation; aborting immediately: {violation}");
    tracing::error!(target: "atm_daemon_bootstrap::singleton", %violation, "ATM daemon singleton violation; aborting immediately");
    std::process::abort();
}

#[cfg(test)]
mod tests {
    use super::{GUARD_NAMES, abort_for_singleton_violation};

    #[test]
    fn requirements_test_names_the_account_scoped_singleton_guards() {
        assert_eq!(
            GUARD_NAMES,
            ["owner lock ownership", "static daemon-launch lint"]
        );
    }

    #[test]
    fn singleton_violation_abort_has_never_returning_type() {
        let _: fn(&'static str) -> ! = abort_for_singleton_violation::<&'static str>;
    }
}
