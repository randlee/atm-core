#![forbid(unsafe_code)]
//! Concrete runtime/store composition for ATM callers that must stay
//! storage-neutral at their own crate boundary.

mod composition;
mod legacy_storage_adapters;
mod replay_store;

pub use composition::{
    RuntimeAssembly, RuntimeAssemblyInputs, assemble_default_runtime, assemble_sqlite_runtime,
    default_local_runtime, with_default_roster_store,
};
#[cfg(any(test, feature = "test-utils"))]
pub use replay_store::sqlite_remote_replay_store_for_test;
