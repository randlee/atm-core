//! Keep AY.8's socket transport out of the production composition root.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_sources(root: &Path, output: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root).expect("source directory");
    for entry in entries {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            rust_sources(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}

#[test]
fn socket_variant_constructed_only_in_tests() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    rust_sources(&source_root, &mut sources);

    for source in sources {
        let contents = fs::read_to_string(&source).expect("Rust source");
        if source
            .file_name()
            .is_some_and(|name| name == "transport_socket.rs")
        {
            let production = contents
                .split("#[cfg(test)]")
                .next()
                .expect("transport socket source");
            assert!(
                !production.contains("HerdrIo::Socket("),
                "socket construction must remain test-only: {}",
                source.display()
            );
        } else {
            assert!(
                !contents.contains("HerdrIo::Socket("),
                "socket construction escaped its AY.8 test module: {}",
                source.display()
            );
        }
    }
}
