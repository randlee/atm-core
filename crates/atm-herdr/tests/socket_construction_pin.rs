//! Keep AY.8's socket transport out of the production composition root.

use std::fs;
use std::path::{Path, PathBuf};

fn socket_construction_offsets(source: &str) -> Vec<usize> {
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
        } else if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
        } else if bytes[index] == b'"' || bytes[index] == b'\'' {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
        } else if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push((start, &source[start..index]));
        } else {
            tokens.push((index, &source[index..index + 1]));
            index += 1;
        }
    }

    tokens
        .windows(4)
        .filter_map(|window| {
            (window[0].1 == "HerdrIo"
                && window[1].1 == ":"
                && window[2].1 == ":"
                && window[3].1 == "Socket"
                && tokens
                    .get(tokens.iter().position(|token| token.0 == window[3].0)? + 1)?
                    .1
                    == "(")
                .then_some(window[0].0)
        })
        .collect()
}

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
        for offset in socket_construction_offsets(&contents) {
            let allowed = source
                .file_name()
                .is_some_and(|name| name == "transport_socket.rs")
                && contents[..offset].contains("#[cfg(test)]");
            assert!(
                allowed,
                "socket construction escaped its AY.8 test module: {}:{}",
                source.display(),
                contents[..offset]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1
            );
        }
    }
}

#[test]
fn socket_transport_items_remain_crate_private() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/transport_socket.rs"))
            .expect("socket transport source");
    for item in [
        "pub struct HerdrHostEnv",
        "pub enum HerdrEndpoint",
        "pub struct SocketIo",
        "pub fn herdr_api_endpoint",
    ] {
        assert!(
            !source.contains(item),
            "AY.2 public-item pin widened: {item}"
        );
    }
    for item in [
        "pub(crate) struct HerdrHostEnv",
        "pub(crate) enum HerdrEndpoint",
        "pub(crate) struct SocketIo",
        "pub(crate) fn herdr_api_endpoint",
    ] {
        assert!(source.contains(item), "missing crate-private pin: {item}");
    }
}
