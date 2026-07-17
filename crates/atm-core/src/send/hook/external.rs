use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use super::payload::{HookExecution, prepare_post_send_hook_execution};
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct PostSendHookResult {
    pub(super) level: PostSendHookResultLevel,
    pub(super) message: String,
    #[serde(default)]
    pub(super) fields: Map<String, Value>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum PostSendHookResultLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Default)]
pub(super) struct HookCancellationToken(pub(super) Arc<AtomicBool>);

impl HookCancellationToken {
    pub(super) fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub(super) fn run_post_send_hooks_for_cli(
    warnings: &mut Vec<WarningEntry>,
    config: &AtmConfig,
    event: &PostSendHookEvent,
) -> HookExecutionSummary {
    let matching_rules: Vec<_> = config
        .post_send_hooks
        .iter()
        .filter(|rule| hook_matches_recipient(&rule.recipient, &event.recipient))
        .collect();

    if matching_rules.is_empty() {
        debug!(
            sender = %event.sender,
            recipient = %event.recipient,
            recipient_team = %event.recipient_team,
            "post-send hook had no matching recipient rules"
        );
        return HookExecutionSummary::new(0, 0, 0).expect("zero summary");
    }

    let mut succeeded_rules = 0usize;
    let mut failed_rules = 0usize;
    for rule in matching_rules {
        if execute_post_send_hook(warnings, config, rule, event) {
            succeeded_rules += 1;
        } else {
            failed_rules += 1;
        }
    }
    HookExecutionSummary::new(
        succeeded_rules + failed_rules,
        succeeded_rules,
        failed_rules,
    )
    .expect("validated hook execution summary")
}

fn execute_post_send_hook(
    warnings: &mut Vec<WarningEntry>,
    config: &AtmConfig,
    rule: &config::types::PostSendHookRule,
    event: &PostSendHookEvent,
) -> bool {
    let Some(execution) = prepare_post_send_hook_execution(config, rule, event) else {
        return false;
    };
    let mut child = match spawn_post_send_hook_process(config, &execution, event, rule, warnings) {
        Some(child) => child,
        None => return false,
    };
    let stdout_cancellation = HookCancellationToken::default();
    let mut stdout_reader =
        spawn_post_send_hook_stdout_reader(&mut child, stdout_cancellation.clone());

    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return handle_post_send_hook_exit(
                    status,
                    stdout_reader.take(),
                    &execution.command_path,
                    warnings,
                    event,
                    rule,
                );
            }
            Ok(None) if started_at.elapsed() < POST_SEND_HOOK_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                handle_post_send_hook_timeout(
                    &mut child,
                    stdout_reader.take(),
                    &stdout_cancellation,
                    &execution.command_path,
                    warnings,
                    event,
                    rule,
                );
                return false;
            }
            Err(error) => {
                handle_post_send_hook_status_error(
                    error,
                    HookStatusFailureArgs {
                        child: &mut child,
                        stdout_reader: stdout_reader.take(),
                        stdout_cancellation: &stdout_cancellation,
                        command_path: &execution.command_path,
                        warnings,
                        context: event,
                        rule,
                    },
                );
                return false;
            }
        }
    }
}

fn spawn_post_send_hook_process(
    config: &AtmConfig,
    execution: &HookExecution,
    event: &PostSendHookEvent,
    rule: &config::types::PostSendHookRule,
    warnings: &mut Vec<WarningEntry>,
) -> Option<std::process::Child> {
    debug!(
        sender = %event.sender,
        recipient = %event.recipient,
        recipient_team = %event.recipient_team,
        hook_recipient = %rule.recipient,
        hook_path = %execution.command_path.display(),
        "post-send hook matched recipient rule"
    );

    let mut command = Command::new(&execution.command_path);
    command
        .args(&execution.argv)
        .current_dir(&config.config_root)
        .env("ATM_POST_SEND", execution.payload.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    match command.spawn() {
        Ok(child) => Some(child),
        Err(error) => {
            warn_post_send_hook_start_failure(
                &execution.command_path,
                &error,
                event,
                rule,
                warnings,
            );
            None
        }
    }
}

fn warn_post_send_hook_start_failure(
    command_path: &Path,
    error: &std::io::Error,
    event: &PostSendHookEvent,
    rule: &config::types::PostSendHookRule,
    warnings: &mut Vec<WarningEntry>,
) {
    warn!(
        code = %AtmErrorCode::WarningHookExecutionFailed,
        sender = %event.sender,
        recipient = %event.recipient,
        recipient_team = %event.recipient_team,
        hook_recipient = %rule.recipient,
        hook_path = %command_path.display(),
        %error,
        "post-send hook failed to start"
    );
    warnings.push(WarningEntry::with_code(
        AtmErrorCode::WarningHookExecutionFailed,
        format!(
            "warning: post-send hook failed to start from {}: {error}.",
            command_path.display()
        ),
        Some("Check that the hook command in .atm.toml points to a valid executable."),
    ));
}

fn handle_post_send_hook_exit(
    status: std::process::ExitStatus,
    stdout_reader: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    command_path: &Path,
    warnings: &mut Vec<WarningEntry>,
    event: &PostSendHookEvent,
    rule: &config::types::PostSendHookRule,
) -> bool {
    maybe_log_post_send_hook_result(
        command_path,
        finish_post_send_hook_stdout_capture(stdout_reader, command_path),
    );
    if !status.success() {
        warn_post_send_hook_exit_failure(command_path, status, warnings, event, rule);
        return false;
    }
    true
}

fn warn_post_send_hook_exit_failure(
    command_path: &Path,
    status: std::process::ExitStatus,
    warnings: &mut Vec<WarningEntry>,
    event: &PostSendHookEvent,
    rule: &config::types::PostSendHookRule,
) {
    warn!(
        code = %AtmErrorCode::WarningHookExecutionFailed,
        sender = %event.sender,
        recipient = %event.recipient,
        recipient_team = %event.recipient_team,
        hook_recipient = %rule.recipient,
        hook_path = %command_path.display(),
        %status,
        "post-send hook exited unsuccessfully"
    );
    warnings.push(WarningEntry::with_code(
        AtmErrorCode::WarningHookExecutionFailed,
        format!(
            "warning: post-send hook exited unsuccessfully from {} with status {status}.",
            command_path.display()
        ),
        Some("Check the hook script for errors; it exited with a non-zero status."),
    ));
}

fn handle_post_send_hook_timeout(
    child: &mut std::process::Child,
    stdout_reader: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    stdout_cancellation: &HookCancellationToken,
    command_path: &Path,
    warnings: &mut Vec<WarningEntry>,
    event: &PostSendHookEvent,
    rule: &config::types::PostSendHookRule,
) {
    terminate_post_send_hook_process(child, command_path);
    abandon_post_send_hook_stdout_capture(stdout_reader, stdout_cancellation, command_path);
    warn!(
        code = %AtmErrorCode::WarningHookExecutionFailed,
        sender = %event.sender,
        recipient = %event.recipient,
        recipient_team = %event.recipient_team,
        hook_recipient = %rule.recipient,
        hook_path = %command_path.display(),
        timeout_seconds = POST_SEND_HOOK_TIMEOUT.as_secs(),
        "post-send hook timed out"
    );
    warnings.push(WarningEntry::with_code(
        AtmErrorCode::WarningHookExecutionFailed,
        format!(
            "warning: post-send hook timed out after {}s for {}.",
            POST_SEND_HOOK_TIMEOUT.as_secs(),
            command_path.display()
        ),
        Some("The hook script exceeded the 5-second timeout; ensure it exits promptly."),
    ));
}

struct HookStatusFailureArgs<'a> {
    child: &'a mut std::process::Child,
    stdout_reader: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    stdout_cancellation: &'a HookCancellationToken,
    command_path: &'a Path,
    warnings: &'a mut Vec<WarningEntry>,
    context: &'a PostSendHookEvent,
    rule: &'a config::types::PostSendHookRule,
}

fn handle_post_send_hook_status_error(error: std::io::Error, args: HookStatusFailureArgs<'_>) {
    terminate_post_send_hook_process(args.child, args.command_path);
    abandon_post_send_hook_stdout_capture(
        args.stdout_reader,
        args.stdout_cancellation,
        args.command_path,
    );
    warn!(
        code = %AtmErrorCode::WarningHookExecutionFailed,
        sender = %args.context.sender,
        recipient = %args.context.recipient,
        recipient_team = %args.context.recipient_team,
        hook_recipient = %args.rule.recipient,
        hook_path = %args.command_path.display(),
        %error,
        "post-send hook status check failed"
    );
    args.warnings.push(WarningEntry::with_code(
        AtmErrorCode::WarningHookExecutionFailed,
        format!(
            "warning: post-send hook status check failed for {}: {error}.",
            args.command_path.display()
        ),
        Some("This is an OS-level error; check that the hook process is not being killed externally."),
    ));
}

fn terminate_post_send_hook_process(child: &mut std::process::Child, command_path: &Path) {
    if let Err(error) = child.kill()
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        warn!(
            code = %AtmErrorCode::WarningHookExecutionFailed,
            hook_path = %command_path.display(),
            %error,
            "failed to terminate post-send hook child process"
        );
    }

    if let Err(error) = child.wait() {
        warn!(
            code = %AtmErrorCode::WarningHookExecutionFailed,
            hook_path = %command_path.display(),
            %error,
            "failed to reap post-send hook child process"
        );
    }
}

fn spawn_post_send_hook_stdout_reader(
    child: &mut std::process::Child,
    cancellation: HookCancellationToken,
) -> Option<thread::JoinHandle<std::io::Result<Vec<u8>>>> {
    let mut stdout = child.stdout.take()?;
    Some(thread::spawn(move || {
        let mut captured = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            if cancellation.is_cancelled() {
                break;
            }
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

pub(super) fn finish_abandoned_post_send_hook_stdout_capture(
    mut stdout_reader: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    command_path: &Path,
) {
    let Some(handle) = stdout_reader.as_ref() else {
        return;
    };

    let deadline = Instant::now() + POST_SEND_HOOK_STDOUT_JOIN_TIMEOUT;
    while !handle.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }

    let Some(stdout_reader) = stdout_reader.take() else {
        return;
    };
    if !stdout_reader.is_finished() {
        debug!(
            hook_path = %command_path.display(),
            join_timeout_ms = POST_SEND_HOOK_STDOUT_JOIN_TIMEOUT.as_millis(),
            "post-send hook stdout reader did not exit before the bounded teardown deadline"
        );
        return;
    }

    match stdout_reader.join() {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            warn!(
                code = %AtmErrorCode::WarningHookExecutionFailed,
                hook_path = %command_path.display(),
                %error,
                "post-send hook stdout capture failed during bounded teardown"
            );
        }
        Err(_) => {
            warn!(
                code = %AtmErrorCode::WarningHookExecutionFailed,
                hook_path = %command_path.display(),
                "post-send hook stdout capture panicked during bounded teardown"
            );
        }
    }
}

fn abandon_post_send_hook_stdout_capture(
    stdout_reader: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    cancellation: &HookCancellationToken,
    command_path: &Path,
) {
    cancellation.cancel();
    finish_abandoned_post_send_hook_stdout_capture(stdout_reader, command_path);
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

pub(super) fn parse_post_send_hook_result(
    command_path: &Path,
    stdout: &[u8],
) -> Option<PostSendHookResult> {
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
pub(super) fn hook_result_log_level(level: PostSendHookResultLevel) -> Level {
    match level {
        PostSendHookResultLevel::Debug => Level::DEBUG,
        PostSendHookResultLevel::Info => Level::INFO,
        PostSendHookResultLevel::Warn => Level::WARN,
        PostSendHookResultLevel::Error => Level::ERROR,
    }
}
