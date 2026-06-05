#![forbid(unsafe_code)]
mod composition;
mod replay_store;

pub use composition::{
    RuntimeAssembly, RuntimeAssemblyInputs, assemble_default_runtime, assemble_sqlite_runtime,
    default_local_runtime, with_default_roster_store,
};
#[cfg(any(test, feature = "test-utils"))]
pub use replay_store::sqlite_remote_replay_store_for_test;
