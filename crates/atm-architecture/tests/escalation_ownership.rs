//! BA.3 escalation ownership guard.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_sources(directory: &std::path::Path) -> String {
    fs::read_dir(directory)
        .expect("source directory readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .map(|path| {
            if path.is_dir() {
                rust_sources(&path)
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                fs::read_to_string(path).expect("Rust source readable")
            } else {
                String::new()
            }
        })
        .collect()
}

#[test]
fn escalation_ownership_architecture_test() {
    let root = workspace_root();
    let core = rust_sources(&root.join("crates/atm-core/src"));
    assert!(
        !core.contains("escalate_mail") && !core.contains("escalation_summary"),
        "escalation policy belongs to atm-http-runtime, never atm-core"
    );

    let wake = fs::read_to_string(root.join("crates/atm-http-runtime/src/herdr_queue_wake.rs"))
        .expect("read Herdr queue wake source");
    assert!(
        !wake.contains("DeliveryChannel::HerdrSteer"),
        "BA.3 evaluates every roster member after observation; it must not pre-filter Herdr delivery"
    );
}
