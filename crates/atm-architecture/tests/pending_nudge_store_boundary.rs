//! Structural ownership guard for the AQ1 `PendingNudgeStore` seam.
//!
//! `atm-storage-rusqlite` is the concrete backend that must implement
//! `PendingNudgeStore`, but it cannot depend on `atm-core` (a documented
//! forbidden edge enforced generically by `boundary_enforcement.rs`). This
//! file asserts the narrower, seam-specific placement decision recorded in
//! `docs/plans/phase-aq/aq1-blueprint.md` D1: `MemberKey`, `NudgeClaim`, and
//! `PendingNudgeStore` are defined in `atm-storage`, and the concrete SQLite
//! implementation satisfies the contract without referencing `atm-core` or
//! `atm_core` anywhere in its own module.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn member_key_and_pending_nudge_store_are_owned_by_atm_storage() {
    let root = workspace_root();
    let types_source = fs::read_to_string(root.join("crates/atm-storage/src/types.rs"))
        .expect("read atm-storage types source");
    let contract_source = fs::read_to_string(root.join("crates/atm-storage/src/contract.rs"))
        .expect("read atm-storage contract source");
    let lib_source = fs::read_to_string(root.join("crates/atm-storage/src/lib.rs"))
        .expect("read atm-storage lib source");

    assert!(
        types_source.contains("pub struct MemberKey"),
        "MemberKey must be defined in atm-storage::types, not re-derived by a downstream crate"
    );
    assert!(
        contract_source.contains("pub trait PendingNudgeStore: sealed::Sealed + Send + Sync {"),
        "PendingNudgeStore must be a sealed atm-storage::contract trait"
    );
    assert!(
        contract_source.contains("pub struct NudgeClaim"),
        "NudgeClaim must be defined in atm-storage::contract"
    );
    assert!(
        contract_source.contains("pub const MAX_NUDGE_ATTEMPTS: u32 = 5;"),
        "MAX_NUDGE_ATTEMPTS must stay a documented atm-storage::contract constant"
    );
    assert!(
        lib_source.contains("MemberKey"),
        "atm-storage::lib must re-export MemberKey for downstream crates"
    );
    assert!(
        lib_source.contains("PendingNudgeStore"),
        "atm-storage::lib must re-export PendingNudgeStore for downstream crates"
    );
}

#[test]
fn sqlite_pending_nudge_store_implements_the_contract_without_depending_on_atm_core() {
    let root = workspace_root();
    let manifest = fs::read_to_string(root.join("crates/atm-storage-rusqlite/Cargo.toml"))
        .expect("read atm-storage-rusqlite manifest");
    let backend_source =
        fs::read_to_string(root.join("crates/atm-storage-rusqlite/src/pending_nudge_store.rs"))
            .expect("read atm-storage-rusqlite pending_nudge_store source");

    assert!(
        !manifest.contains("atm-core"),
        "atm-storage-rusqlite must not depend on atm-core to implement PendingNudgeStore"
    );
    assert!(
        backend_source
            .contains("impl atm_storage::contract::sealed::Sealed for SqlitePendingNudgeStore"),
        "SqlitePendingNudgeStore must seal itself against atm_storage::contract::sealed::Sealed directly"
    );
    assert!(
        backend_source.contains("impl PendingNudgeStore for SqlitePendingNudgeStore"),
        "SqlitePendingNudgeStore must implement the atm-storage PendingNudgeStore contract"
    );
    assert!(
        !backend_source.contains("atm_core") && !backend_source.contains("atm-core"),
        "the concrete backend module must not reference atm-core; atm-storage-rusqlite cannot see atm-core"
    );
}
