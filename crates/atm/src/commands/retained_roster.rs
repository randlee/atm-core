use anyhow::Result;
use atm_core::boundary::RosterStore;
use atm_core::error::AtmError;
use atm_daemon_bootstrap::with_default_roster_store;

pub(crate) fn with_retained_roster_store<T>(
    f: impl FnOnce(&(dyn RosterStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T> {
    with_default_roster_store(f).map_err(Into::into)
}
