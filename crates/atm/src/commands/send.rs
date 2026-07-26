use std::path::PathBuf;

use anyhow::Result;
use atm_core::send::{SendMessageSource, SendRequest, input};
use atm_core::types::TaskId;
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

    #[arg(long = "chat-id", conflicts_with = "actor")]
    chat_id: Option<String>,

    #[arg(long = "as")]
    actor: Option<String>,

    #[arg(long)]
    file: Option<PathBuf>,

    #[arg(long)]
    stdin: bool,

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

    /// Execute the `atm send` command.
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("send")?;
        let json = self.json;
        let request = self.build_request(home_dir.clone(), current_dir.clone())?;
        let composition = CliComposition::bootstrap(
            "send",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        let outcome = composition.send(request)?;

        output::print_send_result(&outcome, json)
    }

    fn build_request(self, home_dir: PathBuf, current_dir: PathBuf) -> Result<SendRequest> {
        let caller_context = if self.actor.is_some() || self.chat_id.is_some() {
            resolve_cli_mutation_caller_context_with_overrides(CallerContextOverrides {
                identity_override: self.actor.as_deref().map(CallerIdentityOverride),
                chat_id_override: self.chat_id.as_deref().map(CallerChatIdOverride),
                team_override: self.team.as_deref().map(CallerTeamOverride),
            })?
        } else {
            resolve_cli_mutation_caller_context(self.team.as_deref().map(CallerTeamOverride))?
        };
        let message_source = self.build_message_source()?;
        SendRequest::new(
            home_dir,
            current_dir,
            caller_context.caller_identity,
            &self.to,
            caller_context.caller_team,
            message_source,
            self.summary,
            self.requires_ack,
            self.task_id,
            self.dry_run,
        )
        .map(|request| request.with_caller_chat_id(caller_context.caller_chat_id))
        .map_err(Into::into)
    }

    fn build_message_source(&self) -> Result<SendMessageSource> {
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

        match (&self.file, self.stdin, &self.message) {
            (Some(path), false, message) => Ok(SendMessageSource::File {
                path: path.clone(),
                message: message.clone(),
            }),
            // stdin is a CLI-owned input source. Materialize it before
            // bootstrapping the daemon so a wire request can never ask the
            // daemon (whose stdin is intentionally null) to read it.
            (None, true, None) => input::read_message_from_stdin()
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
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::SendCommand;
    use atm_core::roles::ROLE_TEAM_LEAD;
    use atm_core::send::SendMessageSource;
    use atm_core::test_support::{EnvGuard, TEST_SENDER};
    use serial_test::serial;
    use tempfile::TempDir;

    const TEST_TEAM: &str = "test-team";

    #[test]
    #[serial(env)]
    fn build_request_rejects_invalid_target_before_core() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(ROLE_TEAM_LEAD))]);
        let command = SendCommand {
            to: "../evil".to_string(),
            message: Some("hello".to_string()),
            team: Some(TEST_TEAM.to_string()),
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
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
            chat_id: None,
            actor: None,
            file: Some(PathBuf::from("message.txt")),
            stdin: true,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };
        let error = command.build_message_source().expect_err("invalid sources");
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
            chat_id: None,
            actor: None,
            file: Some(PathBuf::from("message.md")),
            stdin: true,
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
            chat_id: None,
            actor: None,
            file: None,
            stdin: true,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let file_error = stdin_and_file
            .build_message_source()
            .expect_err("stdin/file conflict");
        let message_error = stdin_and_message
            .build_message_source()
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
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let error = command.build_message_source().expect_err("missing message");

        assert!(error.to_string().contains(
            "Pass positional message text, `--file <path>`, or `--stdin` before retrying `atm send`."
        ));
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
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
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
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
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
            chat_id: Some("1234".to_string()),
            actor: None,
            file: None,
            stdin: false,
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
            chat_id: Some("1234".to_string()),
            actor: None,
            file: None,
            stdin: false,
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
            chat_id: None,
            actor: Some(format!("{TEST_SENDER}-other:1234")),
            file: None,
            stdin: false,
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
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
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
            chat_id: None,
            actor: None,
            file: Some(PathBuf::from("incident.md")),
            stdin: false,
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
