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
    let authorized_record_publication_sites = [
        "fn write_receiver_record(",
        "write_receiver_record(record_path, &record)?;",
        "write_receiver_record(&self.record_path, &self.record)?;",
    ];
    assert_eq!(
        production.matches("write_receiver_record(").count(),
        authorized_record_publication_sites.len(),
        "only the private helper, GraftReceiverListener::bind, and same-owner republish may publish a receiver record"
    );
    for site in authorized_record_publication_sites {
        assert_eq!(
            production.matches(site).count(),
            1,
            "receiver record publication site must remain the reviewed exact call site: {site}"
        );
    }
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
