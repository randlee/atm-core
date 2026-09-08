//! Pin AY.9's sole production socket construction factory.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Token {
    offset: usize,
    text: String,
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

fn lex_rust(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
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
            tokens.push(Token {
                offset: start,
                text: source[start..index].to_owned(),
            });
        } else {
            tokens.push(Token {
                offset: index,
                text: source[index..index + 1].to_owned(),
            });
            index += 1;
        }
    }
    tokens
}

fn aliases(tokens: &[Token]) -> BTreeSet<String> {
    let mut aliases = BTreeSet::from(["HerdrIo".to_owned()]);
    for window in tokens.windows(3) {
        if window[0].text == "HerdrIo" && window[1].text == "as" {
            aliases.insert(window[2].text.clone());
        }
    }
    for index in 0..tokens.len().saturating_sub(3) {
        if tokens[index].text == "type"
            && tokens[index + 2].text == "="
            && tokens[index + 3].text == "HerdrIo"
        {
            aliases.insert(tokens[index + 1].text.clone());
        }
    }
    aliases
}

fn brace_ranges(tokens: &[Token], source: &str, markers: &[&str]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for marker in markers {
        let Some(marker_offset) = source.find(marker) else {
            continue;
        };
        let Some(open_index) = tokens
            .iter()
            .position(|token| token.offset > marker_offset && token.text == "{")
        else {
            continue;
        };
        let mut depth = 0usize;
        for token in &tokens[open_index..] {
            if token.text == "{" {
                depth += 1;
            } else if token.text == "}" {
                depth -= 1;
                if depth == 0 {
                    ranges.push((tokens[open_index].offset, token.offset));
                    break;
                }
            }
        }
    }
    ranges
}

fn socket_construction_offsets(tokens: &[Token], aliases: &BTreeSet<String>) -> Vec<usize> {
    tokens
        .windows(4)
        .filter(|window| {
            aliases.contains(&window[0].text)
                && window[1].text == ":"
                && window[2].text == ":"
                && window[3].text == "Socket"
        })
        .map(|window| window[0].offset)
        .collect()
}

fn construction_findings(source: &str, allowed_ranges: &[(usize, usize)]) -> Vec<usize> {
    let tokens = lex_rust(source);
    let aliases = aliases(&tokens);
    socket_construction_offsets(&tokens, &aliases)
        .into_iter()
        .filter(|offset| {
            !allowed_ranges
                .iter()
                .any(|(start, end)| *start <= *offset && *offset < *end)
        })
        .collect()
}

#[test]
fn socket_variant_is_constructed_only_by_the_ay9_factory_or_tests() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    rust_sources(&manifest.join("src"), &mut sources);
    rust_sources(&manifest.join("tests"), &mut sources);

    for source in sources {
        let contents = fs::read_to_string(&source).expect("Rust source");
        let is_integration_test = source.starts_with(manifest.join("tests"));
        let is_transport_source = source
            .file_name()
            .is_some_and(|name| name == "transport_socket.rs");
        let is_factory_source = source
            .file_name()
            .is_some_and(|name| name == "transport.rs");
        let markers = if is_transport_source {
            vec!["#[cfg(test)]"]
        } else if is_factory_source {
            vec!["pub(crate) fn from_config", "#[cfg(test)]"]
        } else if source.file_name().is_some_and(|name| name == "lib.rs") {
            vec!["#[cfg(feature = \"test-utils\")]"]
        } else {
            Vec::new()
        };
        let tokens = lex_rust(&contents);
        let ranges = if is_integration_test {
            vec![(0, contents.len())]
        } else {
            brace_ranges(&tokens, &contents, &markers)
        };
        let findings = construction_findings(&contents, &ranges);
        assert!(
            findings.is_empty(),
            "socket construction escaped the AY.9 production factory or test scope in {} at offsets {findings:?}",
            source.display()
        );
    }
}

#[test]
fn construction_pin_detects_aliases_and_variant_function_pointers() {
    let source = r#"
        use crate::transport::HerdrIo as Io;
        type Alias = HerdrIo;
        fn leaked(socket: SocketIo) {
            let _ = Io::Socket(socket);
            let constructor = Alias::Socket;
            let _ = constructor;
        }
    "#;
    assert_eq!(construction_findings(source, &[]).len(), 2);
    assert!(
        construction_findings(
            "// HerdrIo::Socket(fake)\nconst NOTE: &str = \"HerdrIo::Socket(fake)\";",
            &[]
        )
        .is_empty()
    );
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
