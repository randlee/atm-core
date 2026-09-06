//! Tokio process implementation of the private Herdr transport seam.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use atm_core::{HerdrSession, RequestDeadline};
use serde_json::Value;
use tokio::io::AsyncReadExt;

use crate::transport::{HerdrClientConfig, HerdrEnvelope, HerdrErrorEnvelope, HerdrOp};
use crate::{HERDR_MAX_OUTPUT_BYTES, HERDR_PROCESS_CAP, HerdrError};

#[derive(Clone, Debug)]
pub(crate) struct CliIo {
    binary_path: Option<PathBuf>,
    extra_environment: Vec<(String, String)>,
}

impl CliIo {
    pub(crate) fn new(config: &HerdrClientConfig) -> Self {
        let _ = config.socket_path();
        Self {
            binary_path: config.binary_path().cloned(),
            extra_environment: Vec::new(),
        }
    }

    #[cfg(feature = "test-utils")]
    pub(crate) fn with_test_binary_and_environment(
        binary_path: PathBuf,
        extra_environment: Vec<(String, String)>,
    ) -> Self {
        Self {
            binary_path: Some(binary_path),
            extra_environment,
        }
    }

    pub(crate) async fn call(
        &self,
        op: HerdrOp<'_>,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
    ) -> Result<HerdrEnvelope, HerdrError> {
        let args = command_args(op);
        let output = run_command(
            self.binary_path.as_deref(),
            &self.extra_environment,
            &args,
            session,
            deadline,
        )
        .await?;
        decode_envelope(&output)
    }
}

struct CommandOutput {
    stdout: String,
    stderr: String,
    success: bool,
}

pub(crate) fn command_args(op: HerdrOp<'_>) -> Vec<String> {
    match op {
        HerdrOp::Prompt { agent, text } => vec![
            "agent".to_owned(),
            "prompt".to_owned(),
            agent.to_string(),
            text.to_owned(),
        ],
        HerdrOp::Wait {
            agent,
            until,
            timeout,
        } => {
            let mut args = vec!["agent".to_owned(), "wait".to_owned(), agent.to_string()];
            for status in until {
                args.push("--until".to_owned());
                args.push(status.as_str().to_owned());
            }
            args.push("--timeout".to_owned());
            args.push(timeout.as_millis().to_string());
            args
        }
        HerdrOp::Get { agent } => vec!["agent".to_owned(), "get".to_owned(), agent.to_string()],
        HerdrOp::List => vec!["agent".to_owned(), "list".to_owned()],
        HerdrOp::Notify { title, body } => vec![
            "notification".to_owned(),
            "show".to_owned(),
            title.to_owned(),
            "--body".to_owned(),
            body.to_owned(),
            "--sound".to_owned(),
            "request".to_owned(),
        ],
    }
}

async fn run_command(
    configured_binary: Option<&std::path::Path>,
    extra_environment: &[(String, String)],
    args: &[String],
    session: Option<&HerdrSession>,
    deadline: RequestDeadline,
) -> Result<CommandOutput, HerdrError> {
    let Some(remaining) = deadline.remaining() else {
        return Err(HerdrError::TimedOut);
    };
    let binary = configured_binary.unwrap_or_else(|| std::path::Path::new("herdr"));
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.envs(extra_environment.iter().map(|(key, value)| (key, value)));
    if let Some(session) = session {
        command.env("HERDR_SESSION", session.as_str());
    }
    let mut child = command.spawn().map_err(|_| HerdrError::ServerUnavailable {
        message: String::new(),
        retry_after: None,
    })?;
    let status =
        match tokio::time::timeout(effective_process_timeout(remaining), child.wait()).await {
            Ok(Ok(status)) => status,
            Ok(Err(_)) => {
                return Err(HerdrError::ServerUnavailable {
                    message: String::new(),
                    retry_after: None,
                });
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(HerdrError::TimedOut);
            }
        };
    let (stdout, stderr) = capture_command_output(&mut child).await?;
    Ok(CommandOutput {
        stdout,
        stderr,
        success: status.success(),
    })
}

pub(crate) fn effective_process_timeout(remaining: Duration) -> Duration {
    remaining.min(HERDR_PROCESS_CAP)
}

async fn capture_command_output(
    child: &mut tokio::process::Child,
) -> Result<(String, String), HerdrError> {
    let (stdout, stdout_truncated) = if let Some(pipe) = child.stdout.take() {
        read_capped(pipe).await
    } else {
        (Vec::new(), false)
    };
    let (stderr, stderr_truncated) = if let Some(pipe) = child.stderr.take() {
        read_capped(pipe).await
    } else {
        (Vec::new(), false)
    };
    if stdout_truncated || stderr_truncated {
        return Err(HerdrError::Advisory {
            code: format!(
                "output_truncated: stdout={stdout_truncated}, stderr={stderr_truncated}, limit={HERDR_MAX_OUTPUT_BYTES}"
            ),
            message: String::new(),
        });
    }
    Ok((
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    ))
}

pub(crate) async fn read_capped(reader: impl tokio::io::AsyncRead + Unpin) -> (Vec<u8>, bool) {
    let mut output = Vec::new();
    let _ = reader
        .take((HERDR_MAX_OUTPUT_BYTES as u64).saturating_add(1))
        .read_to_end(&mut output)
        .await;
    let truncated = output.len() > HERDR_MAX_OUTPUT_BYTES;
    if truncated {
        output.truncate(HERDR_MAX_OUTPUT_BYTES);
    }
    (output, truncated)
}

fn decode_envelope(output: &CommandOutput) -> Result<HerdrEnvelope, HerdrError> {
    let value = serde_json::from_str::<Value>(if output.success {
        &output.stdout
    } else {
        &output.stderr
    })
    .map_err(|_| HerdrError::ProtocolMismatch)?;
    let error = value.get("error").and_then(|error| {
        Some(HerdrErrorEnvelope {
            code: error.get("code")?.as_str()?.to_owned(),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            retry_after_ms: error.get("retry_after_ms").and_then(Value::as_u64),
        })
    });
    Ok(HerdrEnvelope {
        result: value.get("result").cloned(),
        error,
    })
}
