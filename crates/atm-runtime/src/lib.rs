#![forbid(unsafe_code)]
#![allow(
    deprecated,
    reason = "Phase AC runtime composition still consumes the transitional shared storage traits while backend adoption settles"
)]
//! Backend-neutral runtime/store composition for ATM callers.

mod composition;
mod legacy_storage_adapters;

pub use atm_storage::{StorageFactory, StorageHandles};
pub use composition::{
    RuntimeAssembly, RuntimeAssemblyInputs, assemble_runtime, with_installed_roster_store,
};
