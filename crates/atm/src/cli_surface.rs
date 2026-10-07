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
//!   output, used to regenerate the installed CLI manual and website reference.
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
    out.push_str(&format!("Version: `{}`\n\n", env!("CARGO_PKG_VERSION")));
    out.push_str(
        "This document is generated from the live `clap` command tree. Do \
         not hand-edit it — regenerate with `cargo run -p agent-team-mail \
         --features cli-surface-dump --example gen_cli_docs` \
         (see `crates/atm/src/cli_surface.rs`).\n\n",
    );
    render_command_markdown(command, 2, &mut out, "atm");
    out
}

/// Renders the same generated reference as a standalone HTML page for the
/// published website. Keeping the Markdown as the single rendered body means
/// the two delivery surfaces cannot acquire independent option lists.
pub(crate) fn command_surface_html(command: &Command) -> String {
    let mut out = "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>ATM CLI Reference</title><style>body{font:16px system-ui,sans-serif;line-height:1.5;max-width:80rem;margin:3rem auto;padding:0 1rem}table{border-collapse:collapse;width:100%}th,td{border:1px solid #888;padding:.4rem;text-align:left;vertical-align:top}code{white-space:nowrap}nav ul{columns:3}</style></head><body><main>\n".to_owned();
    out.push_str(&format!("<h1>ATM CLI Reference</h1><p>Version: <code>{}</code></p><p>This page is generated from the live Clap command tree. Do not hand-edit it.</p><nav aria-label=\"Command index\"><h2>Command index</h2><ul>", env!("CARGO_PKG_VERSION")));
    let mut index = Vec::new();
    collect_command_index(command, "atm", &mut index);
    for (name, id) in &index {
        out.push_str(&format!(
            "<li><a href=\"#{id}\"><code>{}</code></a></li>",
            html_escape(name)
        ));
    }
    out.push_str("</ul></nav>\n");
    render_command_html(command, 2, &mut out, "atm");
    out.push_str("</main></body></html>\n");
    out
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn command_anchor(full_name: &str) -> String {
    full_name.replace(' ', "-")
}

fn public_subcommands(command: &Command) -> Vec<&Command> {
    let mut subcommands: Vec<&Command> = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .collect();
    subcommands.sort_by_key(|sub| sub.get_name().to_string());
    subcommands
}

fn collect_command_index(command: &Command, full_name: &str, index: &mut Vec<(String, String)>) {
    index.push((full_name.to_owned(), command_anchor(full_name)));
    for sub in public_subcommands(command) {
        collect_command_index(sub, &format!("{full_name} {}", sub.get_name()), index);
    }
}

fn render_command_html(command: &Command, level: usize, out: &mut String, full_name: &str) {
    let heading = level.min(6);
    out.push_str(&format!(
        "<section><h{heading} id=\"{}\"><code>{}</code></h{heading}>",
        command_anchor(full_name),
        html_escape(full_name)
    ));
    if let Some(about) = command.get_long_about().or_else(|| command.get_about()) {
        out.push_str(&format!("<p>{}</p>", html_escape(&about.to_string())));
    }
    let mut usage = command.clone();
    out.push_str(&format!(
        "<h{}>Usage</h{}><pre>{}</pre>",
        (heading + 1).min(6),
        (heading + 1).min(6),
        html_escape(&usage.render_usage().to_string())
    ));
    let args: Vec<&Arg> = command
        .get_arguments()
        .filter(|arg| !is_auto_injected(arg))
        .collect();
    if !args.is_empty() {
        out.push_str("<table><thead><tr><th>Flag</th><th>Short</th><th>Value</th><th>Required</th><th>Default</th><th>Allowed values</th><th>Description</th></tr></thead><tbody>");
        for arg in args {
            let flag = arg
                .get_long()
                .map(|long| format!("--{long}"))
                .unwrap_or_else(|| format!("<{}>", arg.get_id()));
            let short = arg
                .get_short()
                .map(|short| format!("-{short}"))
                .unwrap_or_default();
            let value = arg
                .get_value_names()
                .map(|names| {
                    names
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let default = arg
                .get_default_values()
                .first()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            let allowed = arg
                .get_possible_values()
                .iter()
                .filter(|value| !value.is_hide_set())
                .map(|value| value.get_name())
                .collect::<Vec<_>>()
                .join(", ");
            let description = arg
                .get_help()
                .map(|help| help.to_string())
                .unwrap_or_default();
            out.push_str(&format!("<tr><td><code>{}</code></td><td><code>{}</code></td><td><code>{}</code></td><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>", html_escape(&flag), html_escape(&short), html_escape(&value), if arg.is_required_set() { "yes" } else { "no" }, html_escape(&default), html_escape(&allowed), html_escape(&description)));
        }
        out.push_str("</tbody></table>");
    }
    for sub in public_subcommands(command) {
        render_command_html(
            sub,
            heading + 1,
            out,
            &format!("{full_name} {}", sub.get_name()),
        );
    }
    out.push_str("</section>");
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

    let mut usage = command.clone();
    out.push_str("**Usage:**\n\n```text\n");
    out.push_str(&usage.render_usage().to_string());
    out.push_str("\n```\n\n");

    let args: Vec<&Arg> = command
        .get_arguments()
        .filter(|arg| !is_auto_injected(arg))
        .collect();

    if !args.is_empty() {
        out.push_str(
            "| Flag | Short | Value | Required | Default | Allowed values | Description |\n",
        );
        out.push_str(
            "|------|-------|-------|----------|---------|----------------|-------------|\n",
        );
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
            let value = arg
                .get_value_names()
                .map(|names| {
                    names
                        .iter()
                        .map(|name| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let default = arg
                .get_default_values()
                .first()
                .map(|value| format!("`{}`", value.to_string_lossy()))
                .unwrap_or_default();
            let allowed_values = arg
                .get_possible_values()
                .iter()
                .filter(|value| !value.is_hide_set())
                .map(|value| format!("`{}`", value.get_name()))
                .collect::<Vec<_>>()
                .join(", ");
            let description = arg
                .get_help()
                .map(|help| help.to_string().replace(['\n', '|'], " "))
                .unwrap_or_default();
            out.push_str(&format!(
                "| {long} | {short} | {value} | {required} | {default} | {allowed_values} | {description} |\n"
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

    use super::{command_surface_html, command_surface_json, command_surface_markdown};

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
        assert!(markdown.contains("Usage:"));
        assert!(markdown.contains("Default"));
        assert!(markdown.contains("### `atm child`"));
    }

    #[test]
    fn html_reference_wraps_the_markdown_without_an_independent_surface() {
        let html = command_surface_html(&sample_command());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("ATM CLI Reference"));
        assert!(html.contains("&lt;name&gt;"));
    }
}
