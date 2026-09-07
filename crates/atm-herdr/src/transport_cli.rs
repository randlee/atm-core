//! Tokio process implementation of the private Herdr transport seam.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use atm_core::{HerdrSession, RequestDeadline};
use serde_json::Value;
use tokio::io::AsyncReadExt;

#[cfg(windows)]
use std::future::Future;
#[cfg(windows)]
use std::pin::Pin;

use crate::transport::{HerdrClientConfig, HerdrEnvelope, HerdrErrorEnvelope, HerdrOp};
use crate::{HERDR_MAX_OUTPUT_BYTES, HERDR_PROCESS_CAP, HerdrError};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const WINDOWS_CHILD_CLEANUP_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub(crate) struct CliIo {
    binary_path: Option<PathBuf>,
    extra_environment: Vec<(String, String)>,
}

impl CliIo {
    pub(crate) fn new(config: &HerdrClientConfig) -> Self {
        let _ = config.socket_path();
        Self {
            binary_path: config.binary_path().map(Path::to_path_buf),
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
        HerdrOp::StatusServer => vec![
            "status".to_owned(),
            "server".to_owned(),
            "--json".to_owned(),
        ],
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
    let binary = command_binary(configured_binary);
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    configure_windows_command(&mut command);
    command.envs(extra_environment.iter().map(|(key, value)| (key, value)));
    if let Some(session) = session {
        command.env("HERDR_SESSION", session.as_str());
    }
    let mut child = command
        .spawn()
        .map_err(|error| spawn_unavailable(error, configured_binary))?;
    let status =
        match tokio::time::timeout(effective_process_timeout(remaining), child.wait()).await {
            Ok(Ok(status)) => status,
            Ok(Err(error)) => return Err(server_unavailable(error)),
            Err(_) => {
                #[cfg(windows)]
                cleanup_after_timeout(&mut child, WINDOWS_CHILD_CLEANUP_GRACE).await?;
                #[cfg(not(windows))]
                {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
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

fn command_binary(configured_binary: Option<&Path>) -> PathBuf {
    #[cfg(windows)]
    {
        return configured_binary.map_or_else(
            || PathBuf::from("herdr.exe"),
            |path| {
                if path.is_dir() {
                    path.join("herdr.exe")
                } else {
                    path.to_path_buf()
                }
            },
        );
    }

    #[cfg(not(windows))]
    configured_binary
        .unwrap_or_else(|| Path::new("herdr"))
        .to_path_buf()
}

#[cfg(windows)]
fn configure_windows_command(command: &mut tokio::process::Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

fn spawn_unavailable(error: std::io::Error, configured_binary: Option<&Path>) -> HerdrError {
    #[cfg(windows)]
    {
        let source = configured_binary.map_or_else(
            || "Windows PATH search for herdr.exe".to_owned(),
            |path| format!("configured Herdr binary {}", path.display()),
        );
        return HerdrError::ServerUnavailable {
            message: format!("{source}: {error}"),
            retry_after: None,
            io_error_kind: Some(error.kind()),
        };
    }

    #[cfg(not(windows))]
    {
        let _ = configured_binary;
        server_unavailable(error)
    }
}

fn server_unavailable(error: std::io::Error) -> HerdrError {
    HerdrError::ServerUnavailable {
        message: error.to_string(),
        retry_after: None,
        io_error_kind: Some(error.kind()),
    }
}

#[cfg(windows)]
trait ChildHandle {
    fn pid(&self) -> Option<u32>;
    fn kill<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + 'a>>;
    fn wait_for_exit<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + 'a>>;
}

#[cfg(windows)]
impl ChildHandle for tokio::process::Child {
    fn pid(&self) -> Option<u32> {
        self.id()
    }

    fn kill<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + 'a>> {
        Box::pin(tokio::process::Child::kill(self))
    }

    fn wait_for_exit<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + 'a>> {
        Box::pin(async move { self.wait().await.map(|_| ()) })
    }
}

#[cfg(windows)]
async fn cleanup_after_timeout(
    child: &mut dyn ChildHandle,
    grace: Duration,
) -> Result<(), HerdrError> {
    let pid = child.pid();
    match tokio::time::timeout(grace, async {
        let kill_result = child.kill().await;
        let wait_result = child.wait_for_exit().await;
        kill_result.and(wait_result)
    })
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(server_unavailable(error)),
        Err(_) => {
            eprintln!(
                "event=herdr_child_cleanup_timeout cause=child_cleanup_timeout pid={}",
                pid.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
            );
            Err(HerdrError::ServerUnavailable {
                message: "child_cleanup_timeout".to_owned(),
                retry_after: None,
                io_error_kind: None,
            })
        }
    }
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
        decode_command_output(stdout)?,
        decode_command_output(stderr)?,
    ))
}

fn decode_command_output(bytes: Vec<u8>) -> Result<String, HerdrError> {
    #[cfg(windows)]
    {
        return String::from_utf8(bytes).map_err(|_| HerdrError::ProtocolMismatch {
            message: "Herdr process output was not valid UTF-8".to_owned(),
        });
    }

    #[cfg(not(windows))]
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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
    .map_err(|error| HerdrError::ProtocolMismatch {
        message: format!("failed to decode Herdr response: {error}"),
    })?;
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

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use super::server_unavailable;
    use crate::HerdrError;

    #[test]
    fn spawn_not_found_preserves_io_error_kind() {
        assert!(matches!(
            server_unavailable(std::io::Error::from(ErrorKind::NotFound)),
            HerdrError::ServerUnavailable {
                io_error_kind: Some(ErrorKind::NotFound),
                ..
            }
        ));
    }

    #[test]
    fn spawn_permission_denied_preserves_io_error_kind() {
        assert!(matches!(
            server_unavailable(std::io::Error::from(ErrorKind::PermissionDenied)),
            HerdrError::ServerUnavailable {
                io_error_kind: Some(ErrorKind::PermissionDenied),
                ..
            }
        ));
    }

    #[test]
    fn malformed_response_preserves_json_parse_diagnostic() {
        let result = super::decode_envelope(&super::CommandOutput {
            stdout: "{".to_owned(),
            stderr: String::new(),
            success: true,
        });

        match result {
            Err(HerdrError::ProtocolMismatch { message }) => assert!(!message.is_empty()),
            _ => panic!("malformed JSON must preserve a protocol mismatch diagnostic"),
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_binary_resolution_is_per_spawn_and_uses_the_stable_alias() {
        let root = std::env::temp_dir().join(format!("atm-herdr-cli-{}", std::process::id()));
        let alias = root.join("bin");
        std::fs::create_dir_all(&alias).expect("temporary alias directory");

        assert_eq!(
            super::command_binary(Some(&alias)),
            alias.join("herdr.exe"),
            "a directory configuration resolves the executable for this spawn"
        );
        std::fs::remove_dir(&alias).expect("remove alias directory");
        std::fs::write(&alias, b"stand-in executable").expect("replace alias with executable");
        assert_eq!(
            super::command_binary(Some(&alias)),
            alias,
            "the next spawn re-resolves rather than caching the old directory result"
        );
        assert_eq!(
            super::command_binary(None),
            std::path::PathBuf::from("herdr.exe"),
            "an omitted configuration delegates lookup to Windows PATH"
        );
        std::fs::remove_dir_all(root).expect("remove temporary alias root");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_missing_binary_names_its_configured_source() {
        let missing = std::env::temp_dir().join(format!(
            "atm-herdr-missing-{}-{}.exe",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let result = super::run_command(
            Some(&missing),
            &[],
            &[],
            None,
            atm_core::RequestDeadline::after(std::time::Duration::from_secs(1)),
        )
        .await;
        assert!(matches!(
            result,
            Err(crate::HerdrError::ServerUnavailable { ref message, .. })
                if message.contains(&missing.display().to_string())
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_output_requires_utf8_and_accepts_lf_or_crlf() {
        assert_eq!(
            super::decode_command_output(b"{\"result\":{}}\n".to_vec()),
            Ok("{\"result\":{}}\n".to_owned())
        );
        assert_eq!(
            super::decode_command_output(b"{\"result\":{}}\r\n".to_vec()),
            Ok("{\"result\":{}}\r\n".to_owned())
        );
        assert!(matches!(
            super::decode_command_output(vec![0xff]),
            Err(crate::HerdrError::ProtocolMismatch { .. })
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_spawn_site_sets_create_no_window_once() {
        let source = include_str!("transport_cli.rs");
        assert_eq!(super::CREATE_NO_WINDOW, 0x0800_0000);
        assert_eq!(
            source
                .matches("command.creation_flags(CREATE_NO_WINDOW)")
                .count(),
            1
        );
    }

    #[cfg(windows)]
    struct DelayedWaitChild {
        killed: bool,
    }

    #[cfg(windows)]
    impl super::ChildHandle for DelayedWaitChild {
        fn pid(&self) -> Option<u32> {
            Some(42)
        }

        fn kill<'a>(
            &'a mut self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + 'a>>
        {
            self.killed = true;
            Box::pin(async { Ok(()) })
        }

        fn wait_for_exit<'a>(
            &'a mut self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + 'a>>
        {
            Box::pin(std::future::pending())
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_cleanup_grace_returns_typed_failure_when_wait_stalls() {
        let mut child = DelayedWaitChild { killed: false };
        let result = super::cleanup_after_timeout(&mut child, std::time::Duration::ZERO).await;
        assert!(child.killed);
        assert_eq!(
            super::WINDOWS_CHILD_CLEANUP_GRACE,
            std::time::Duration::from_secs(5)
        );
        assert!(matches!(
            result,
            Err(crate::HerdrError::ServerUnavailable { ref message, .. })
                if message == "child_cleanup_timeout"
        ));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_timeout_cleanup_kills_and_reaps_a_real_child() {
        let mut child = tokio::process::Command::new("cmd.exe")
            .args(["/C", "timeout", "/T", "60", "/NOBREAK"])
            .spawn()
            .expect("long-running Windows stand-in child");
        super::cleanup_after_timeout(&mut child, super::WINDOWS_CHILD_CLEANUP_GRACE)
            .await
            .expect("kill then reap completes inside the cleanup grace period");
        assert!(
            child
                .try_wait()
                .expect("inspect child exit after reap")
                .is_some()
        );
    }
}
