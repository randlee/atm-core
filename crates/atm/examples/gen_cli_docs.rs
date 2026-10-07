//! Regenerates the committed CLI-surface diff-gate baseline and the
//! customer-facing CLI reference doc from the live `atm` binary.
//!
//! `crates/atm` ships only a `[[bin]]` target (no library), so this example
//! cannot import `commands::Cli` directly. Instead it builds (if needed) and
//! shells out to the sibling `atm` binary with the hidden parsed
//! `__dump-cli-surface` subcommand (see `crates/atm/src/cli_surface.rs` and
//! `crates/atm/src/main.rs`), which walks the live `clap::Command` tree and
//! prints canonical output through the normal CLI bootstrap path. This keeps a
//! single source of truth for the walk/render logic — this example is just a
//! thin driver that writes the two outputs to disk.
//!
//! # Usage
//!
//! ```text
//! cargo run -p agent-team-mail --features cli-surface-dump --example gen_cli_docs
//! ```
//!
//! This regenerates:
//! - `crates/atm/tests/cli_surface_baseline.json` (consumed by the
//!   `cli_surface` diff-gate integration test)
//! - `docs/user-documents/cli-reference.md` (the installed, versioned manual)
//! - `site/cli-reference.html` (the matching published website page)
//!
//! Pass `--check` to reject missing or stale generated output without writing.

use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> PathBuf {
    manifest_dir()
        .parent()
        .and_then(Path::parent)
        .expect("crates/atm has a workspace root two directories up")
        .to_path_buf()
}

/// Builds and locates the sibling `atm` binary alongside this example's own
/// executable, matching this example's profile.
///
/// The example does not link the binary target as a Cargo dependency, so an
/// existing binary could otherwise be older than the source being snapshotted.
fn ensure_atm_binary_built() -> PathBuf {
    let mut profile_dir = std::env::current_exe().expect("resolve current example executable path");
    profile_dir.pop(); // drop the example executable's own file name
    if profile_dir
        .file_name()
        .is_some_and(|name| name == "examples")
    {
        profile_dir.pop(); // target/<profile>/examples -> target/<profile>
    }

    let bin_name = if cfg!(windows) { "atm.exe" } else { "atm" };
    let bin_path = profile_dir.join(bin_name);
    let release = profile_dir
        .file_name()
        .is_some_and(|name| name == "release");
    let mut build = Command::new(env!("CARGO"));
    build.args(["build", "--bin", "atm", "--features", "cli-surface-dump"]);
    if release {
        build.arg("--release");
    }
    build.current_dir(manifest_dir());

    let status = build
        .status()
        .expect("invoke `cargo build --bin atm` to produce the introspection target");
    assert!(status.success(), "cargo build --bin atm failed");
    assert!(
        bin_path.is_file(),
        "expected atm binary at {} after build",
        bin_path.display()
    );
    bin_path
}

fn dump(atm_bin: &Path, mode: &str) -> String {
    let output = Command::new(atm_bin)
        .args(["__dump-cli-surface", "--format", mode])
        .output()
        .unwrap_or_else(|error| panic!("failed to run {} ({mode}): {error}", atm_bin.display()));
    assert!(
        output.status.success(),
        "`atm` CLI-surface dump ({mode}) exited with {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("CLI-surface dump output must be valid UTF-8")
}

fn write_or_check(path: &Path, contents: &str, check: bool) {
    if check {
        match std::fs::read_to_string(path) {
            Ok(current) if current == contents => return,
            Ok(_) => panic!(
                "generated CLI reference is stale: {}; rerun gen_cli_docs",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                panic!(
                    "generated CLI reference is missing: {}; rerun gen_cli_docs",
                    path.display()
                )
            }
            Err(error) => panic!("failed to read {}: {error}", path.display()),
        }
    }
    std::fs::write(path, contents)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let check = match arguments.as_slice() {
        [] => false,
        [argument] if argument == "--check" => true,
        _ => panic!("usage: gen_cli_docs [--check]"),
    };
    let atm_bin = ensure_atm_binary_built();

    let json = dump(&atm_bin, "json");
    let baseline_path = manifest_dir().join("tests/cli_surface_baseline.json");
    write_or_check(&baseline_path, &json, check);

    let markdown = dump(&atm_bin, "markdown");
    let doc_path = workspace_root().join("docs/user-documents/cli-reference.md");
    write_or_check(&doc_path, &markdown, check);

    let html = dump(&atm_bin, "html");
    let site_path = workspace_root().join("site/cli-reference.html");
    write_or_check(&site_path, &html, check);
}
