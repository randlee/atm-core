use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Map, Value, json};
use tracing::{Level, debug, error, info, warn};

use crate::config;
use crate::error::AtmErrorCode;

use super::{PostSendHookContext, qualified_sender_identity};

const POST_SEND_HOOK_TIMEOUT: Duration = Duration::from_secs(5);
const POST_SEND_HOOK_MAX_STDOUT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy)]
struct PostSendHookMatch {
    sender: bool,
    recipient: bool,
}

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
    let Some(command_argv) = config.post_send_hook.as_ref() else {
        return;
    };

    let hook_match = PostSendHookMatch {
        sender: matches_hook_axis(&config.post_send_hook_senders, context.sender),
        recipient: matches_hook_axis(&config.post_send_hook_recipients, &context.recipient.agent),
    };
    if !hook_match.sender && !hook_match.recipient {
        let warning = format_post_send_hook_skipped_warning(
            context.sender,
            &context.recipient.agent,
            &config.post_send_hook_senders,
            &config.post_send_hook_recipients,
        );
        warn!(
            code = %AtmErrorCode::WarningHookSkipped,
            sender = context.sender,
            recipient = %context.recipient.agent,
            recipient_team = %context.recipient.team,
            sender_filters = %display_filter_list(&config.post_send_hook_senders),
            recipient_filters = %display_filter_list(&config.post_send_hook_recipients),
            sender_match = hook_match.sender,
            recipient_match = hook_match.recipient,
            "post-send hook skipped"
        );
        warnings.push(warning);
        return;
    }

    debug!(
        sender = context.sender,
        recipient = %context.recipient.agent,
        recipient_team = %context.recipient.team,
        sender_filters = %display_filter_list(&config.post_send_hook_senders),
        recipient_filters = %display_filter_list(&config.post_send_hook_recipients),
        sender_match = hook_match.sender,
        recipient_match = hook_match.recipient,
        "post-send hook matched"
    );

    let mut argv = command_argv.iter();
    let Some(command_path) = argv.next() else {
        return;
    };
    let command_path = resolve_command_path(config, command_path);
    let payload = json!({
        "from": qualified_sender_identity(context.sender, context.sender_team),
        "to": format!("{}@{}", context.recipient.agent, context.recipient.team),
        "message_id": context.message_id.to_string(),
        "requires_ack": context.requires_ack,
        "task_id": context.task_id,
        "hook_match": {
            "sender": hook_match.sender,
            "recipient": hook_match.recipient,
        },
    });

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
                "warning: post-send hook failed to start from {}: {error}. Check that post_send_hook in .atm.toml points to a valid executable.",
                command_path.display()
            );
            warn!(
                code = %AtmErrorCode::WarningHookExecutionFailed,
                sender = context.sender,
                recipient = %context.recipient.agent,
                recipient_team = %context.recipient.team,
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
                        sender = context.sender,
                        recipient = %context.recipient.agent,
                        recipient_team = %context.recipient.team,
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
                    sender = context.sender,
                    recipient = %context.recipient.agent,
                    recipient_team = %context.recipient.team,
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
                    sender = context.sender,
                    recipient = %context.recipient.agent,
                    recipient_team = %context.recipient.team,
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
    if path.is_absolute() {
        path
    } else {
        config.config_root.join(path)
    }
}

fn matches_hook_axis(filters: &[String], candidate: &str) -> bool {
    filters.is_empty() || hook_filter_matches(filters, candidate)
}

fn hook_filter_matches(filters: &[String], candidate: &str) -> bool {
    filters
        .iter()
        .any(|filter| filter == "*" || filter == candidate)
}

fn format_post_send_hook_skipped_warning(
    sender: &str,
    recipient: &str,
    senders: &[String],
    recipients: &[String],
) -> String {
    format!(
        "post-send hook skipped: sender {sender} not in post_send_hook_senders {}\nand recipient {recipient} not in post_send_hook_recipients {}",
        display_filter_list(senders),
        display_filter_list(recipients)
    )
}

fn display_filter_list(filters: &[String]) -> String {
    if filters.is_empty() {
        "*".to_string()
    } else {
        filters.join(", ")
    }
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

    match hook_result_log_level(level) {
        Level::DEBUG => debug!(
            hook_path = %command_path.display(),
            hook_result_message = %message,
            hook_result_fields = %fields,
            "post-send hook reported result"
        ),
        Level::INFO => info!(
            hook_path = %command_path.display(),
            hook_result_message = %message,
            hook_result_fields = %fields,
            "post-send hook reported result"
        ),
        Level::WARN => warn!(
            hook_path = %command_path.display(),
            hook_result_message = %message,
            hook_result_fields = %fields,
            "post-send hook reported warning"
        ),
        Level::ERROR => error!(
            hook_path = %command_path.display(),
            hook_result_message = %message,
            hook_result_fields = %fields,
            "post-send hook reported error"
        ),
        _ => unreachable!("all tracing levels are covered"),
    }
}

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
        POST_SEND_HOOK_MAX_STDOUT_BYTES, PostSendHookResultLevel,
        format_post_send_hook_skipped_warning, hook_filter_matches, hook_result_log_level,
        matches_hook_axis, parse_post_send_hook_result,
    };

    #[test]
    fn hook_filter_matches_exact_and_wildcard_values() {
        assert!(hook_filter_matches(&["arch-ctm".to_string()], "arch-ctm"));
        assert!(hook_filter_matches(&["*".to_string()], "arch-ctm"));
        assert!(!hook_filter_matches(&["team-lead".to_string()], "arch-ctm"));
    }

    #[test]
    fn matches_hook_axis_treats_empty_filter_list_as_unconditional() {
        assert!(matches_hook_axis(&[], "arch-ctm"));
    }

    #[test]
    fn format_post_send_hook_skipped_warning_uses_documented_template() {
        let warning = format_post_send_hook_skipped_warning(
            "arch-ctm",
            "recipient",
            &["team-lead".to_string()],
            &["quality-mgr".to_string()],
        );

        assert_eq!(
            warning,
            "post-send hook skipped: sender arch-ctm not in post_send_hook_senders team-lead\nand recipient recipient not in post_send_hook_recipients quality-mgr"
        );
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
