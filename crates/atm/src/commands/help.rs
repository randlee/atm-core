use std::io::Cursor;

use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory};

use super::Cli;
use crate::observability::CliObservability;
use crate::output;
use crate::output_contract::{HelpResult, HelpResultKind, HelpTopicSummary, HelpTopicTier};

#[derive(Debug, Args)]
/// Show ATM-owned conceptual help or delegated clap subcommand help.
pub struct HelpCommand {
    #[arg()]
    target: Option<String>,

    #[arg(long, conflicts_with = "target")]
    list: bool,

    #[arg(long)]
    json: bool,
}

impl HelpCommand {
    /// Execute the `atm help` command.
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        let json = self.json;
        let result = self.render()?;
        output::print_help_result(&result, json)
    }

    fn render(&self) -> Result<HelpResult> {
        if self.list {
            return Ok(HelpResult::topic_list());
        }

        let Some(target) = self.target.as_deref() else {
            return Ok(HelpResult::overview());
        };

        if let Some(topic) = HelpTopic::parse(target) {
            return Ok(HelpResult::concept_topic(topic));
        }

        if let Some(body) = render_subcommand_help(target)? {
            return Ok(HelpResult::command_help(target, body));
        }

        bail!(
            "unknown help topic or subcommand `{target}`. Use `atm help --list` to inspect the available help targets."
        )
    }
}

impl HelpResult {
    fn overview() -> Self {
        let commands = top_level_command_names();
        let topics = help_topics();
        Self {
            kind: HelpResultKind::Overview,
            requested_target: None,
            title: "ATM Help".to_string(),
            body: format!(
                "\
ATM Help

Use `atm --help` for clap-generated command syntax.
Use `atm help --list` to inspect conceptual topics and command help targets.
Use `atm help <topic>` for ATM-owned conceptual guidance.
Use `atm help <subcommand>` for clap-generated command help.

Current runtime model:
- SQLite and the daemon own ATM durable mail and roster state.
- Shared inbox JSONL is a compatibility output surface, not ATM's mutable source of truth.
- General structured JSON input is out of scope for Phase Y and Phase Z.

Tier-1 concept topics:
- config
- errors

Tier-2 concept topics:
- hooks
- identity
- skills

Available commands:
- {}
",
                commands.join("\n- ")
            ),
            commands,
            topics,
        }
    }

    fn topic_list() -> Self {
        let commands = top_level_command_names();
        let topics = help_topics();
        let topic_lines = topics
            .iter()
            .map(|topic| {
                format!(
                    "- {} ({}): {}",
                    topic.name,
                    topic.tier.label(),
                    topic.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            kind: HelpResultKind::TopicList,
            requested_target: None,
            title: "ATM Help Targets".to_string(),
            body: format!(
                "\
ATM Help Targets

Concept topics:
{}

Commands:
- {}
",
                topic_lines,
                commands.join("\n- ")
            ),
            commands,
            topics,
        }
    }

    fn concept_topic(topic: HelpTopic) -> Self {
        let commands = top_level_command_names();
        let topics = help_topics();
        Self {
            kind: HelpResultKind::ConceptTopic,
            requested_target: Some(topic.name().to_string()),
            title: topic.title().to_string(),
            body: topic.body().to_string(),
            commands,
            topics,
        }
    }

    fn command_help(target: &str, body: String) -> Self {
        Self {
            kind: HelpResultKind::CommandHelp,
            requested_target: Some(target.to_string()),
            title: format!("ATM command help: {target}"),
            body,
            commands: top_level_command_names(),
            topics: help_topics(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpTopic {
    Config,
    Errors,
    Hooks,
    Identity,
    Skills,
}

impl HelpTopic {
    const ALL: [Self; 5] = [
        Self::Config,
        Self::Errors,
        Self::Hooks,
        Self::Identity,
        Self::Skills,
    ];

    fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|topic| topic.name() == normalized.as_str())
    }

    fn name(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Errors => "errors",
            Self::Hooks => "hooks",
            Self::Identity => "identity",
            Self::Skills => "skills",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Config => "ATM Help: config",
            Self::Errors => "ATM Help: errors",
            Self::Hooks => "ATM Help: hooks",
            Self::Identity => "ATM Help: identity",
            Self::Skills => "ATM Help: skills",
        }
    }

    fn tier(self) -> HelpTopicTier {
        match self {
            Self::Config | Self::Errors => HelpTopicTier::Tier1,
            Self::Hooks | Self::Identity | Self::Skills => HelpTopicTier::Tier2,
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::Config => "Where ATM reads local configuration and what remains host-scoped.",
            Self::Errors => {
                "How ATM reports typed failures and where to look when delivery degrades."
            }
            Self::Hooks => "What post-send hooks are for and what they are not allowed to replace.",
            Self::Identity => "How ATM resolves sender, actor, team, and harness-facing identity.",
            Self::Skills => "How repo-local skills shape agent execution around ATM work.",
        }
    }

    fn body(self) -> &'static str {
        match self {
            Self::Config => {
                "\
ATM Help: config

ATM reads local configuration from `.atm.toml` and the documented ATM host paths.
The daemon + SQLite release line keeps durable ATM mail and roster state in the
daemon-owned SQLite store, not in shared inbox JSON.

Use config to control:
- post-send hooks
- retained log behavior
- compatibility export sizing such as `[atm].claude_jsonl_body_export_max_bytes`

Config does not change the durable-truth rule:
- SQLite + daemon own ATM durable state
- shared inbox JSONL remains a compatibility output surface
"
            }
            Self::Errors => {
                "\
ATM Help: errors

ATM surfaces typed errors with stable ATM-owned error codes.
The CLI preserves those typed failures until render time instead of rewriting
them into ad hoc text.

When a command fails:
- read the reported ATM error code first
- use `atm doctor` for local runtime and observability diagnostics
- treat compatibility output failures differently from durable store failures

The daemon + SQLite line keeps durable truth in SQLite. Compatibility output
problems may degrade nudges or projections, but they do not redefine durable
ATM state.
"
            }
            Self::Hooks => {
                "\
ATM Help: hooks

Post-send hooks are ATM-owned automation that run after ATM processes a send.
They are for notification and integration side effects, not for replacing ATM's
durable store or command contract.

Current Y.1 status:
- hook semantics remain supported
- Y.2 will add more worked troubleshooting examples for hook authoring
"
            }
            Self::Identity => {
                "\
ATM Help: identity

ATM command identity is about the sending agent, the selected team, and the
resolved runtime destination. Harness and model are not the same thing.

Current Y.1 status:
- actor and team overrides remain documented through command help
- Y.2 will expand examples for override-driven troubleshooting and operator UX
"
            }
            Self::Skills => {
                "\
ATM Help: skills

Skills are repo-local execution instructions used by agent harnesses while they
work on ATM tasks. They are not part of ATM durable mail semantics.

Current Y.1 status:
- this topic exists to anchor the conceptual surface
- Y.2 will expand examples for skill-driven team workflows and operator usage
"
            }
        }
    }
}

fn help_topics() -> Vec<HelpTopicSummary> {
    HelpTopic::ALL
        .into_iter()
        .map(|topic| HelpTopicSummary {
            name: topic.name(),
            tier: topic.tier(),
            summary: topic.summary(),
        })
        .collect()
}

fn top_level_command_names() -> Vec<String> {
    Cli::command()
        .get_subcommands()
        .map(|command| command.get_name().to_string())
        .collect()
}

fn render_subcommand_help(target: &str) -> Result<Option<String>> {
    let command = Cli::command()
        .get_subcommands()
        .find(|command| command.get_name() == target)
        .cloned();

    let Some(mut command) = command else {
        return Ok(None);
    };

    let mut buffer = Cursor::new(Vec::new());
    command
        .write_long_help(&mut buffer)
        .context("failed to render clap help for subcommand")?;
    let rendered = String::from_utf8(buffer.into_inner())
        .context("clap help for subcommand was not valid UTF-8")?;
    Ok(Some(rendered))
}

#[cfg(test)]
mod tests {
    use super::{HelpCommand, HelpResultKind, HelpTopic, HelpTopicTier};

    #[test]
    fn overview_mentions_runtime_model() {
        let command = HelpCommand {
            target: None,
            list: false,
            json: false,
        };

        let result = command.render().expect("overview");

        assert_eq!(result.kind, HelpResultKind::Overview);
        assert!(
            result
                .body
                .contains("SQLite and the daemon own ATM durable mail")
        );
        assert!(
            result
                .body
                .contains("Shared inbox JSONL is a compatibility output surface")
        );
    }

    #[test]
    fn list_includes_topics_and_commands() {
        let command = HelpCommand {
            target: None,
            list: true,
            json: false,
        };

        let result = command.render().expect("list");

        assert_eq!(result.kind, HelpResultKind::TopicList);
        assert!(result.commands.iter().any(|command| command == "send"));
        assert!(
            result
                .topics
                .iter()
                .any(|topic| topic.name == "config" && topic.tier == HelpTopicTier::Tier1)
        );
    }

    #[test]
    fn concept_topics_are_case_insensitive() {
        assert_eq!(HelpTopic::parse("ConFiG"), Some(HelpTopic::Config));
        assert_eq!(HelpTopic::parse("ERRORS"), Some(HelpTopic::Errors));
    }

    #[test]
    fn subcommand_help_renders_clap_output() {
        let command = HelpCommand {
            target: Some("send".to_string()),
            list: false,
            json: false,
        };

        let result = command.render().expect("send help");

        assert_eq!(result.kind, HelpResultKind::CommandHelp);
        assert!(result.body.starts_with("Send one ATM mailbox message"));
        assert!(!result.body.is_empty());
    }

    #[test]
    fn unknown_target_returns_error() {
        let command = HelpCommand {
            target: Some("not-a-real-target".to_string()),
            list: false,
            json: false,
        };

        let error = command.render().expect_err("unknown target should fail");

        assert!(
            error
                .to_string()
                .contains("unknown help topic or subcommand")
        );
    }
}
