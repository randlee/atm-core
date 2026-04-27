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
    if config.post_send_hooks.is_empty() {
        debug!(
            sender = context.sender,
            recipient = %context.recipient.agent,
            recipient_team = %context.recipient.team,
            "post-send hook disabled because no recipient-scoped rules are configured"
        );
        return;
    }

    let mut matched = false;
    for rule in &config.post_send_hooks {
        if !recipient_rule_matches(&rule.recipient, context.recipient.agent.as_str()) {
            continue;
        }

        matched = true;
        debug!(
            sender = context.sender,
            recipient = %context.recipient.agent,
            recipient_team = %context.recipient.team,
            rule_recipient = rule.recipient,
            "post-send hook matched recipient rule"
        );
        run_post_send_hook_rule(warnings, config, context, &rule.command);
    }

    if !matched {
        debug!(
            sender = context.sender,
            recipient = %context.recipient.agent,
            recipient_team = %context.recipient.team,
            configured_recipients = %display_rule_recipients(&config.post_send_hooks),
            "post-send hook did not match any configured recipient rule"
        );
    }
}

fn resolve_command_path(config: &config::AtmConfig, command_path: &str) -> PathBuf {
    let path = PathBuf::from(command_path);
    if path.is_absolute() || !command_path_contains_path_separator(command_path) {
        path
    } else {
        config.config_root.join(path)
    }
}

fn command_path_contains_path_separator(command_path: &str) -> bool {
    command_path.contains('/') || command_path.contains('\\')
}

fn run_post_send_hook_rule(
    warnings: &mut Vec<String>,
    config: &config::AtmConfig,
    context: PostSendHookContext<'_>,
    command_argv: &[String],
) {
    let Some((command_path, argv)) = command_argv.split_first() else {
        return;
    };
    let command_path = resolve_command_path(config, command_path);
    let mut payload = json!({
        "from": qualified_sender_identity(context.sender, context.sender_team),
        "to": format!("{}@{}", context.recipient.agent, context.recipient.team),
        "sender": context.sender,
        "recipient": context.recipient.agent.as_str(),
        "team": context.recipient.team.as_str(),
        "message_id": context.message_id.to_string(),
        "requires_ack": context.requires_ack,
    });
    if let Some(task_id) = context.task_id {
        payload["task_id"] = Value::String(task_id.to_string());
    }

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
                "warning: post-send hook failed to start from {}: {error}. Check that [[atm.post_send_hooks]].command points to a valid executable.",
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

fn recipient_rule_matches(recipient_rule: &str, candidate: &str) -> bool {
    recipient_rule == "*" || recipient_rule == candidate
}

fn display_rule_recipients(rules: &[config::types::PostSendHookRule]) -> String {
    if rules.is_empty() {
        "(not configured)".to_string()
    } else {
        rules
            .iter()
            .map(|rule| rule.recipient.as_str())
            .collect::<Vec<_>>()
            .join(", ")
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
            warn!(code = %AtmErrorCode::WarningHookExecutionFailed,
                hook_path = %command_path.display(),
                %error,
                "post-send hook stdout capture failed"
            );
            None
        }
        Err(_) => {
            warn!(code = %AtmErrorCode::WarningHookExecutionFailed,
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
            code = %AtmErrorCode::WarningHookExecutionFailed,
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
    use std::env;
    use std::path::Path;

    use serde_json::json;
    use tracing::Level;

    use super::{
        POST_SEND_HOOK_MAX_STDOUT_BYTES, PostSendHookResultLevel,
        command_path_contains_path_separator, display_rule_recipients, hook_result_log_level,
        parse_post_send_hook_result, recipient_rule_matches, resolve_command_path,
    };

    fn test_config_root() -> std::path::PathBuf {
        env::temp_dir().join("atm-config-root")
    }

    #[test]
    fn recipient_rule_matches_exact_and_wildcard_values() {
        assert!(recipient_rule_matches("arch-ctm", "arch-ctm"));
        assert!(recipient_rule_matches("*", "arch-ctm"));
        assert!(!recipient_rule_matches("team-lead", "arch-ctm"));
    }

    #[test]
    fn command_path_contains_path_separator_matches_path_like_commands_only() {
        assert!(command_path_contains_path_separator("scripts/hook.sh"));
        assert!(command_path_contains_path_separator(r"scripts\hook.bat"));
        assert!(!command_path_contains_path_separator("bash"));
    }

    #[test]
    fn resolve_command_path_preserves_absolute_paths() {
        let config = crate::config::AtmConfig {
            config_root: test_config_root(),
            ..Default::default()
        };

        let abs = std::env::temp_dir().join("hook");
        assert_eq!(
            resolve_command_path(&config, abs.to_str().unwrap()),
            abs.as_path()
        );
    }

    #[test]
    fn resolve_command_path_joins_relative_paths_with_separators_under_config_root() {
        let config = crate::config::AtmConfig {
            config_root: test_config_root(),
            ..Default::default()
        };

        assert_eq!(
            resolve_command_path(&config, "scripts/hook.sh"),
            test_config_root().join("scripts/hook.sh")
        );
    }

    #[test]
    fn resolve_command_path_preserves_bare_command_names_for_path_lookup() {
        let config = crate::config::AtmConfig {
            config_root: test_config_root(),
            ..Default::default()
        };

        assert_eq!(resolve_command_path(&config, "bash"), Path::new("bash"));
    }

    #[test]
    fn display_rule_recipients_renders_empty_as_not_configured() {
        assert_eq!(display_rule_recipients(&[]), "(not configured)");
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
