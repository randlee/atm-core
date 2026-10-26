//! The published HTTP API document must state the runtime's `HTTP_API_VERSION`.
//!
//! The header row, the current-baseline sentence and the version history table
//! all name the version, so each one is checked against the runtime constant.

use std::path::PathBuf;

use atm_core::protocol::HTTP_API_VERSION;

fn doc_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/atm-daemon/http-api.md")
}

#[test]
fn published_http_api_doc_states_the_runtime_version() {
    let source = std::fs::read_to_string(doc_path()).expect("read HTTP API document");
    let version = HTTP_API_VERSION;

    let header = source
        .lines()
        .find(|line| line.contains("HTTP API SemVer"))
        .expect("HTTP API document has a SemVer header row");
    assert!(
        header.contains(&format!("`{version}`")),
        "header row must state `{version}`: {header}"
    );

    assert!(
        source.contains(&format!("The current baseline is `{version}`.")),
        "the current-baseline sentence must state `{version}`"
    );

    assert!(
        source.contains(&format!("| `{version}` |")),
        "the version history table must have a `{version}` row"
    );
}
