use std::path::PathBuf;

use anyhow::Result;
use atm_core::address::AgentAddress;
use atm_core::load_atm_config;
use atm_core::send::{
    MessageClassification, SendMessageSource, SendRequest, TemplateSendSource, input,
};
use atm_core::types::{HostName, TaskId, TeamName};
use clap::Args;

use crate::commands::caller_context::{
    CallerChatIdOverride, CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
    resolve_cli_mutation_caller_context, resolve_cli_mutation_caller_context_with_overrides,
};
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
#[command(
    after_help = "Post-send hooks can be configured in .atm.toml via one or more [[atm.post_send_hooks]] rules with recipient = \"name-or-*\" and command = [\"argv\", ...]. Matching rules run after a successful non-dry-run send, in config order. Path-like command[0] values resolve relative to the declaring .atm.toml; bare executables like bash or python3 use normal PATH resolution. Recipient non-match is silent. For hook troubleshooting, combine --stderr-logs with ATM_LOG=debug to surface debug-level hook diagnostics on stderr."
)]
/// Send one ATM mailbox message.
pub struct SendCommand {
    #[arg()]
    to: String,

    #[arg(index = 2)]
    message: Option<String>,

    #[arg(long)]
    team: Option<String>,

    /// Route this send through the explicitly named host.
    ///
    /// Supplying this flag allows a same-identity send to be an intentional
    /// physical delivery test (for example, `--host localhost` or a same-host
    /// IP).
    /// This is wire-equivalent to a host-qualified recipient address. When
    /// both forms are supplied, they must name the same host.
    #[arg(long, value_name = "HOST")]
    host: Option<String>,

    #[arg(long = "chat-id", conflicts_with = "actor")]
    chat_id: Option<String>,

    #[arg(long = "as")]
    actor: Option<String>,

    #[arg(long)]
    file: Option<PathBuf>,

    #[arg(long)]
    stdin: bool,

    /// Render and send a locally loaded template through the daemon-owned
    /// template admission path.
    #[arg(long, value_name = "PATH")]
    template: Option<PathBuf>,

    /// JSON object providing template variables. `-` reads this object from
    /// stdin; it is distinct from `--stdin`, which is a plain message source.
    #[arg(long, value_name = "FILE|-")]
    vars: Option<String>,

    /// One template variable. May be repeated; values parse as JSON when
    /// possible and otherwise remain strings.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    var: Vec<String>,

    /// Capture current environment variables with this prefix at CLI
    /// composition time.
    #[arg(long = "env-prefix", value_name = "PREFIX")]
    env_prefix: Option<String>,

    #[arg(long, value_name = "CATEGORY")]
    category: Option<String>,

    #[arg(long = "tag", value_name = "TAG")]
    tag: Vec<String>,

    #[arg(long = "content-format", value_name = "FORMAT")]
    content_format: Option<String>,

    #[arg(long)]
    summary: Option<String>,

    #[arg(long = "requires-ack")]
    requires_ack: bool,

    #[arg(long = "task-id")]
    task_id: Option<TaskId>,

    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    json: bool,
}

impl SendCommand {
    fn message_validation_error(
        message: impl Into<String>,
        recovery: impl Into<String>,
    ) -> anyhow::Error {
        atm_core::error::AtmError::validation_with_recovery(message, recovery).into()
    }

    fn template_load_error(message: impl Into<String>) -> anyhow::Error {
        atm_core::error::AtmError::new(atm_core::error::AtmErrorCode::TemplateLoadFailed, message)
            .into()
    }

    fn template_classification_error(message: impl Into<String>) -> anyhow::Error {
        atm_core::error::AtmError::new(
            atm_core::error::AtmErrorCode::TemplateClassificationInvalid,
            message,
        )
        .into()
    }

    /// Execute the `atm send` command.
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("send")?;
        let json = self.json;
        let request = self.build_request(home_dir.clone(), current_dir.clone())?;
        let composition = CliComposition::bootstrap(
            "send",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        let outcome = composition.send(request).await?;

        output::print_send_result(&outcome, json)
    }

    fn build_request(self, home_dir: PathBuf, current_dir: PathBuf) -> Result<SendRequest> {
        let max_message_bytes = load_atm_config(&current_dir)?
            .map(|config| {
                config.max_message_bytes.as_usize().ok_or_else(|| {
                    anyhow::anyhow!("configured max_message_bytes does not fit this platform")
                })
            })
            .transpose()?
            .unwrap_or(input::default_message_max_bytes());
        let caller_context = if self.actor.is_some() || self.chat_id.is_some() {
            resolve_cli_mutation_caller_context_with_overrides(CallerContextOverrides {
                identity_override: self.actor.as_deref().map(CallerIdentityOverride),
                chat_id_override: self.chat_id.as_deref().map(CallerChatIdOverride),
                team_override: self.team.as_deref().map(CallerTeamOverride),
            })?
        } else {
            resolve_cli_mutation_caller_context(self.team.as_deref().map(CallerTeamOverride))?
        };
        let target = self.target_with_explicit_host(&caller_context.caller_team)?;
        let classification = self.build_classification()?;
        let message_source = self.build_message_source(max_message_bytes, &current_dir)?;
        SendRequest::new(
            home_dir,
            current_dir,
            caller_context.caller_identity,
            &target,
            caller_context.caller_team,
            message_source,
            self.summary,
            self.requires_ack,
            self.task_id,
            self.dry_run,
        )
        .map(|request| {
            request
                .with_caller_chat_id(caller_context.caller_chat_id)
                .with_activity_observation(caller_context.activity_observation)
                .with_max_message_bytes(max_message_bytes)
                .with_classification(classification)
        })
        .map_err(Into::into)
    }

    fn target_with_explicit_host(&self, caller_team: &TeamName) -> Result<String> {
        let explicit_host = match self.host.as_deref() {
            Some(raw_host) => Some(raw_host.parse::<HostName>().map_err(|_source| {
                Self::message_validation_error(
                    "invalid --host",
                    "Pass a valid hostname or IP address to `--host` before retrying `atm send`.",
                )
            })?),
            None => None,
        };
        let recipient: AgentAddress = self.to.parse().map_err(anyhow::Error::from)?;

        match (recipient.host(), explicit_host) {
            (Some(address_host), Some(flag_host)) if address_host != &flag_host => {
                Err(Self::message_validation_error(
                    "recipient host and --host disagree",
                    "Use the same host in both places, or specify it once with either `recipient@team.host` or `--host <host>`.",
                ))
            }
            (Some(_), _) => Ok(recipient.to_string()),
            (None, None) => Ok(recipient.to_string()),
            (None, Some(host)) => AgentAddress::new(
                recipient.agent().clone(),
                recipient.chat_id().cloned(),
                Some(
                    recipient
                        .team()
                        .cloned()
                        .unwrap_or_else(|| caller_team.clone()),
                ),
                Some(host),
            )
            .map(|target| target.to_string())
            .map_err(Into::into),
        }
    }

    fn build_message_source(
        &self,
        max_message_bytes: usize,
        current_dir: &std::path::Path,
    ) -> Result<SendMessageSource> {
        if self.template.is_some() && (self.stdin || self.file.is_some() || self.message.is_some())
        {
            return Err(Self::message_validation_error(
                "--template is mutually exclusive with message text, --file, and --stdin",
                "Use `--template <path>` by itself, then pass template data through --vars, --var, or --env-prefix.",
            ));
        }
        if self.stdin && self.file.is_some() {
            return Err(Self::message_validation_error(
                "--stdin and --file are mutually exclusive",
                "Choose exactly one message source: either pass `--stdin` or `--file <path>` before retrying `atm send`.",
            ));
        }

        if self.stdin && self.message.is_some() {
            return Err(Self::message_validation_error(
                "--stdin and positional message text are mutually exclusive",
                "Choose exactly one message source: either pass `--stdin` or provide positional message text before retrying `atm send`.",
            ));
        }

        if let Some(template) = &self.template {
            return self
                .build_template_source(template, current_dir)
                .map(SendMessageSource::Template);
        }

        if self.vars.is_some() || !self.var.is_empty() || self.env_prefix.is_some() {
            return Err(Self::message_validation_error(
                "template-only option supplied without --template",
                "Pass `--template <path>` before using --vars, --var, or --env-prefix.",
            ));
        }

        match (&self.file, self.stdin, &self.message) {
            (Some(path), false, message) => Ok(SendMessageSource::File {
                path: path.clone(),
                message: message.clone(),
            }),
            // stdin is a CLI-owned input source. Materialize it before
            // bootstrapping the daemon so a wire request can never ask the
            // daemon (whose stdin is intentionally null) to read it.
            (None, true, None) => input::read_message_from_stdin_with_limit(max_message_bytes)
                .map(SendMessageSource::Inline)
                .map_err(Into::into),
            (None, false, Some(message)) => Ok(SendMessageSource::Inline(message.clone())),
            (None, false, None) => Err(Self::message_validation_error(
                "provide message text, --file, or --stdin",
                "Pass positional message text, `--file <path>`, or `--stdin` before retrying `atm send`.",
            )),
            (Some(_), true, _) => unreachable!("validated above"),
            (None, true, Some(_)) => unreachable!("validated above"),
        }
    }

    fn build_template_source(
        &self,
        template: &std::path::Path,
        current_dir: &std::path::Path,
    ) -> Result<TemplateSendSource> {
        let template_path = if template.is_absolute() {
            template.to_path_buf()
        } else {
            current_dir.join(template)
        };
        let canonical_template_path = std::fs::canonicalize(&template_path).map_err(|error| {
            Self::template_load_error(format!("template could not be resolved: {error}"))
        })?;
        let canonical_template_root = canonical_template_path
            .parent()
            .ok_or_else(|| Self::template_load_error("template path has no parent directory"))?
            .to_path_buf();
        let raw_file_bytes = std::fs::read(&canonical_template_path).map_err(|error| {
            Self::template_load_error(format!("template could not be read: {error}"))
        })?;
        let var_file_values = self.read_var_file(current_dir)?;
        let explicit_values = parse_assignment_values(&self.var)?;
        let environment_values = capture_environment_values(self.env_prefix.as_deref())?;
        Ok(TemplateSendSource {
            canonical_template_path,
            canonical_template_root,
            raw_file_bytes,
            input_defaults: serde_json::Map::new(),
            var_file_values,
            explicit_values,
            environment_values,
        })
    }

    fn build_classification(&self) -> Result<MessageClassification> {
        let category = normalize_optional_label(self.category.as_deref(), "category")?;
        let content_format =
            normalize_optional_label(self.content_format.as_deref(), "content format")?;
        let tags = parse_tags(&self.tag)?;
        validate_classification(&category, &tags, &content_format)?;
        Ok(MessageClassification {
            category,
            tags,
            content_format,
        })
    }

    fn read_var_file(
        &self,
        current_dir: &std::path::Path,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        let Some(source) = self.vars.as_deref() else {
            return Ok(serde_json::Map::new());
        };
        let contents = if source == "-" {
            use std::io::Read as _;
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|error| {
                    Self::template_load_error(format!("--vars stdin could not be read: {error}"))
                })?;
            input
        } else {
            let path = std::path::Path::new(source);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                current_dir.join(path)
            };
            std::fs::read_to_string(path).map_err(|error| {
                Self::template_load_error(format!("--vars file could not be read: {error}"))
            })?
        };
        let value: serde_json::Value = serde_json::from_str(&contents).map_err(|error| {
            Self::template_load_error(format!("--vars must contain a JSON object: {error}"))
        })?;
        value
            .as_object()
            .cloned()
            .ok_or_else(|| Self::template_load_error("--vars must contain a JSON object"))
    }
}

fn parse_assignment_values(
    values: &[String],
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let mut parsed = serde_json::Map::new();
    for raw in values {
        let (key, value) = raw.split_once('=').ok_or_else(|| {
            SendCommand::message_validation_error(
                format!("invalid --var '{raw}'"),
                "Use `--var key=value`; the key must not be blank.",
            )
        })?;
        if key.trim().is_empty() {
            return Err(SendCommand::message_validation_error(
                "template variable key must not be blank",
                "Use `--var key=value` with a non-blank key.",
            ));
        }
        let json_value = serde_json::from_str(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
        parsed.insert(key.to_string(), json_value);
    }
    Ok(parsed)
}

fn capture_environment_values(
    prefix: Option<&str>,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let Some(prefix) = prefix else {
        return Ok(serde_json::Map::new());
    };
    if prefix.is_empty() {
        return Err(SendCommand::message_validation_error(
            "--env-prefix must not be empty",
            "Pass a non-empty prefix such as `ATM_TEMPLATE_`.",
        ));
    }
    Ok(std::env::vars()
        .filter_map(|(key, value)| {
            key.strip_prefix(prefix)
                .map(|name| (name.to_string(), serde_json::Value::String(value)))
        })
        .collect())
}

fn normalize_optional_label(value: Option<&str>, kind: &str) -> Result<Option<String>> {
    value
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                Err(SendCommand::message_validation_error(
                    format!("template {kind} must not be blank"),
                    format!("Pass a non-blank --{} value.", kind.replace(' ', "-")),
                ))
            } else {
                Ok(value.to_string())
            }
        })
        .transpose()
}

fn parse_tags(raw_tags: &[String]) -> Result<Vec<String>> {
    let tags: Vec<String> = raw_tags
        .iter()
        .flat_map(|raw| raw.split(','))
        .map(str::trim)
        .map(str::to_string)
        .collect();
    if tags.iter().any(|tag| tag.is_empty()) {
        return Err(SendCommand::message_validation_error(
            "template tag must not be blank",
            "Use comma-separated non-blank tags, or repeat --tag.",
        ));
    }
    Ok(tags)
}

fn validate_classification(
    category: &Option<String>,
    tags: &[String],
    content_format: &Option<String>,
) -> Result<()> {
    const MAX_TAGS: usize = 16;
    if tags.len() > MAX_TAGS {
        return Err(SendCommand::template_classification_error(format!(
            "template tag count exceeds {MAX_TAGS}"
        )));
    }
    let valid_label = |label: &str| {
        !label.is_empty()
            && label.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            })
    };
    if category.as_deref().is_some_and(|value| !valid_label(value))
        || content_format
            .as_deref()
            .is_some_and(|value| !valid_label(value))
        || tags.iter().any(|tag| !valid_label(tag))
    {
        return Err(SendCommand::template_classification_error(
            "template category, tag, and content format must use lowercase letters, digits, '-', '_', or '.'",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::SendCommand;
    use atm_core::roles::ROLE_TEAM_LEAD;
    use atm_core::send::{SendMessageSource, input};
    use atm_core::test_support::{EnvGuard, TEST_SENDER};
    use clap::Parser;
    use serial_test::serial;
    use tempfile::TempDir;

    const TEST_TEAM: &str = "test-team";

    fn send_command(to: &str, host: Option<&str>) -> SendCommand {
        SendCommand {
            to: to.to_string(),
            message: Some("hello".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: host.map(str::to_string),
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        }
    }

    #[test]
    #[serial(env)]
    fn explicit_host_is_wire_equivalent_to_a_host_qualified_destination() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(TEST_SENDER))]);
        let via_flag = send_command(&format!("{TEST_SENDER}@{TEST_TEAM}"), Some("localhost"))
            .build_request(".".into(), ".".into())
            .expect("explicit loopback host target");
        let via_destination = send_command(&format!("{TEST_SENDER}@{TEST_TEAM}.localhost"), None)
            .build_request(".".into(), ".".into())
            .expect("host-qualified destination");

        assert_eq!(
            via_flag.to.expect("flag target").to_string(),
            via_destination.to.expect("destination target").to_string(),
            "both forms must create the same host-qualified destination before shared self-send validation"
        );
    }

    #[test]
    #[serial(env)]
    fn explicit_same_ip_host_qualifies_a_self_send_for_the_shared_guard() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(TEST_SENDER))]);
        let request = send_command(
            &format!("{TEST_SENDER}@{TEST_TEAM}"),
            Some("192.168.128.82"),
        )
        .build_request(".".into(), ".".into())
        .expect("same-IP target");

        assert_eq!(
            request.to.expect("target").to_string(),
            format!("{TEST_SENDER}@{TEST_TEAM}.192.168.128.82")
        );
    }

    #[test]
    #[serial(env)]
    fn explicit_host_rejects_a_conflicting_destination_suffix() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(TEST_SENDER))]);
        let error = send_command(
            &format!("{TEST_SENDER}@{TEST_TEAM}.localhost"),
            Some("192.168.128.82"),
        )
        .build_request(".".into(), ".".into())
        .expect_err("mismatched host selection must be rejected");

        assert!(
            error
                .to_string()
                .contains("recipient host and --host disagree")
        );
    }

    #[test]
    fn cli_accepts_the_explicit_host_flag() {
        crate::commands::Cli::try_parse_from([
            "atm",
            "send",
            "recipient-a@test-team",
            "hello",
            "--host",
            "localhost",
        ])
        .expect("documented explicit host command must parse");
    }

    #[test]
    #[serial(env)]
    fn build_request_rejects_invalid_target_before_core() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(ROLE_TEAM_LEAD))]);
        let command = SendCommand {
            to: "../evil".to_string(),
            message: Some("hello".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("invalid target");

        assert!(error.to_string().contains("agent name"));
    }

    #[test]
    fn validation_errors_retain_their_actionable_recovery() {
        let command = SendCommand {
            to: "recipient@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: Some(PathBuf::from("message.txt")),
            stdin: true,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };
        let error = command
            .build_message_source(
                input::default_message_max_bytes(),
                std::path::Path::new("."),
            )
            .expect_err("invalid sources");
        assert!(
            error
                .to_string()
                .contains("Choose exactly one message source")
        );
    }

    #[test]
    fn build_message_source_rejects_conflicting_input_flags() {
        let stdin_and_file = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: None,
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: Some(PathBuf::from("message.md")),
            stdin: true,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };
        let stdin_and_message = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: true,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let file_error = stdin_and_file
            .build_message_source(
                input::default_message_max_bytes(),
                std::path::Path::new("."),
            )
            .expect_err("stdin/file conflict");
        let message_error = stdin_and_message
            .build_message_source(
                input::default_message_max_bytes(),
                std::path::Path::new("."),
            )
            .expect_err("stdin/message conflict");

        assert!(file_error.to_string().contains(
            "Choose exactly one message source: either pass `--stdin` or `--file <path>`"
        ));
        assert!(message_error.to_string().contains(
            "Choose exactly one message source: either pass `--stdin` or provide positional message text"
        ));
    }

    #[test]
    fn build_message_source_requires_one_input_channel() {
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: None,
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let error = command
            .build_message_source(
                input::default_message_max_bytes(),
                std::path::Path::new("."),
            )
            .expect_err("missing message");

        assert!(error.to_string().contains(
            "Pass positional message text, `--file <path>`, or `--stdin` before retrying `atm send`."
        ));
    }

    #[test]
    #[serial(env)]
    fn template_path_is_resolved_from_the_command_invocation_directory() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(ROLE_TEAM_LEAD))]);
        let tempdir = TempDir::new().expect("tempdir");
        let template_dir = tempdir.path().join("templates");
        std::fs::create_dir_all(&template_dir).expect("template directory");
        std::fs::write(template_dir.join("notice.j2"), "hello {{ name }}")
            .expect("template fixture");
        let mut command = send_command("recipient-a@test-team", None);
        command.message = None;
        command.template = Some(PathBuf::from("templates/notice.j2"));
        command.var = vec!["name=Rand".to_string()];

        let source = command
            .build_message_source(input::default_message_max_bytes(), tempdir.path())
            .expect("template source");

        let SendMessageSource::Template(source) = source else {
            panic!("expected template source");
        };
        assert_eq!(
            source.canonical_template_path,
            std::fs::canonicalize(template_dir.join("notice.j2")).expect("canonical path")
        );
        assert_eq!(
            source.explicit_values.get("name"),
            Some(&serde_json::Value::String("Rand".to_string()))
        );
    }

    #[test]
    fn template_source_rejects_plain_message_and_invalid_classification() {
        let mut conflict = send_command("recipient-a@test-team", None);
        conflict.template = Some(PathBuf::from("notice.j2"));
        let error = conflict
            .build_message_source(input::default_message_max_bytes(), Path::new("."))
            .expect_err("template and positional message conflict");
        assert!(
            error
                .to_string()
                .contains("--template is mutually exclusive")
        );

        let error = super::validate_classification(&Some("Uppercase".to_string()), &[], &None)
            .expect_err("classification must be normalized");
        let error = error
            .downcast_ref::<atm_core::error::AtmError>()
            .expect("typed ATM error");
        assert_eq!(
            error.code(),
            atm_core::error::AtmErrorCode::TemplateClassificationInvalid
        );
    }

    #[test]
    #[serial(env)]
    fn classification_is_available_for_an_ordinary_send() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(ROLE_TEAM_LEAD)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let mut command = send_command("recipient-a@test-team", None);
        command.category = Some("assignment".to_owned());
        command.tag = vec!["phase-an,dev".to_owned()];
        command.content_format = Some("text.markdown".to_owned());

        let request = command
            .build_request(".".into(), ".".into())
            .expect("ordinary classified request");

        assert_eq!(
            request.classification.category.as_deref(),
            Some("assignment")
        );
        assert_eq!(request.classification.tags, ["phase-an", "dev"]);
        assert_eq!(
            request.classification.content_format.as_deref(),
            Some("text.markdown")
        );
    }

    #[test]
    #[serial(env)]
    fn build_request_preserves_cli_send_options() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(ROLE_TEAM_LEAD)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello from send".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: Some("summary".to_string()),
            requires_ack: true,
            task_id: Some("TASK-42".parse().expect("task id")),
            dry_run: true,
            json: true,
        };
        let tempdir = TempDir::new().expect("tempdir");

        let request = command
            .build_request(tempdir.path().join("home"), tempdir.path().join("cwd"))
            .expect("request");

        assert_eq!(Some(request.caller_identity.as_str()), Some(ROLE_TEAM_LEAD));
        assert_eq!(Some(request.caller_team.as_str()), Some(TEST_TEAM));
        assert_eq!(request.summary_override.as_deref(), Some("summary"));
        assert!(request.requires_ack);
        assert_eq!(
            request.task_id.as_ref().map(|value| value.as_str()),
            Some("TASK-42")
        );
        assert!(request.dry_run);
        assert_eq!(
            request.to.expect("destination").to_string(),
            "recipient-a@test-team"
        );
        match request.message_source {
            SendMessageSource::Inline(message) => assert_eq!(message, "hello from send"),
            other => panic!("expected inline message source, got {other:?}"),
        }
    }

    #[test]
    #[serial(env)]
    fn build_request_uses_environment_when_overrides_are_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a")),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let request = command
            .build_request(".".into(), ".".into())
            .expect("request");

        assert_eq!(request.caller_identity.as_str(), "sender-a");
        assert_eq!(request.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial(env)]
    fn chat_id_and_equivalent_as_construct_the_same_caller() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let base = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: Some("1234".to_string()),
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };
        let explicit = SendCommand {
            chat_id: None,
            actor: Some(format!("{TEST_SENDER}:1234")),
            ..base
        };

        let shorthand = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: Some("1234".to_string()),
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        }
        .build_request(".".into(), ".".into())
        .expect("shorthand request");
        let explicit = explicit
            .build_request(".".into(), ".".into())
            .expect("explicit request");

        assert_eq!(shorthand.caller_identity, explicit.caller_identity);
        assert_eq!(shorthand.caller_chat_id, explicit.caller_chat_id);
        assert_eq!(shorthand.caller_chat_id.unwrap().as_str(), "1234");
    }

    #[test]
    #[serial(env)]
    fn build_request_rejects_as_for_a_different_base_agent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: None,
            actor: Some(format!("{TEST_SENDER}-other:1234")),
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("different agent must be rejected");
        assert!(error.to_string().contains("same base agent"));
    }

    #[test]
    #[serial(env)]
    fn build_request_uses_environment_identity_even_with_team_override() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("env-sender")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let request = command
            .build_request(".".into(), ".".into())
            .expect("request");

        assert_eq!(request.caller_identity.as_str(), "env-sender");
        assert_eq!(request.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial(env)]
    fn build_request_supports_file_with_trailing_inline_note() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(ROLE_TEAM_LEAD))]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("note".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: None,
            chat_id: None,
            actor: None,
            file: Some(PathBuf::from("incident.md")),
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };
        let tempdir = TempDir::new().expect("tempdir");

        let request = command
            .build_request(tempdir.path().join("home"), tempdir.path().join("cwd"))
            .expect("request");

        match request.message_source {
            SendMessageSource::File { path, message } => {
                assert_eq!(path, PathBuf::from("incident.md"));
                assert_eq!(message.as_deref(), Some("note"));
            }
            other => panic!("expected file message source, got {other:?}"),
        }
    }
}
