use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use atm_core::error::AtmError;
use clap::{Args, CommandFactory};

use super::Cli;
use crate::observability::CliObservability;
use crate::output;
use crate::output_contract::{HelpResult, HelpResultKind, HelpTopicSummary, HelpTopicTier};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HelpDocLink {
    pub topic: HelpTopic,
    pub relative_path: &'static str,
}

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
        let executable = std::env::current_exe().ok();
        self.render_for_executable(executable.as_deref())
    }

    fn render_for_executable(&self, executable_path: Option<&Path>) -> Result<HelpResult> {
        if self.list {
            return Ok(HelpResult::topic_list(executable_path));
        }

        let Some(target) = self.target.as_deref() else {
            return Ok(HelpResult::overview(executable_path));
        };

        if let Some(topic) = HelpTopic::parse(target) {
            return Ok(HelpResult::concept_topic(topic, executable_path));
        }

        if let Some(body) = render_subcommand_help(target)? {
            return Ok(HelpResult::command_help(target, body, executable_path));
        }

        Err(
            AtmError::help_topic_not_found(format!("unknown help topic or subcommand `{target}`"))
                .into(),
        )
    }
}

impl HelpResult {
    fn overview(executable_path: Option<&Path>) -> Self {
        let commands = top_level_command_names();
        let topics = help_topics();
        let readme = resolved_readme_string(executable_path);
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

Installed user docs:
- {}

Available commands:
- {}
",
                readme,
                commands.join("\n- ")
            ),
            commands,
            topics,
            installed_doc_readme: Some(readme),
            installed_doc_topic_path: None,
        }
    }

    fn topic_list(executable_path: Option<&Path>) -> Self {
        let commands = top_level_command_names();
        let topics = help_topics();
        let topic_lines = topics
            .iter()
            .map(|topic| {
                let doc_suffix = topic
                    .doc_relative_path
                    .map(|path| format!(" [doc: {path}]"))
                    .unwrap_or_default();
                format!(
                    "- {} ({}): {}{}",
                    topic.name,
                    topic.tier.label(),
                    topic.summary,
                    doc_suffix
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let readme = resolved_readme_string(executable_path);
        Self {
            kind: HelpResultKind::TopicList,
            requested_target: None,
            title: "ATM Help Targets".to_string(),
            body: format!(
                "\
ATM Help Targets

Concept topics:
{}

Installed user docs:
- {}

Commands:
- {}
",
                topic_lines,
                readme,
                commands.join("\n- ")
            ),
            commands,
            topics,
            installed_doc_readme: Some(readme),
            installed_doc_topic_path: None,
        }
    }

    fn concept_topic(topic: HelpTopic, executable_path: Option<&Path>) -> Self {
        let commands = top_level_command_names();
        let topics = help_topics();
        let readme = resolved_readme_string(executable_path);
        let topic_doc = resolved_topic_doc_string(executable_path, topic);
        let body = if let Some(topic_doc) = topic_doc.as_deref() {
            format!(
                "{}\nLong-form docs: {topic_doc}\nInstalled docs index: {readme}\n",
                topic.body()
            )
        } else {
            format!(
                "{}\nInstalled docs index: {readme} (topic-specific docs resolve only from an installed atm binary)\n",
                topic.body()
            )
        };
        Self {
            kind: HelpResultKind::ConceptTopic,
            requested_target: Some(topic.name().to_string()),
            title: topic.title().to_string(),
            body,
            commands,
            topics,
            installed_doc_readme: Some(readme),
            installed_doc_topic_path: topic_doc,
        }
    }

    fn command_help(target: &str, body: String, executable_path: Option<&Path>) -> Self {
        let readme = resolved_readme_string(executable_path);
        Self {
            kind: HelpResultKind::CommandHelp,
            requested_target: Some(target.to_string()),
            title: format!("ATM command help: {target}"),
            body: format!("{body}\n\nInstalled user docs: {readme}\n"),
            commands: top_level_command_names(),
            topics: help_topics(),
            installed_doc_readme: Some(readme),
            installed_doc_topic_path: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HelpTopic {
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
            Self::Config => config_body(),
            Self::Errors => errors_body(),
            Self::Hooks => hooks_body(),
            Self::Identity => identity_body(),
            Self::Skills => skills_body(),
        }
    }
}

fn resolved_readme_string(executable_path: Option<&Path>) -> String {
    executable_path
        .and_then(canonical_installed_doc_readme)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| fallback_doc_readme_hint().to_string())
}

fn resolved_topic_doc_string(executable_path: Option<&Path>, topic: HelpTopic) -> Option<String> {
    let link = doc_link_for_topic(topic)?;
    let root = executable_path.and_then(canonical_installed_doc_root)?;
    Some(root.join(link.relative_path).display().to_string())
}

fn canonical_installed_doc_root(executable_path: &Path) -> Option<PathBuf> {
    let canonical = executable_path.canonicalize().ok()?;
    let bin_dir = canonical.parent()?;
    if bin_dir.file_name()?.to_str()? != "bin" {
        return None;
    }
    let install_root = bin_dir.parent()?;
    let doc_root = install_root.join("share").join("doc").join("atm");
    if !doc_root.join("README.md").is_file() {
        return None;
    }
    Some(doc_root)
}

fn canonical_installed_doc_readme(executable_path: &Path) -> Option<PathBuf> {
    canonical_installed_doc_root(executable_path).map(|root| root.join("README.md"))
}

fn doc_link_for_topic(topic: HelpTopic) -> Option<HelpDocLink> {
    let relative_path = match topic {
        HelpTopic::Config => "install-layout.md",
        HelpTopic::Errors => "troubleshooting.md",
        HelpTopic::Hooks => "hooks.md",
        HelpTopic::Identity => "identity-and-team.md",
        HelpTopic::Skills => return None,
    };
    Some(HelpDocLink {
        topic,
        relative_path,
    })
}

fn fallback_doc_readme_hint() -> &'static str {
    "../share/doc/atm/README.md"
}

fn config_body() -> &'static str {
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

fn errors_body() -> &'static str {
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

fn hooks_body() -> &'static str {
    "\
ATM Help: hooks

Post-send hooks are ATM-owned automation that run after ATM processes a send.
They are for notification and integration side effects, not for replacing ATM's
durable store or command contract.

Operator examples:
- add a recipient-scoped hook in `.atm.toml`:
  [[atm.post_send_hooks]]
  recipient = \"team-lead\"
  command = [\"python3\", \"scripts/notify.py\"]
- keep hooks best-effort: a hook may report a degraded notification path, but
  it does not redefine whether ATM durably accepted the message
- use hooks for notification and local integration only; do not use them to
  emulate message persistence, inbox mutation, or reply-state tracking

Troubleshooting:
- if a hook does not run, inspect the recipient selector first
- path-like `command[0]` values resolve relative to the declaring `.atm.toml`
- combine `ATM_LOG=debug` with `--stderr-logs` when you need hook diagnostics
"
}

fn identity_body() -> &'static str {
    "\
ATM Help: identity

ATM command identity is about the sending agent, the selected team, and the
resolved runtime destination. Harness and model are not the same thing.

Send identity precedence:
- `atm send team-lead \"...\"` always uses the resolved caller identity
- ATM resolves caller identity from `ATM_IDENTITY` for mutating commands
- Chat identity precedence is `--as agent:chat`, `--chat-id`, `ATM_CHAT_ID`,
  qualified `ATM_IDENTITY=agent:chat`, then no chat ID. An unqualified `--as`
  explicitly selects no chat ID.

Inspection vs mutation:
- `atm peek --as alice` inspects `alice`'s mailbox without mutating it
- `atm list --as alice` is also inspection-only and does not mutate mailbox state
- `atm read`, `atm ack`, `atm clear`, and `atm send` do not allow identity impersonation
- only `atm send --requires-ack` and task-linked sends create durable acknowledgement work
- `atm ack` closes existing pending-ack work; it never manufactures a new ack obligation
- `atm read --team atm-dev` changes the selected caller team, not the caller identity

Troubleshooting:
- repo-local `[atm].identity` and legacy top-level `identity` are obsolete and
  do not count as runtime identity
- if ATM cannot resolve the required identity, fix the override or
  `ATM_IDENTITY`; ATM must not guess from hook files, config, or mailbox-local
  state
"
}

fn skills_body() -> &'static str {
    "\
ATM Help: skills

Skills are repo-local execution instructions used by agent harnesses while they
work on ATM tasks. They are not part of ATM durable mail semantics.

Operator examples:
- use repo-local skills to standardize how agents perform sprint, QA, or audit work
- skills may tell an agent which docs to read, which tests to run, or which
  orchestration templates to follow
- harness decides whether Claude-compatible inbox append is allowed; model name
  alone does not

Boundary rule:
- skills shape agent execution around ATM work, but they do not change durable
  ATM delivery state, routing truth, or SQLite ownership
"
}

fn help_topics() -> Vec<HelpTopicSummary> {
    HelpTopic::ALL
        .into_iter()
        .map(|topic| HelpTopicSummary {
            name: topic.name(),
            tier: topic.tier(),
            summary: topic.summary(),
            doc_relative_path: doc_link_for_topic(topic).map(|link| link.relative_path),
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
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{
        HelpCommand, HelpResultKind, HelpTopic, HelpTopicTier, canonical_installed_doc_root,
        fallback_doc_readme_hint,
    };

    fn write_installed_tree(root: &Path) {
        fs::create_dir_all(root.join("bin")).expect("bin dir");
        fs::create_dir_all(root.join("share/doc/atm")).expect("doc root");
        fs::write(root.join("share/doc/atm/README.md"), "# ATM\n").expect("readme");
        fs::write(root.join("share/doc/atm/hooks.md"), "# Hooks\n").expect("hooks");
        fs::write(
            root.join("share/doc/atm/identity-and-team.md"),
            "# Identity\n",
        )
        .expect("identity");
        fs::write(root.join("share/doc/atm/install-layout.md"), "# Install\n").expect("install");
        fs::write(
            root.join("share/doc/atm/troubleshooting.md"),
            "# Troubleshooting\n",
        )
        .expect("troubleshooting");
    }

    fn copy_tree(source_root: &Path, destination_root: &Path) {
        fn recurse(source_root: &Path, destination_root: &Path, current: &Path) {
            for entry in fs::read_dir(current).expect("read source dir") {
                let entry = entry.expect("dir entry");
                let source_path = entry.path();
                let relative = source_path
                    .strip_prefix(source_root)
                    .expect("relative source path");
                let destination = destination_root.join(relative);
                if source_path.is_dir() {
                    fs::create_dir_all(&destination).expect("create destination dir");
                    recurse(source_root, destination_root, &source_path);
                    continue;
                }
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).expect("destination parent");
                }
                fs::copy(&source_path, &destination).expect("copy doc file");
            }
        }

        recurse(source_root, destination_root, source_root);
    }

    #[cfg(unix)]
    fn symlink_file(src: &Path, dst: &Path) {
        std::os::unix::fs::symlink(src, dst).expect("symlink");
    }

    #[cfg(windows)]
    fn symlink_file(src: &Path, dst: &Path) {
        std::os::windows::fs::symlink_file(src, dst).expect("symlink");
    }

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
        assert!(
            result
                .topics
                .iter()
                .any(|topic| topic.name == "hooks" && topic.doc_relative_path == Some("hooks.md"))
        );
        assert!(
            result
                .topics
                .iter()
                .any(|topic| topic.name == "skills" && topic.doc_relative_path.is_none())
        );
    }

    #[test]
    fn doc_link_for_every_topic_resolves_in_source_and_installed_copy() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let tempdir = TempDir::new().expect("tempdir");
        let source_root = repo_root.join("docs/user-documents");
        let installed_root = tempdir.path().join("share/doc/atm");
        copy_tree(&source_root, &installed_root);

        for topic in HelpTopic::ALL {
            let Some(link) = super::doc_link_for_topic(topic) else {
                continue;
            };
            assert!(
                source_root.join(link.relative_path).is_file(),
                "missing source doc for topic {} at {}",
                topic.name(),
                link.relative_path
            );
            assert!(
                installed_root.join(link.relative_path).is_file(),
                "missing installed doc for topic {} at {}",
                topic.name(),
                link.relative_path
            );
        }
    }

    #[test]
    fn concept_topics_are_case_insensitive() {
        assert_eq!(HelpTopic::parse("ConFiG"), Some(HelpTopic::Config));
        assert_eq!(HelpTopic::parse("ERRORS"), Some(HelpTopic::Errors));
    }

    #[test]
    fn tier_two_topics_include_concrete_examples_after_y2() {
        let hooks = HelpCommand {
            target: Some("hooks".to_string()),
            list: false,
            json: false,
        }
        .render()
        .expect("hooks help");
        let identity = HelpCommand {
            target: Some("identity".to_string()),
            list: false,
            json: false,
        }
        .render()
        .expect("identity help");
        let skills = HelpCommand {
            target: Some("skills".to_string()),
            list: false,
            json: false,
        }
        .render()
        .expect("skills help");

        assert!(hooks.body.contains("[[atm.post_send_hooks]]"));
        assert!(hooks.body.contains("ATM_LOG=debug"));
        assert!(identity.body.contains("`ATM_IDENTITY`"));
        assert!(identity.body.contains("`atm peek --as alice`"));
        assert!(identity.body.contains("`atm list --as alice`"));
        assert!(identity.body.contains("`atm send --requires-ack`"));
        assert!(
            skills
                .body
                .contains("harness decides whether Claude-compatible")
        );
        assert!(!hooks.body.contains("Y.2 will"));
        assert!(!identity.body.contains("Y.2 will"));
        assert!(!skills.body.contains("Y.2 will"));
    }

    #[test]
    fn skills_topic_does_not_claim_installed_long_form_docs() {
        let result = HelpCommand {
            target: Some("skills".to_string()),
            list: false,
            json: false,
        }
        .render()
        .expect("skills help");

        assert_eq!(result.installed_doc_topic_path, None);
        assert!(result.body.contains("Installed docs index:"));
        assert!(!result.body.contains("Long-form docs:"));
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
        assert!(result.body.contains(fallback_doc_readme_hint()));
        assert!(!result.body.is_empty());
    }

    #[test]
    fn concept_topic_json_fields_point_at_installed_docs_when_available() {
        let tempdir = TempDir::new().expect("tempdir");
        let install_root = tempdir.path().join("atm");
        write_installed_tree(&install_root);
        let executable = install_root.join("bin/atm");
        fs::write(&executable, "atm").expect("atm binary");

        let command = HelpCommand {
            target: Some("hooks".to_string()),
            list: false,
            json: true,
        };
        let result = command
            .render_for_executable(Some(&executable))
            .expect("hooks help");
        let canonical_install_root = install_root.canonicalize().expect("canonical install root");

        assert_eq!(
            result.installed_doc_readme.as_deref(),
            Some(
                canonical_install_root
                    .join("share/doc/atm/README.md")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            result.installed_doc_topic_path.as_deref(),
            Some(
                canonical_install_root
                    .join("share/doc/atm/hooks.md")
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }

    #[test]
    fn installed_doc_root_resolves_symlinked_executable() {
        let tempdir = TempDir::new().expect("tempdir");
        let install_root = tempdir.path().join("versions/1.3.1");
        write_installed_tree(&install_root);
        let real_executable = install_root.join("bin/atm");
        fs::write(&real_executable, "atm").expect("atm binary");

        let shim_dir = tempdir.path().join("shims");
        fs::create_dir_all(&shim_dir).expect("shim dir");
        let shim = shim_dir.join("atm");
        symlink_file(&real_executable, &shim);

        let resolved = canonical_installed_doc_root(&shim).expect("installed doc root");
        let expected = install_root
            .canonicalize()
            .expect("canonical install root")
            .join("share/doc/atm");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn installed_doc_root_returns_none_for_dev_build_layout() {
        let tempdir = TempDir::new().expect("tempdir");
        let executable = tempdir.path().join("target/debug/atm");
        fs::create_dir_all(executable.parent().expect("parent")).expect("debug dir");
        fs::write(&executable, "atm").expect("atm binary");

        assert_eq!(canonical_installed_doc_root(&executable), None);
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
