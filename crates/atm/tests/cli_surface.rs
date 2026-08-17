//! Diff-gate test proving the `atm` CLI's public surface (subcommands,
//! arguments, flags) only ever grows across a release.
//!
//! This walks the *live* `clap::Command` tree of the real `atm` binary (via
//! the hidden `__dump-cli-surface --format json` command documented in
//! `crates/atm/src/cli_surface.rs` and `crates/atm/src/main.rs` — not by
//! parsing `--help` text) and diffs it structurally against the committed
//! `cli_surface_baseline.json`. It hard-fails on:
//!
//! - any removed subcommand,
//! - any removed or renamed argument (`id`, `long`, or `short`),
//! - any argument whose `required` or arity (`num_args`) changed,
//! - any **new** subcommand or argument not yet reflected in the baseline.
//!
//! That last point is deliberate: additions are allowed, but the baseline
//! must be updated *in the same commit* that adds the new surface, not
//! silently drift. Regenerate the baseline with:
//!
//! ```text
//! UPDATE_ATM_CLI_SURFACE_BASELINE=1 cargo test -p agent-team-mail --test cli_surface
//! ```
//!
//! or via `cargo run -p agent-team-mail --example gen_cli_docs`, which
//! regenerates both this baseline and the version-suffixed
//! `docs/atm/cli-reference-<version>.md` from the same live tree in one
//! step. No established bless/regen convention exists
//! elsewhere in this repo (searched for `bless`/`UPDATE_*` env vars in
//! existing golden-file tests and found none), so this follows the common
//! Rust ecosystem `UPDATE_<THING>=1` pattern (cf. `UPDATE_EXPECT`,
//! `INSTA_UPDATE`).

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

const BASELINE_PATH_COMPONENTS: &str = "tests/cli_surface_baseline.json";
const BLESS_ENV: &str = "ATM_CLI_SURFACE_BLESS";

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(BASELINE_PATH_COMPONENTS)
}

/// Invokes the real `atm` binary's hidden parsed CLI-surface JSON command and
/// parses the result. This uses the normal parser/bootstrap path and the exact
/// same [`clap::Command`] tree `atm` uses at runtime.
fn live_surface_json() -> Value {
    let atm_bin = env!("CARGO_BIN_EXE_atm");
    let output = Command::new(atm_bin)
        .args(["__dump-cli-surface", "--format", "json"])
        .output()
        .expect("failed to run `atm` for CLI-surface introspection");
    assert!(
        output.status.success(),
        "`atm` CLI-surface dump exited with {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout =
        String::from_utf8(output.stdout).expect("CLI-surface dump output must be valid UTF-8");
    serde_json::from_str(&stdout).expect("CLI-surface dump output must be valid JSON")
}

#[test]
fn legacy_environment_variable_cannot_bypass_clap_parsing() {
    let atm_bin = env!("CARGO_BIN_EXE_atm");
    let output = Command::new(atm_bin)
        .env("ATM_CLI_SURFACE_DUMP", "json")
        .arg("--version")
        .output()
        .expect("failed to run `atm --version`");

    assert!(
        output.status.success(),
        "`atm --version` exited with {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("version output must be valid UTF-8");
    assert!(
        stdout.starts_with("atm "),
        "legacy environment variable must not replace parsed --version output: {stdout:?}"
    );
}

fn as_str<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field]
        .as_str()
        .unwrap_or_else(|| panic!("expected string field {field:?} in {value}"))
}

fn as_array<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value[field]
        .as_array()
        .unwrap_or_else(|| panic!("expected array field {field:?} in {value}"))
}

/// Recursively diffs `baseline` against `live`, appending human-readable
/// failure descriptions to `issues`. `path` tracks the current command path
/// (e.g. `atm teams add-member`) for readable messages.
fn diff_command(
    path: &str,
    baseline: &Value,
    live: &Value,
    breaking: &mut Vec<String>,
    additions: &mut Vec<String>,
) {
    diff_args(
        path,
        as_array(baseline, "args"),
        as_array(live, "args"),
        breaking,
        additions,
    );
    diff_subcommands(
        path,
        as_array(baseline, "subcommands"),
        as_array(live, "subcommands"),
        breaking,
        additions,
    );
}

fn diff_args(
    path: &str,
    baseline_args: &[Value],
    live_args: &[Value],
    breaking: &mut Vec<String>,
    additions: &mut Vec<String>,
) {
    for baseline_arg in baseline_args {
        let id = as_str(baseline_arg, "id");
        match live_args.iter().find(|arg| as_str(arg, "id") == id) {
            None => breaking.push(format!("{path}: argument {id:?} was removed or renamed")),
            Some(live_arg) => {
                for field in ["long", "short", "required", "num_args", "has_default"] {
                    if baseline_arg[field] != live_arg[field] {
                        breaking.push(format!(
                            "{path}: argument {id:?} field {field:?} changed: {:?} -> {:?}",
                            baseline_arg[field], live_arg[field]
                        ));
                    }
                }
            }
        }
    }

    for live_arg in live_args {
        let id = as_str(live_arg, "id");
        if !baseline_args.iter().any(|arg| as_str(arg, "id") == id) {
            additions.push(format!(
                "{path}: argument {id:?} is new and not yet reflected in the baseline \
                 (regenerate with `cargo run -p agent-team-mail --example gen_cli_docs`)"
            ));
        }
    }
}

fn diff_subcommands(
    path: &str,
    baseline_subcommands: &[Value],
    live_subcommands: &[Value],
    breaking: &mut Vec<String>,
    additions: &mut Vec<String>,
) {
    for baseline_sub in baseline_subcommands {
        let name = as_str(baseline_sub, "name");
        let child_path = format!("{path} {name}");
        match live_subcommands
            .iter()
            .find(|sub| as_str(sub, "name") == name)
        {
            None => breaking.push(format!(
                "{path}: subcommand {name:?} was removed or renamed"
            )),
            Some(live_sub) => {
                diff_command(&child_path, baseline_sub, live_sub, breaking, additions)
            }
        }
    }

    for live_sub in live_subcommands {
        let name = as_str(live_sub, "name");
        if !baseline_subcommands
            .iter()
            .any(|sub| as_str(sub, "name") == name)
        {
            additions.push(format!(
                "{path}: subcommand {name:?} is new and not yet reflected in the baseline \
                 (regenerate with `cargo run -p agent-team-mail --example gen_cli_docs`)"
            ));
        }
    }
}

#[test]
fn cli_surface_matches_committed_baseline() {
    let live = live_surface_json();

    let baseline_raw = std::fs::read_to_string(baseline_path()).unwrap_or_else(|error| {
        panic!(
            "failed to read {}: {error} (generate it first with \
             `cargo run -p agent-team-mail --example gen_cli_docs`)",
            baseline_path().display()
        )
    });
    let baseline: Value =
        serde_json::from_str(&baseline_raw).expect("baseline CLI-surface JSON must parse");

    assert_eq!(
        as_str(&baseline, "name"),
        as_str(&live, "name"),
        "root command name changed"
    );

    let mut breaking = Vec::new();
    let mut additions = Vec::new();
    diff_command("atm", &baseline, &live, &mut breaking, &mut additions);
    assert!(
        breaking.is_empty(),
        "ATM CLI baseline update is additions-only; refusing removed, renamed, or changed entries:\n{}",
        breaking.join("\n")
    );

    if std::env::var_os(BLESS_ENV).is_some() {
        let pretty = serde_json::to_string_pretty(&live).expect("serialize live CLI surface");
        std::fs::write(baseline_path(), format!("{pretty}\n"))
            .expect("write regenerated CLI-surface baseline");
        eprintln!(
            "{BLESS_ENV} set: wrote {} — re-run without {BLESS_ENV} to verify the diff gate",
            baseline_path().display()
        );
        return;
    }

    assert!(
        additions.is_empty(),
        "atm CLI surface diverged from crates/atm/tests/cli_surface_baseline.json:\n{}\n\n\
         If this is an intentional, reviewed addition, regenerate the baseline in the same \
         commit with `cargo run -p agent-team-mail --example gen_cli_docs` (or \
         `{BLESS_ENV}=1 cargo test -p agent-team-mail --test cli_surface`).",
        additions.join("\n")
    );
}
