//! Structural ownership guard for the AZ.4 attention scheduler seams.
//!
//! The semantic scheduler traits belong to `atm-storage`; the SQLite adapter
//! implements them downstream. This test keeps that direction visible in both
//! the source and the real Cargo dependency graph, so a concrete SQLite type
//! cannot cross back into the storage-neutral boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use cargo_metadata::{DependencyKind, MetadataCommand};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn attention_schedule_traits_are_owned_and_reexported_by_atm_storage() {
    let root = workspace_root();
    let contract = fs::read_to_string(root.join("crates/atm-storage/src/attention.rs"))
        .expect("read attention scheduler contract");
    let lib = fs::read_to_string(root.join("crates/atm-storage/src/lib.rs"))
        .expect("read atm-storage lib");

    assert!(
        contract.contains("pub trait AttentionScheduleStore: sealed::Sealed + Send + Sync {"),
        "AttentionScheduleStore must remain an atm-storage-owned sealed trait"
    );
    assert!(
        contract.contains("pub trait AsyncAttentionScheduleStore: sealed::Sealed + Send + Sync {"),
        "AsyncAttentionScheduleStore must remain an atm-storage-owned sealed trait"
    );
    assert!(
        lib.contains("AsyncAttentionScheduleStore") && lib.contains("AttentionScheduleStore"),
        "atm-storage::lib must re-export both attention scheduler traits"
    );
}

#[test]
fn sqlite_attention_adapter_implements_downstream_without_crossing_the_boundary() {
    let root = workspace_root();
    let contract = fs::read_to_string(root.join("crates/atm-storage/src/attention.rs"))
        .expect("read attention scheduler contract");
    let storage_manifest = fs::read_to_string(root.join("crates/atm-storage/Cargo.toml"))
        .expect("read atm-storage manifest");
    let backend = fs::read_to_string(
        root.join("crates/atm-storage-rusqlite/src/attention_schedule_store.rs"),
    )
    .expect("read sqlite attention scheduler adapter");
    let backend_manifest = fs::read_to_string(root.join("crates/atm-storage-rusqlite/Cargo.toml"))
        .expect("read sqlite storage manifest");

    assert!(
        !contract.contains("rusqlite")
            && !contract.contains("Connection")
            && !contract.contains("Sqlite"),
        "the storage-neutral attention contract must not name concrete SQLite types"
    );
    assert!(
        !storage_manifest.contains("rusqlite"),
        "atm-storage must not depend on the concrete SQLite backend"
    );
    assert!(
        backend_manifest.contains("atm-storage.workspace = true"),
        "the SQLite adapter must depend on the storage-neutral contract"
    );
    assert!(
        backend.contains(
            "impl atm_storage::contract::sealed::Sealed for SqliteAttentionScheduleStore {}"
        ),
        "the SQLite attention adapter must seal its implementation directly"
    );
    assert!(
        backend.contains("impl AttentionScheduleStore for SqliteAttentionScheduleStore"),
        "the SQLite adapter must implement the synchronous attention contract"
    );
    assert!(
        backend.contains("impl AsyncAttentionScheduleStore for SqliteAttentionScheduleStore"),
        "the SQLite adapter must implement the asynchronous attention contract"
    );

    let dependencies = direct_normal_dependencies();
    assert!(
        !dependencies
            .get("atm-storage")
            .is_some_and(|deps| deps.contains("atm-storage-rusqlite")),
        "the Cargo graph must not point the storage boundary back at SQLite"
    );
}

fn direct_normal_dependencies() -> BTreeMap<String, BTreeSet<String>> {
    let metadata = MetadataCommand::new()
        .manifest_path(workspace_root().join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata must succeed for the workspace");

    metadata
        .packages
        .into_iter()
        .map(|package| {
            let dependencies = package
                .dependencies
                .into_iter()
                .filter(|dependency| dependency.kind == DependencyKind::Normal)
                .map(|dependency| dependency.name.to_string())
                .collect();
            (package.name.to_string(), dependencies)
        })
        .collect()
}
