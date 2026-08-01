//! Structural ownership guard for the graft receiver endpoint record.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn receiver_record_publication_and_unlink_stay_in_the_listener_owner() {
    let root = workspace_root();
    let graft_source = fs::read_to_string(root.join("crates/atm-core/src/graft.rs"))
        .expect("read graft ownership source");
    let production = graft_source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");
    let python_source = fs::read_to_string(root.join("crates/atm-graft-python/src/lib.rs"))
        .expect("read Python graft source");

    assert!(
        !production.contains("pub fn write_receiver_record"),
        "endpoint record publication must remain private to GraftReceiverListener"
    );
    assert_eq!(
        production.matches("write_receiver_record(").count(),
        2,
        "only the private helper and GraftReceiverListener::bind may publish a receiver record"
    );
    assert_eq!(
        production
            .matches("fs::remove_file(&self.record_path)")
            .count(),
        1,
        "only GraftReceiverListener cleanup may unlink its endpoint record"
    );
    assert!(
        !python_source.contains("write_receiver_record")
            && !python_source.contains("remove_file(&self.record_path)"),
        "Python graft bindings must not publish or unlink receiver endpoint records"
    );
}
