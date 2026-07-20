#![forbid(unsafe_code)]
#![allow(
    deprecated,
    reason = "Phase AC runtime composition still consumes the transitional shared storage traits while backend adoption settles"
)]
//! Concrete runtime/store composition for ATM callers that must stay
//! storage-neutral at their own crate boundary.

mod composition;
mod legacy_storage_adapters;
mod sqlite_observability;

pub use composition::{
    RuntimeAssembly, RuntimeAssemblyInputs, assemble_default_runtime, assemble_sqlite_runtime,
    default_local_runtime, with_default_nudge_template_override_store, with_default_roster_store,
    with_installed_roster_store,
};
pub use sqlite_observability::{RuntimeSqliteEvent, RuntimeSqliteObserver, RuntimeSqliteOutcome};
