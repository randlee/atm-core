use std::path::PathBuf;

use anyhow::Result;
use atm_core::send::{SendMessageSource, SendRequest, input};
use atm_core::types::TaskId;
use clap::Args;

use crate::commands::caller_context::{CallerTeamOverride, resolve_cli_mutation_caller_context};
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
        let caller_context =
            resolve_cli_mutation_caller_context(self.team.as_deref().map(CallerTeamOverride))?;
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
        .map_err(Into::into)
    }

    fn build_message_source(&self) -> Result<SendMessageSource> {
        if self.stdin && self.file.is_some() {
            anyhow::bail!("--stdin and --file are mutually exclusive");
        }

        if self.stdin && self.message.is_some() {
            anyhow::bail!("--stdin and positional message text are mutually exclusive");
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
            (None, false, None) => {
                anyhow::bail!("provide message text, --file, or --stdin")
            }
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
    use atm_core::test_support::EnvGuard;
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
    fn build_message_source_rejects_conflicting_input_flags() {
        let stdin_and_file = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: None,
            team: None,
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

        assert!(file_error.to_string().contains("mutually exclusive"));
        assert!(message_error.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn build_message_source_requires_one_input_channel() {
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: None,
            team: None,
            file: None,
            stdin: false,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let error = command.build_message_source().expect_err("missing message");

        assert!(error.to_string().contains("provide message text"));
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
        assert_eq!(request.to.to_string(), "recipient-a@test-team");
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
    fn build_request_uses_environment_identity_even_with_team_override() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("env-sender")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: Some(TEST_TEAM.to_string()),
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
