//! Architecture guard for AY.7's Windows CLI process boundary.

use std::fs;
use std::path::Path;

const WINDOWS_PROCESS_MARKERS: &[&str] = &[
    "creation_flags",
    "CREATE_NO_WINDOW",
    "herdr.exe",
    "cfg(windows)",
];

#[test]
fn windows_cli_process_behavior_is_confined_to_transport_cli() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for source in rust_sources(&source_root) {
        if source.file_name().and_then(|name| name.to_str()) == Some("transport_cli.rs") {
            continue;
        }
        let text = fs::read_to_string(&source).expect("Rust source must be UTF-8");
        for marker in WINDOWS_PROCESS_MARKERS {
            assert!(
                !text.contains(marker),
                "Windows CLI process marker {marker:?} escaped transport_cli.rs into {}",
                source.display()
            );
        }
    }
}

fn rust_sources(root: &Path) -> Vec<std::path::PathBuf> {
    let mut sources = Vec::new();
    for entry in fs::read_dir(root).expect("read atm-herdr source directory") {
        let entry = entry.expect("read source entry");
        let path = entry.path();
        if path.is_dir() {
            sources.extend(rust_sources(&path));
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            sources.push(path);
        }
    }
    sources.sort();
    sources
}
