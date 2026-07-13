#![allow(
    deprecated,
    reason = "the retained CLI roster helper still forwards the legacy atm-core roster boundary during the Phase AC transition"
)]

use anyhow::Result;
use atm_core::boundary::RosterStore;
use atm_core::error::AtmError;
use atm_daemon_bootstrap::{install_sqlite_retained_runtime_factory, with_default_roster_store};

pub(crate) fn with_retained_roster_store<T>(
    f: impl FnOnce(&(dyn RosterStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T> {
    // Retained roster commands do not create a `CliComposition`, so they must
    // install the production runtime factory before opening the boundary.
    // This is intentionally the same one-shot bootstrap used by daemon-backed
    // commands and makes the standalone CLI executable self-sufficient.
    install_sqlite_retained_runtime_factory();
    with_default_roster_store(f).map_err(Into::into)
}
