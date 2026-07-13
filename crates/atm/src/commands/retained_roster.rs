#![allow(
    deprecated,
    reason = "the retained CLI roster helper still forwards the legacy atm-core roster boundary during the Phase AC transition"
)]

use anyhow::Result;
use atm_core::boundary::RosterStore;
use atm_core::error::AtmError;
#[cfg(not(test))]
use atm_daemon_bootstrap::install_sqlite_retained_runtime_factory;
use atm_daemon_bootstrap::with_default_roster_store;
#[cfg(test)]
use atm_runtime_test_support::install_sqlite_retained_runtime_factory;

pub(crate) fn with_retained_roster_store<T>(
    f: impl FnOnce(&(dyn RosterStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T> {
    // Retained roster commands do not create a `CliComposition`, so they must
    // install the appropriate runtime factory before opening the boundary.
    // The test factory is deliberately distinct so fixture-scoped databases
    // remain isolated from the production host database.
    install_sqlite_retained_runtime_factory();
    with_default_roster_store(f).map_err(Into::into)
}
