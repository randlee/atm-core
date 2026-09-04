//! Structural ownership guard for the registry-backed graft receiver.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn receiver_registry_ownership_has_one_flock_owner_and_no_file_publication() {
    let root = workspace_root();
    let graft_source = fs::read_to_string(root.join("crates/atm-core/src/graft.rs"))
        .expect("read graft ownership source");
    let production = graft_source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    let python_source = fs::read_to_string(root.join("crates/atm-graft-python/src/lib.rs"))
        .expect("read Python graft source");
    let write_symbol = concat!("write_", "receiver_record");
    let read_symbol = concat!("read_", "receiver_record");
    let home_path_symbol = concat!("graft_receiver_", "record_path_from_home");
    let root_path_symbol = concat!("graft_receiver_", "record_path_from_root");

    assert_eq!(
        production.matches(write_symbol).count(),
        0,
        "graft listener must not publish a file endpoint"
    );
    assert_eq!(
        production.matches(read_symbol).count(),
        0,
        "graft listener must not read a file endpoint"
    );
    assert_eq!(
        production.matches(home_path_symbol).count(),
        0,
        "home-derived endpoint record paths must be retired"
    );
    assert_eq!(
        production.matches(root_path_symbol).count(),
        0,
        "root-derived endpoint record paths must be retired"
    );
    assert_eq!(
        production
            .matches("ReceiverOwnershipGuard::acquire(")
            .count(),
        1,
        "the flock acquisition primitive must remain singular"
    );
    assert_eq!(
        production
            .matches("impl Drop for ReceiverOwnershipGuard")
            .count(),
        1,
        "the flock release owner must remain singular"
    );

    // AQ1.6 QA-2 (RULE-003) split the registry lease lifecycle out of
    // `runtime.rs` into `runtime/mod.rs` + `runtime/lease_client.rs`; read
    // both so this guard survives code moving between the two.
    let runtime_source = format!(
        "{}\n{}",
        fs::read_to_string(root.join("crates/atm-graft/src/runtime/mod.rs"))
            .expect("read graft runtime module"),
        fs::read_to_string(root.join("crates/atm-graft/src/runtime/lease_client.rs"))
            .expect("read graft lease client module")
    );
    assert_eq!(
        runtime_source
            .matches("impl Drop for RegisteredGraftReceiver")
            .count(),
        1,
        "registry lease cleanup must have one owner"
    );
    let unregister_start = runtime_source
        .find("impl Drop for RegisteredGraftReceiver")
        .expect("registered receiver drop");
    let unregister_body = &runtime_source[unregister_start..];
    assert_eq!(
        runtime_source.matches(".unregister_receiver_sync(").count(),
        unregister_body
            .matches(".unregister_receiver_sync(")
            .count(),
        "daemon unregister must remain owned by RegisteredGraftReceiver::drop"
    );

    assert!(
        !python_source.contains(write_symbol)
            && !python_source.contains(read_symbol)
            && !python_source.contains(concat!("graft_receiver_", "record_path"))
            && !python_source.contains(".register(")
            && !python_source.contains(".unregister(")
            && !python_source.contains(".refresh("),
        "Python graft bindings must not manage receiver file or lease lifecycle"
    );
}
