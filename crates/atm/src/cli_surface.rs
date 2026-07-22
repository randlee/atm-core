//! Introspection helpers that walk the live [`clap::Command`] tree produced
//! by [`crate::commands::Cli`] and render it either as canonical
//! machine-readable JSON or as human-readable Markdown documentation.
//!
//! Both renderers walk the exact same [`clap::Command`] structure returned by
//! [`clap::CommandFactory::command`], so they can never drift from the real
//! parser: there is no separate hand-maintained flag list to keep in sync.
//! Adding, removing, or renaming a subcommand or argument anywhere in
//! `crate::commands` is immediately visible to both outputs the next time
//! they are regenerated.
//!
//! This module backs two maintainer-facing tools through the hidden parsed
//! `atm __dump-cli-surface --format <json|markdown>` subcommand:
//!
//! - `atm __dump-cli-surface --format json` prints [`command_surface_json`] output,
//!   consumed by `crates/atm/tests/cli_surface.rs` and used to regenerate
//!   `crates/atm/tests/cli_surface_baseline.json`.
//! - `atm __dump-cli-surface --format markdown` prints [`command_surface_markdown`]
//!   output, used to regenerate the version-suffixed `docs/atm/cli-reference-<version>.md`.
//!
//! The command is hidden from normal help but still uses the normal parse,
//! tracing, and observability bootstrap path. It is invoked by
//! `crates/atm/examples/gen_cli_docs.rs` and the CLI-surface diff test.

use clap::{Arg, Command};
use serde_json::{Value, json};

/// Clap auto-injects `--help`/`--version` (and their short forms) on every
/// command. These are parser plumbing, not CLI-surface content owned by
/// `atm`, so both renderers exclude them from their output.
fn is_auto_injected(arg: &Arg) -> bool {
    matches!(arg.get_id().as_str(), "help" | "version")
}

/// Walks `command` (and all subcommands, recursively) into canonical JSON.
///
/// Only structural properties are captured: argument identity, long/short
/// flags, requiredness, arity, and default-value presence. Help-text prose
/// is deliberately omitted so that wording-only changes never trigger a
/// false-positive diff against the committed baseline.
///
/// Both the argument list and the subcommand list are sorted by name so the
/// output is stable across runs regardless of declaration order, which keeps
/// diffs focused on real additions, removals, and renames.
pub(crate) fn command_surface_json(command: &Command) -> Value {
    let mut args: Vec<Value> = command
        .get_arguments()
        .filter(|arg| !is_auto_injected(arg))
        .map(arg_surface_json)
        .collect();
    args.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    let mut subcommands: Vec<Value> = command
        .get_subcommands()
        .map(command_surface_json)
        .collect();
    subcommands.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

    json!({
        "name": command.get_name(),
        "args": args,
        "subcommands": subcommands,
    })
}

fn arg_surface_json(arg: &Arg) -> Value {
    let num_args = arg.get_num_args().map(|range| {
        json!({
            "min": range.min_values(),
            "max": range.max_values(),
        })
    });

    json!({
        "id": arg.get_id().as_str(),
        "long": arg.get_long(),
        "short": arg.get_short().map(|c| c.to_string()),
        "required": arg.is_required_set(),
        "num_args": num_args,
        "has_default": !arg.get_default_values().is_empty(),
    })
}

/// Renders `command` (recursively) as a customer-facing Markdown CLI
/// reference, including descriptions and full argument tables.
///
/// Unlike [`command_surface_json`], this renderer intentionally includes
/// help-text prose (`about`/`long_about`/per-argument help and `after_help`
/// notes) — that prose is exactly what a human reader needs and is exactly
/// what the JSON diff gate excludes to avoid false positives.
pub(crate) fn command_surface_markdown(command: &Command) -> String {
    let mut out = String::new();
    out.push_str("# ATM CLI Reference\n\n");
    out.push_str(
        "This document is generated from the live `clap` command tree. Do \
         not hand-edit it — regenerate with `cargo run -p agent-team-mail \
         --example gen_cli_docs` (see `crates/atm/src/cli_surface.rs`).\n\n",
    );
    render_command_markdown(command, 2, &mut out, "atm");
    out
}

fn render_command_markdown(
    command: &Command,
    heading_level: usize,
    out: &mut String,
    full_name: &str,
) {
    let heading = "#".repeat(heading_level.min(6));
    out.push_str(&format!("{heading} `{full_name}`\n\n"));

    if let Some(about) = command.get_long_about().or_else(|| command.get_about()) {
        out.push_str(&about.to_string());
        out.push_str("\n\n");
    }

    let args: Vec<&Arg> = command
        .get_arguments()
        .filter(|arg| !is_auto_injected(arg))
        .collect();

    if !args.is_empty() {
        out.push_str("| Flag | Short | Required | Description |\n");
        out.push_str("|------|-------|----------|-------------|\n");
        for arg in &args {
            let long = arg
                .get_long()
                .map(|long| format!("`--{long}`"))
                .unwrap_or_else(|| format!("`<{}>`", arg.get_id()));
            let short = arg
                .get_short()
                .map(|short| format!("`-{short}`"))
                .unwrap_or_default();
            let required = if arg.is_required_set() { "yes" } else { "no" };
            let description = arg
                .get_help()
                .map(|help| help.to_string().replace('\n', " "))
                .unwrap_or_default();
            out.push_str(&format!(
                "| {long} | {short} | {required} | {description} |\n"
            ));
        }
        out.push('\n');
    }

    if let Some(after_help) = command
        .get_after_help()
        .or_else(|| command.get_after_long_help())
    {
        out.push_str("**Notes:**\n\n");
        out.push_str(&after_help.to_string());
        out.push_str("\n\n");
    }

    // Hidden subcommands (e.g. `internal-nudge`, used only for daemon/CLI
    // internal plumbing) are intentionally absent from `--help` and are not
    // part of the customer-facing surface this document describes. The JSON
    // structural diff in `command_surface_json` still tracks them.
    let mut subcommands: Vec<&Command> = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .collect();
    subcommands.sort_by_key(|sub| sub.get_name().to_string());
    for sub in subcommands {
        let child_full_name = format!("{full_name} {}", sub.get_name());
        render_command_markdown(sub, (heading_level + 1).min(6), out, &child_full_name);
    }
}

#[cfg(test)]
mod tests {
    use clap::{Arg, Command};

    use super::{command_surface_json, command_surface_markdown};

    fn sample_command() -> Command {
        Command::new("sample")
            .about("Sample root command.")
            .arg(Arg::new("help").long("help").short('h'))
            .arg(
                Arg::new("name")
                    .long("name")
                    .short('n')
                    .required(true)
                    .help("The name to greet."),
            )
            .subcommand(
                Command::new("child")
                    .about("Sample child command.")
                    .arg(Arg::new("count").long("count").default_value("1")),
            )
    }

    #[test]
    fn json_surface_excludes_auto_injected_help_and_version() {
        let surface = command_surface_json(&sample_command());
        let args = surface["args"].as_array().expect("args array");
        assert_eq!(args.len(), 1, "help/version args must be excluded");
        assert_eq!(args[0]["id"], "name");
        assert_eq!(args[0]["required"], true);
    }

    #[test]
    fn json_surface_recurses_into_subcommands() {
        let surface = command_surface_json(&sample_command());
        let subcommands = surface["subcommands"]
            .as_array()
            .expect("subcommands array");
        assert_eq!(subcommands.len(), 1);
        assert_eq!(subcommands[0]["name"], "child");
        let child_args = subcommands[0]["args"].as_array().expect("child args array");
        assert_eq!(child_args[0]["id"], "count");
        assert_eq!(child_args[0]["has_default"], true);
    }

    #[test]
    fn markdown_surface_includes_help_text_and_tables() {
        let markdown = command_surface_markdown(&sample_command());
        assert!(markdown.contains("Sample root command."));
        assert!(markdown.contains("The name to greet."));
        assert!(markdown.contains("`--name`"));
        assert!(markdown.contains("### `atm child`"));
    }
}
