//! Structural ownership guard for the AX3 task ledger seam.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn task_store_is_storage_owned_and_sqlite_implements_without_core_dependency() {
    let root = workspace_root();
    let contract = fs::read_to_string(root.join("crates/atm-storage/src/task_store.rs"))
        .expect("read task store contract");
    let backend = fs::read_to_string(root.join("crates/atm-storage-rusqlite/src/task_store.rs"))
        .expect("read sqlite task store");
    let manifest = fs::read_to_string(root.join("crates/atm-storage-rusqlite/Cargo.toml"))
        .expect("read sqlite manifest");

    assert!(contract.contains("pub trait TaskStore: sealed::Sealed + Send + Sync"));
    assert!(contract.contains("pub struct DummyTaskStore"));
    assert!(backend.contains("impl TaskStore for SqliteTaskStore"));
    assert!(backend.contains("impl atm_storage::contract::sealed::Sealed for SqliteTaskStore"));
    assert!(!manifest.contains("atm-core"));
    assert!(!backend.contains("atm_core") && !backend.contains("atm-core"));
}
