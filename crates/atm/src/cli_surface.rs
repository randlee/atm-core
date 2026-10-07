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
//! `atm __dump-cli-surface --format <json|markdown|html>` subcommand:
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
use serde::Serialize;
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

/// Lists public CLI arguments that have no non-empty help text.
///
/// This follows the same visibility rules as the customer-facing renderers:
/// clap's auto-injected flags, hidden arguments, and hidden subcommands are
/// intentionally absent. Each returned value identifies the command and
/// argument so the CI gate can name the source surface that needs help.
pub(crate) fn undocumented_public_args(command: &Command) -> Vec<String> {
    let mut missing = Vec::new();
    collect_undocumented_public_args(command, command.get_name(), &mut missing);
    missing.sort();
    missing
}

fn collect_undocumented_public_args(command: &Command, path: &str, missing: &mut Vec<String>) {
    for arg in command
        .get_arguments()
        .filter(|arg| !is_auto_injected(arg) && !arg.is_hide_set())
    {
        let has_help = arg
            .get_help()
            .is_some_and(|help| !help.to_string().trim().is_empty());
        if !has_help {
            let display = arg
                .get_long()
                .map(|long| format!("--{long}"))
                .unwrap_or_else(|| format!("<{}>", arg.get_id()));
            missing.push(format!("{path} {display}"));
        }
    }

    for subcommand in public_subcommands(command) {
        let subcommand_path = format!("{path} {}", subcommand.get_name());
        collect_undocumented_public_args(subcommand, &subcommand_path, missing);
    }
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
    let document = command_document(command, "atm");
    let mut out = String::new();
    out.push_str("# ATM CLI Reference\n\n");
    out.push_str(&format!("Version: `{}`\n\n", env!("CARGO_PKG_VERSION")));
    out.push_str(
        "This document is generated from the live `clap` command tree. Do \
         not hand-edit it — regenerate with `cargo run -p agent-team-mail \
         --features cli-surface-dump --example gen_cli_docs` \
         (see `crates/atm/src/cli_surface.rs`).\n\n",
    );
    render_command_markdown(&document, 2, &mut out);
    out
}

/// Renders a standalone HTML page from the same Clap-derived document model
/// consumed by the Markdown renderer.
pub(crate) fn command_surface_html(command: &Command) -> String {
    let document = command_document(command, "atm");
    let data = serde_json::to_string(&document)
        .expect("CLI reference document model must serialize to JSON")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    include_str!("../assets/cli-reference/template.html")
        .replace("{{VERSION}}", env!("CARGO_PKG_VERSION"))
        .replace("{{CLI_REFERENCE_DATA}}", &data)
}

fn command_anchor(full_name: &str) -> String {
    full_name
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn public_subcommands(command: &Command) -> Vec<&Command> {
    let mut subcommands: Vec<&Command> = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .collect();
    subcommands.sort_by_key(|sub| sub.get_name().to_string());
    subcommands
}

#[derive(Clone, Serialize)]
struct ArgDoc {
    anchor: String,
    flag: String,
    short: String,
    value: String,
    required: bool,
    default: String,
    allowed: String,
    description: String,
    name: String,
    brief: String,
}
#[derive(Clone, Serialize)]
struct CommandDoc {
    name: String,
    parent: Option<String>,
    anchor: String,
    about: String,
    long_about: String,
    usage: String,
    args: Vec<ArgDoc>,
    notes: String,
    children: Vec<CommandDoc>,
    brief: String,
    description: String,
    usages: Vec<String>,
    arguments: Vec<ArgDoc>,
    categories: Vec<OptionCategory>,
}

#[derive(Clone, Serialize)]
struct OptionCategory {
    title: String,
    options: Vec<ArgDoc>,
}

fn command_document(command: &Command, name: &str) -> CommandDoc {
    command_document_with_parent(command, name, None)
}

fn command_document_with_parent(
    command: &Command,
    name: &str,
    parent: Option<String>,
) -> CommandDoc {
    let mut usage = command.clone();
    let args = command_arguments(command, name);
    let children = command_children(command, name);
    let about = command
        .get_about()
        .map(ToString::to_string)
        .unwrap_or_default();
    let long_about = command
        .get_long_about()
        .map(ToString::to_string)
        .unwrap_or_default();
    CommandDoc {
        name: name.to_owned(),
        parent,
        anchor: command_anchor(name),
        about: about.clone(),
        long_about: long_about.clone(),
        usage: usage.render_usage().to_string(),
        args: args.clone(),
        notes: command_notes(command),
        children,
        brief: about,
        description: long_about,
        usages: vec![usage.render_usage().to_string()],
        arguments: args.clone(),
        categories: vec![OptionCategory {
            title: "Options".to_owned(),
            options: args,
        }],
    }
}

fn command_arguments(command: &Command, name: &str) -> Vec<ArgDoc> {
    command
        .get_arguments()
        .filter(|arg| !is_auto_injected(arg) && !arg.is_hide_set())
        .enumerate()
        .map(|(index, arg)| {
            // Match clap's full `--help` rendering: a doc comment's first
            // paragraph is brief help, while later paragraphs are long help.
            // The generated manual is the long-form reference, so preserve
            // that complete argument description when it is available.
            let description = arg
                .get_long_help()
                .or_else(|| arg.get_help())
                .map(ToString::to_string)
                .unwrap_or_default();
            let flag = arg
                .get_long()
                .map(|long| format!("--{long}"))
                .unwrap_or_else(|| format!("<{}>", arg.get_id()));
            ArgDoc {
                anchor: format!("{}-arg-{index}", command_anchor(name)),
                flag: flag.clone(),
                short: arg
                    .get_short()
                    .map(|short| format!("-{short}"))
                    .unwrap_or_default(),
                value: arg
                    .get_value_names()
                    .map(|names| {
                        names
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default(),
                required: arg.is_required_set(),
                default: arg
                    .get_default_values()
                    .first()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_default(),
                allowed: arg
                    .get_possible_values()
                    .iter()
                    .filter(|value| !value.is_hide_set())
                    .map(|value| value.get_name())
                    .collect::<Vec<_>>()
                    .join(", "),
                description: description.clone(),
                name: flag,
                brief: description,
            }
        })
        .collect()
}

fn command_children(command: &Command, name: &str) -> Vec<CommandDoc> {
    public_subcommands(command)
        .into_iter()
        .map(|sub| {
            command_document_with_parent(
                sub,
                &format!("{name} {}", sub.get_name()),
                Some(name.to_owned()),
            )
        })
        .collect()
}

fn command_notes(command: &Command) -> String {
    command
        .get_after_help()
        .or_else(|| command.get_after_long_help())
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn render_command_markdown(command: &CommandDoc, heading_level: usize, out: &mut String) {
    let heading = "#".repeat(heading_level.min(6));
    out.push_str(&format!("{heading} `{}`\n\n", command.name));

    if !command.about.is_empty() {
        out.push_str(&command.about);
        out.push_str("\n\n");
    }

    out.push_str("**Usage:**\n\n```text\n");
    out.push_str(&command.usage);
    out.push_str("\n```\n\n");

    if !command.args.is_empty() {
        out.push_str(
            "| Flag | Short | Value | Required | Default | Allowed values | Description |\n",
        );
        out.push_str(
            "|------|-------|-------|----------|---------|----------------|-------------|\n",
        );
        for arg in &command.args {
            let long = format!("`{}`", arg.flag);
            let short = format!("`{}`", arg.short);
            let required = if arg.required { "yes" } else { "no" };
            let value = format!("`{}`", arg.value);
            let default = format!("`{}`", arg.default);
            let allowed_values = arg.allowed.clone();
            let description = arg.description.replace(['\n', '|'], " ");
            out.push_str(&format!(
                "| {long} | {short} | {value} | {required} | {default} | {allowed_values} | {description} |\n"
            ));
        }
        out.push('\n');
    }

    if !command.notes.is_empty() {
        out.push_str("**Notes:**\n\n");
        out.push_str(&command.notes);
        out.push_str("\n\n");
    }

    // Hidden subcommands (e.g. `internal-nudge`, used only for daemon/CLI
    // internal plumbing) are intentionally absent from `--help` and are not
    // part of the customer-facing surface this document describes. The JSON
    // structural diff in `command_surface_json` still tracks them.
    for sub in &command.children {
        render_command_markdown(sub, (heading_level + 1).min(6), out);
    }
}

#[cfg(test)]
mod tests {
    use clap::{Arg, Command};

    use super::{
        command_surface_html, command_surface_json, command_surface_markdown,
        undocumented_public_args,
    };

    fn sample_command() -> Command {
        Command::new("sample")
            .disable_help_flag(true)
            .about("Sample root command.")
            .arg(Arg::new("help").long("help").short('h'))
            .arg(
                Arg::new("name")
                    .long("name")
                    .short('n')
                    .required(true)
                    .help("The name to greet."),
            )
            .after_help("Sample notes.")
            .subcommand(
                Command::new("child").about("Sample child command.").arg(
                    Arg::new("count")
                        .long("count")
                        .default_value("1")
                        .value_parser(["1", "2"]),
                ),
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
    fn undocumented_public_args_excludes_hidden_surface_and_names_offenders() {
        let command = Command::new("sample")
            .arg(Arg::new("missing").long("missing"))
            .arg(Arg::new("hidden").long("hidden").hide(true))
            .subcommand(
                Command::new("hidden-command")
                    .hide(true)
                    .arg(Arg::new("also-missing")),
            )
            .subcommand(
                Command::new("visible-command")
                    .arg(Arg::new("documented").long("documented").help("Explained."))
                    .arg(Arg::new("missing-positional")),
            );

        assert_eq!(
            undocumented_public_args(&command),
            vec![
                "sample --missing".to_owned(),
                "sample visible-command <missing-positional>".to_owned(),
            ]
        );
    }

    #[test]
    fn markdown_surface_includes_help_text_and_tables() {
        let markdown = command_surface_markdown(&sample_command());
        assert!(markdown.contains("Sample root command."));
        assert!(markdown.contains("The name to greet."));
        assert!(markdown.contains("`--name`"));
        assert!(markdown.contains("Usage:"));
        assert!(markdown.contains("Default"));
        assert!(markdown.contains("Sample notes."));
        assert!(markdown.contains("### `atm child`"));
    }

    #[test]
    fn markdown_surface_prefers_long_argument_help() {
        let command = Command::new("sample").arg(
            Arg::new("name")
                .long("name")
                .help("Brief name help.")
                .long_help("Brief name help.\n\nDetailed name help."),
        );

        let markdown = command_surface_markdown(&command);
        assert!(markdown.contains("Detailed name help."));
    }

    #[test]
    fn html_reference_embeds_the_same_command_document_model() {
        let html = command_surface_html(&sample_command());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("ATM CLI Reference"));
        assert!(html.contains("cli-reference-data"));
        assert!(html.contains("\"allowed\":\"1, 2\""));
        assert!(html.contains("\"anchor\":\"61746d206368696c64\""));
        assert!(html.contains("Sample notes."));
    }
}
