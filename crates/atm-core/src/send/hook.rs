use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Map, Value, json};
#[cfg(test)]
use tracing::Level;
use tracing::{debug, error, info, warn};

use crate::config;
use crate::config::types::HookRecipient;
use crate::error::AtmErrorCode;

use super::{PostSendHookContext, qualified_sender_identity};

const POST_SEND_HOOK_TIMEOUT: Duration = Duration::from_secs(5);
const POST_SEND_HOOK_MAX_STDOUT_BYTES: usize = 8 * 1024;

#[derive(Debug, Deserialize)]
struct PostSendHookResult {
    level: PostSendHookResultLevel,
    message: String,
    #[serde(default)]
    fields: Map<String, Value>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PostSendHookResultLevel {
    Debug,
    Info,
    Warn,
    Error,
}

pub(super) fn maybe_run_post_send_hook(
    warnings: &mut Vec<String>,
    config: Option<&config::AtmConfig>,
    context: PostSendHookContext<'_>,
) {
    let Some(config) = config else {
        return;
    };

    let matching_rules: Vec<_> = config
        .post_send_hooks
        .iter()
        .filter(|rule| hook_matches_recipient(&rule.recipient, &context.recipient.agent))
        .collect();

    if matching_rules.is_empty() {
        debug!(
            sender = %context.sender,
            recipient = %context.recipient.agent,
            recipient_team = %context.recipient.team,
            "post-send hook had no matching recipient rules"
        );
        return;
    }

    for rule in matching_rules {
        execute_post_send_hook(warnings, config, rule, &context);
    }
}

fn execute_post_send_hook(
    warnings: &mut Vec<String>,
    config: &config::AtmConfig,
    rule: &config::types::PostSendHookRule,
    context: &PostSendHookContext<'_>,
) {
    let mut argv = rule.command.iter();
    let Some(command_path) = argv.next() else {
        return;
    };
    let command_path = resolve_command_path(config, command_path);
    let mut payload = json!({
        "from": qualified_sender_identity(context.sender, context.sender_team.map(|team| team.as_str())),
        "to": format!("{}@{}", context.recipient.agent, context.recipient.team),
        "sender": context.sender.as_str(),
        "recipient": context.recipient.agent,
        "team": context.recipient.team,
        "message_id": context.message_id.to_string(),
        "requires_ack": context.requires_ack,
    });
    if let Some(task_id) = context.task_id {
        payload["task_id"] = Value::String(task_id.to_string());
    }

    debug!(
        sender = %context.sender,
        recipient = %context.recipient.agent,
        recipient_team = %context.recipient.team,
        hook_recipient = %rule.recipient,
        hook_path = %command_path.display(),
        "post-send hook matched recipient rule"
    );

    let mut command = Command::new(&command_path);
    command
        .args(argv)
        .current_dir(&config.config_root)
        .env("ATM_POST_SEND", payload.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let warning = format!(
                "warning: post-send hook failed to start from {}: {error}. Check that the hook command in .atm.toml points to a valid executable.",
                command_path.display()
            );
            warn!(
                code = %AtmErrorCode::WarningHookExecutionFailed,
                sender = %context.sender,
                recipient = %context.recipient.agent,
                recipient_team = %context.recipient.team,
                hook_recipient = %rule.recipient,
                hook_path = %command_path.display(),
                %error,
                "post-send hook failed to start"
            );
            warnings.push(warning);
            return;
        }
    };
    let mut stdout_reader = spawn_post_send_hook_stdout_reader(&mut child);

    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                maybe_log_post_send_hook_result(
                    &command_path,
                    finish_post_send_hook_stdout_capture(stdout_reader.take(), &command_path),
                );
                if !status.success() {
                    let warning = format!(
                        "warning: post-send hook exited unsuccessfully from {} with status {status}. Check the hook script for errors; it exited with a non-zero status.",
                        command_path.display()
                    );
                    warn!(
                        code = %AtmErrorCode::WarningHookExecutionFailed,
                        sender = %context.sender,
                        recipient = %context.recipient.agent,
                        recipient_team = %context.recipient.team,
                        hook_recipient = %rule.recipient,
                        hook_path = %command_path.display(),
                        %status,
                        "post-send hook exited unsuccessfully"
                    );
                    warnings.push(warning);
                }
                return;
            }
            Ok(None) if started_at.elapsed() < POST_SEND_HOOK_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                maybe_log_post_send_hook_result(
                    &command_path,
                    finish_post_send_hook_stdout_capture(stdout_reader.take(), &command_path),
                );
                let warning = format!(
                    "warning: post-send hook timed out after {}s for {}. The hook script exceeded the 5-second timeout; ensure it exits promptly.",
                    POST_SEND_HOOK_TIMEOUT.as_secs(),
                    command_path.display()
                );
                warn!(
                    code = %AtmErrorCode::WarningHookExecutionFailed,
                    sender = %context.sender,
                    recipient = %context.recipient.agent,
                    recipient_team = %context.recipient.team,
                    hook_recipient = %rule.recipient,
                    hook_path = %command_path.display(),
                    timeout_seconds = POST_SEND_HOOK_TIMEOUT.as_secs(),
                    "post-send hook timed out"
                );
                warnings.push(warning);
                return;
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                maybe_log_post_send_hook_result(
                    &command_path,
                    finish_post_send_hook_stdout_capture(stdout_reader.take(), &command_path),
                );
                let warning = format!(
                    "warning: post-send hook status check failed for {}: {error}. This is an OS-level error; check that the hook process is not being killed externally.",
                    command_path.display()
                );
                warn!(
                    code = %AtmErrorCode::WarningHookExecutionFailed,
                    sender = %context.sender,
                    recipient = %context.recipient.agent,
                    recipient_team = %context.recipient.team,
                    hook_recipient = %rule.recipient,
                    hook_path = %command_path.display(),
                    %error,
                    "post-send hook status check failed"
                );
                warnings.push(warning);
                return;
            }
        }
    }
}

fn resolve_command_path(config: &config::AtmConfig, command_path: &str) -> PathBuf {
    let path = PathBuf::from(command_path);
    if path.is_absolute() || !config::discovery::command_looks_like_path(command_path) {
        path
    } else {
        config.config_root.join(path)
    }
}

fn hook_matches_recipient(configured: &HookRecipient, candidate: &crate::types::AgentName) -> bool {
    configured.matches(candidate)
}

fn spawn_post_send_hook_stdout_reader(
    child: &mut std::process::Child,
) -> Option<thread::JoinHandle<std::io::Result<Vec<u8>>>> {
    let mut stdout = child.stdout.take()?;
    Some(thread::spawn(move || {
        let mut captured = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = stdout.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            if captured.len() <= POST_SEND_HOOK_MAX_STDOUT_BYTES {
                let remaining = POST_SEND_HOOK_MAX_STDOUT_BYTES + 1 - captured.len();
                let to_copy = remaining.min(read);
                captured.extend_from_slice(&chunk[..to_copy]);
            }
        }
        Ok(captured)
    }))
}

fn finish_post_send_hook_stdout_capture(
    stdout_reader: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    command_path: &Path,
) -> Option<Vec<u8>> {
    let stdout_reader = stdout_reader?;
    match stdout_reader.join() {
        Ok(Ok(stdout)) => Some(stdout),
        Ok(Err(error)) => {
            warn!(
                code = %AtmErrorCode::WarningHookExecutionFailed,
                hook_path = %command_path.display(),
                %error,
                "post-send hook stdout capture failed"
            );
            None
        }
        Err(_) => {
            warn!(
                code = %AtmErrorCode::WarningHookExecutionFailed,
                hook_path = %command_path.display(),
                "post-send hook stdout capture panicked"
            );
            None
        }
    }
}

fn maybe_log_post_send_hook_result(command_path: &Path, stdout: Option<Vec<u8>>) {
    let Some(stdout) = stdout else {
        return;
    };
    let Some(result) = parse_post_send_hook_result(command_path, &stdout) else {
        return;
    };
    log_post_send_hook_result(command_path, result);
}

fn parse_post_send_hook_result(command_path: &Path, stdout: &[u8]) -> Option<PostSendHookResult> {
    if stdout.is_empty() {
        return None;
    }
    if stdout.len() > POST_SEND_HOOK_MAX_STDOUT_BYTES {
        debug!(
            hook_path = %command_path.display(),
            max_stdout_bytes = POST_SEND_HOOK_MAX_STDOUT_BYTES,
            "ignoring post-send hook stdout because it exceeded the capture limit"
        );
        return None;
    }

    let rendered = match std::str::from_utf8(stdout) {
        Ok(rendered) => rendered.trim(),
        Err(error) => {
            debug!(
                hook_path = %command_path.display(),
                %error,
                "ignoring post-send hook stdout because it was not valid UTF-8"
            );
            return None;
        }
    };
    if rendered.is_empty() {
        return None;
    }

    match serde_json::from_str::<PostSendHookResult>(rendered) {
        Ok(result) => Some(result),
        Err(error) => {
            debug!(
                hook_path = %command_path.display(),
                %error,
                "ignoring post-send hook stdout because it did not match the hook-result schema"
            );
            None
        }
    }
}

fn log_post_send_hook_result(command_path: &Path, result: PostSendHookResult) {
    let PostSendHookResult {
        level,
        message,
        fields,
    } = result;
    let fields = Value::Object(fields);

    match level {
        PostSendHookResultLevel::Debug => debug!(
            hook_path = %command_path.display(),
            hook_result_message = %message,
            hook_result_fields = %fields,
            "post-send hook reported result"
        ),
        PostSendHookResultLevel::Info => info!(
            hook_path = %command_path.display(),
            hook_result_message = %message,
            hook_result_fields = %fields,
            "post-send hook reported result"
        ),
        PostSendHookResultLevel::Warn => warn!(
            code = %AtmErrorCode::WarningHookExecutionFailed,
            hook_path = %command_path.display(),
            hook_result_message = %message,
            hook_result_fields = %fields,
            "post-send hook reported warning"
        ),
        PostSendHookResultLevel::Error => error!(
            hook_path = %command_path.display(),
            hook_result_message = %message,
            hook_result_fields = %fields,
            "post-send hook reported error"
        ),
    }
}

#[cfg(test)]
fn hook_result_log_level(level: PostSendHookResultLevel) -> Level {
    match level {
        PostSendHookResultLevel::Debug => Level::DEBUG,
        PostSendHookResultLevel::Info => Level::INFO,
        PostSendHookResultLevel::Warn => Level::WARN,
        PostSendHookResultLevel::Error => Level::ERROR,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;
    use tracing::Level;

    use super::{
        POST_SEND_HOOK_MAX_STDOUT_BYTES, PostSendHookResultLevel, hook_matches_recipient,
        hook_result_log_level, parse_post_send_hook_result,
    };
    use crate::config::types::HookRecipient;

    #[test]
    fn hook_matches_recipient_exact_and_wildcard_values() {
        assert!(hook_matches_recipient(
            &HookRecipient::Named("arch-ctm".parse().expect("recipient")),
            &"arch-ctm".parse().expect("candidate")
        ));
        assert!(hook_matches_recipient(
            &HookRecipient::Wildcard,
            &"arch-ctm".parse().expect("candidate")
        ));
        assert!(!hook_matches_recipient(
            &HookRecipient::Named("team-lead".parse().expect("recipient")),
            &"arch-ctm".parse().expect("candidate")
        ));
    }

    #[test]
    fn parse_post_send_hook_result_accepts_valid_json_object() {
        let parsed = parse_post_send_hook_result(
            Path::new("hook"),
            br#"{"level":"debug","message":"nudged","fields":{"pane_id":"%42"}}"#,
        )
        .expect("valid hook result");

        assert_eq!(parsed.message, "nudged");
        assert_eq!(parsed.fields["pane_id"], json!("%42"));
    }

    #[test]
    fn parse_post_send_hook_result_ignores_invalid_schema() {
        let parsed =
            parse_post_send_hook_result(Path::new("hook"), br#"{"level":"trace","message":"x"}"#);

        assert!(parsed.is_none());
    }

    #[test]
    fn parse_post_send_hook_result_ignores_oversized_stdout() {
        let oversized = vec![b'a'; POST_SEND_HOOK_MAX_STDOUT_BYTES + 1];
        let parsed = parse_post_send_hook_result(Path::new("hook"), &oversized);

        assert!(parsed.is_none());
    }

    #[test]
    fn error_hook_results_map_to_error_level() {
        assert_eq!(
            hook_result_log_level(PostSendHookResultLevel::Error),
            Level::ERROR
        );
    }
}
