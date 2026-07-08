use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Map, Value, json};
#[cfg(test)]
use tracing::Level;
use tracing::{debug, error, info, warn};

use super::{POST_SEND_HOOK_TIMEOUT, ResolvedRecipient, WarningEntry, qualified_sender_identity};
use crate::boundary::PostSendHookEvent;
use crate::config::types::HookRecipient;
use crate::config::{self, AtmConfig};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::protocol::{NotificationEvent, NotificationKind};
use crate::schema::compatible_home_dir;
use crate::service_runtime::append_notification_log;
use crate::types::{AgentName, TeamName};

const POST_SEND_HOOK_MAX_STDOUT_BYTES: usize = 8 * 1024;
const POST_SEND_HOOK_STDOUT_JOIN_TIMEOUT: Duration = Duration::from_millis(500);
const ATM_PROGRAM_ENV: &str = "ATM_TEST_ATM_BIN";
const INTERNAL_NUDGE_SINK_ENV: &str = "ATM_INTERNAL_NUDGE_SINK";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuiltInNudgeSinkTarget {
    Tmux,
    Graft,
}

impl BuiltInNudgeSinkTarget {
    fn env_value(self) -> &'static str {
        match self {
            Self::Tmux => "tmux",
            Self::Graft => "graft",
        }
    }
}

#[derive(Clone, Default)]
struct HookCancellationToken(Arc<AtomicBool>);

impl HookCancellationToken {
    fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub(crate) fn emit_post_send_effects(
    warnings: &mut Vec<WarningEntry>,
    config: Option<&AtmConfig>,
    _graft_port: Option<&dyn crate::boundary::GraftPostSendPort>,
    recipient: &ResolvedRecipient,
    delivery_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
    messages: &[crate::delivery_plan::LogicalMessage],
) {
    for message in messages {
        let event = post_send_event_from_message(
            recipient,
            message,
            delivery_snapshot.recipient_pane_id.as_ref(),
        );
        let mut emitted = false;
        let hook_matched = config.is_some_and(|loaded| {
            let mut hook_warnings = Vec::new();
            let matched = run_post_send_hooks_for_cli(&mut hook_warnings, loaded, &event);
            if !hook_warnings.is_empty() {
                warnings.extend(hook_warnings);
            } else if matched {
                emitted = true;
            }
            matched
        });
        if !hook_matched && let Some(target) = built_in_nudge_sink_target(delivery_snapshot) {
            match emit_built_in_nudge(&event, target) {
                Ok(()) => emitted = true,
                Err(error) => warnings.push(post_send_warning(
                    "post-send emission failed",
                    &event,
                    &error,
                )),
            }
        }
        if emitted && let Err(error) = append_notification_log(&notification_event(&event)) {
            warnings.push(WarningEntry::with_code(
                error.code,
                format!(
                    "warning: notification delivery failed for {}@{}: {error}",
                    recipient.agent, recipient.team
                ),
                error.primary_recovery().map(str::to_owned),
            ));
        }
    }
}

pub(crate) fn load_post_send_config_for_sender<R>(
    runtime: &R,
    sender_team: &TeamName,
    sender: &AgentName,
) -> Result<Option<AtmConfig>, AtmError>
where
    R: crate::service_runtime::RetainedServiceRuntime + ?Sized,
{
    let Some(member) = runtime.load_roster_member(sender_team, sender)? else {
        return Ok(None);
    };
    let Some(config_root) = sender_config_root(&member.metadata_json) else {
        return Ok(None);
    };
    runtime.load_config(&config_root)
}

fn run_post_send_hooks_for_cli(
    warnings: &mut Vec<WarningEntry>,
    config: &AtmConfig,
    event: &PostSendHookEvent,
) -> bool {
    // This helper is intentionally synchronous and may block the caller thread
    // for up to POST_SEND_HOOK_TIMEOUT while supervising one child process.
    // Keep it on the CLI path; do not call it from an async runtime thread.
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
        return false;
    }

    for rule in matching_rules {
        execute_post_send_hook(warnings, config, rule, event);
    }
    true
}

fn built_in_nudge_sink_target(
    delivery_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
) -> Option<BuiltInNudgeSinkTarget> {
    if delivery_snapshot.local_tmux_post_send {
        Some(BuiltInNudgeSinkTarget::Tmux)
    } else if delivery_snapshot.graft_post_send {
        Some(BuiltInNudgeSinkTarget::Graft)
    } else {
        None
    }
}

fn emit_built_in_nudge(
    event: &PostSendHookEvent,
    sink_target: BuiltInNudgeSinkTarget,
) -> Result<(), AtmError> {
    let command_path = atm_command_path()?;
    let payload = post_send_hook_payload(event).to_string();
    let mut command = Command::new(&command_path);
    command
        .arg("internal-nudge")
        .env("ATM_POST_SEND", payload)
        .env(INTERNAL_NUDGE_SINK_ENV, sink_target.env_value())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to start built-in post-send nudge command {}: {source}",
            command_path.display()
        ))
        .with_recovery(
            "Ensure the installed `atm` binary is executable and on disk before retrying post-send delivery.",
        )
        .with_source(source)
    })?;
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let error = match sink_target {
                    BuiltInNudgeSinkTarget::Tmux => AtmError::new_with_code(
                        AtmErrorCode::PostSendTmuxSendFailed,
                        crate::error::AtmErrorKind::DaemonUnavailable,
                        format!(
                            "built-in tmux post-send nudge exited unsuccessfully with status {status}"
                        ),
                    )
                    .with_recovery(
                        "Inspect the built-in tmux nudge path and the recipient pane state before retrying post-send delivery.",
                    ),
                    BuiltInNudgeSinkTarget::Graft => AtmError::new_with_code(
                        AtmErrorCode::PostSendGraftUnavailable,
                        crate::error::AtmErrorKind::DaemonUnavailable,
                        format!(
                            "built-in graft post-send nudge exited unsuccessfully with status {status}"
                        ),
                    )
                    .with_recovery(
                        "Ensure the recipient graft receiver is listening and retry once the built-in graft nudge path is healthy.",
                    ),
                };
                return Err(error);
            }
            Ok(None) if started_at.elapsed() < POST_SEND_HOOK_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                terminate_post_send_hook_process(&mut child, &command_path);
                return Err(AtmError::daemon_unavailable(format!(
                    "built-in post-send nudge command timed out after {}s",
                    POST_SEND_HOOK_TIMEOUT.as_secs()
                ))
                .with_recovery(
                    "Fix the built-in `atm internal-nudge` path so it exits promptly after one nudge delivery attempt.",
                ));
            }
            Err(source) => {
                terminate_post_send_hook_process(&mut child, &command_path);
                return Err(AtmError::daemon_unavailable(
                    "failed while waiting for built-in post-send nudge command",
                )
                .with_recovery(
                    "Inspect the built-in `atm internal-nudge` process lifecycle and retry once the local process environment is healthy.",
                )
                .with_source(source));
            }
        }
    }
}

fn atm_command_path() -> Result<PathBuf, AtmError> {
    if let Some(path) = std::env::var_os(ATM_PROGRAM_ENV).filter(|value| !value.is_empty()) {
        return Ok(path.into());
    }

    let current_exe = std::env::current_exe().map_err(|source| {
        AtmError::daemon_unavailable("failed to resolve the running ATM process executable")
            .with_recovery(
                "Run ATM from an installed binary path or repair the current process environment before retrying built-in post-send delivery.",
            )
            .with_source(source)
    })?;
    if is_atm_binary_path(&current_exe) {
        return Ok(current_exe);
    }

    for candidate in atm_binary_candidates(&current_exe) {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(AtmError::daemon_unavailable(format!(
        "failed to resolve the companion `atm` CLI binary for built-in post-send delivery from {}",
        current_exe.display()
    ))
    .with_recovery(
        "Install `atm` alongside the running ATM binary, or set ATM_TEST_ATM_BIN to an explicit `atm` path before retrying built-in post-send delivery.",
    ))
}

fn atm_binary_candidates(current_exe: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(parent) = current_exe.parent() {
        candidates.push(parent.join(atm_binary_leaf()));
        if parent.file_name().is_some_and(|name| name == "deps")
            && let Some(grandparent) = parent.parent()
        {
            candidates.push(grandparent.join(atm_binary_leaf()));
        }
    }
    candidates
}

fn atm_binary_leaf() -> &'static str {
    #[cfg(windows)]
    {
        "atm.exe"
    }
    #[cfg(not(windows))]
    {
        "atm"
    }
}

fn is_atm_binary_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            #[cfg(windows)]
            {
                name.eq_ignore_ascii_case("atm.exe")
            }
            #[cfg(not(windows))]
            {
                name == "atm"
            }
        })
}

fn execute_post_send_hook(
    warnings: &mut Vec<WarningEntry>,
    config: &AtmConfig,
    rule: &config::types::PostSendHookRule,
    event: &PostSendHookEvent,
) {
    // This function performs blocking child-process supervision with short
    // sleeps. It is safe for the current CLI call path and must stay off async
    // runtime threads unless wrapped in spawn_blocking by the caller.
    let Some(execution) = prepare_post_send_hook_execution(config, rule, event) else {
        return;
    };
    let mut child = match spawn_post_send_hook_process(config, &execution, event, rule, warnings) {
        Some(child) => child,
        None => return,
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
                return handle_post_send_hook_timeout(
                    &mut child,
                    stdout_reader.take(),
                    &stdout_cancellation,
                    &execution.command_path,
                    warnings,
                    event,
                    rule,
                );
            }
            Err(error) => {
                return handle_post_send_hook_status_error(
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
            }
        }
    }
}

struct HookExecution {
    command_path: PathBuf,
    argv: Vec<String>,
    payload: Value,
}

fn prepare_post_send_hook_execution(
    config: &AtmConfig,
    rule: &config::types::PostSendHookRule,
    event: &PostSendHookEvent,
) -> Option<HookExecution> {
    let mut argv = rule.command.iter();
    let command_path = resolve_command_path(config, argv.next()?);
    Some(HookExecution {
        command_path,
        argv: argv.cloned().collect(),
        payload: post_send_hook_payload(event),
    })
}

fn post_send_hook_payload(event: &PostSendHookEvent) -> Value {
    let mut payload = json!({
        "from": qualified_sender_identity(&event.sender, Some(&event.sender_team)),
        "to": format!("{}@{}", event.recipient, event.recipient_team),
        "sender": event.sender.as_str(),
        "recipient": event.recipient.as_str(),
        "team": event.recipient_team.as_str(),
        "message_id": event.message_id.to_string(),
        "description": event.description,
        "message": event.description,
        "requires_ack": event.requires_ack,
        "is_ack": event.is_ack,
    });
    if let Some(task_id) = &event.task_id {
        payload["task_id"] = Value::String(task_id.to_string());
    }
    if let Some(recipient_pane_id) = &event.recipient_pane_id {
        payload["recipient_pane_id"] = Value::String(recipient_pane_id.to_string());
    }
    payload
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
    warnings.push(WarningEntry::new(
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
) {
    maybe_log_post_send_hook_result(
        command_path,
        finish_post_send_hook_stdout_capture(stdout_reader, command_path),
    );
    if !status.success() {
        warn_post_send_hook_exit_failure(command_path, status, warnings, event, rule);
    }
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
    warnings.push(WarningEntry::new(
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
    warnings.push(WarningEntry::new(
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
    args.warnings.push(WarningEntry::new(
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

fn notification_event(event: &PostSendHookEvent) -> NotificationEvent {
    NotificationEvent {
        kind: NotificationKind::Delivery,
        detail: serde_json::to_string(&json!({
            "sender": event.sender.as_str(),
            "sender_team": event.sender_team.as_str(),
            "message_id": event.message_id.to_string(),
            "description": event.description,
            "requires_ack": event.requires_ack,
            "is_ack": event.is_ack,
            "task_id": event.task_id.as_ref().map(ToString::to_string),
            "recipient_pane_id": event.recipient_pane_id.as_ref().map(ToString::to_string),
        }))
        .expect("delivery notification detail must serialize to valid JSON"),
        team: Some(event.recipient_team.clone()),
        agent: Some(event.recipient.clone()),
    }
}

fn post_send_event_from_message(
    recipient: &ResolvedRecipient,
    message: &crate::delivery_plan::LogicalMessage,
    recipient_pane_id: Option<&crate::types::PaneId>,
) -> PostSendHookEvent {
    PostSendHookEvent {
        sender: message.envelope.from.clone(),
        sender_team: message
            .envelope
            .source_team
            .clone()
            .unwrap_or_else(|| recipient.team.clone()),
        recipient: recipient.agent.clone(),
        recipient_team: recipient.team.clone(),
        message_id: message.message_id(),
        description: message
            .envelope
            .summary
            .clone()
            .filter(|summary| !summary.trim().is_empty())
            .unwrap_or_else(|| message.envelope.text.clone()),
        requires_ack: message.requires_ack,
        is_ack: message.is_ack,
        task_id: message.envelope.task_id.clone(),
        recipient_pane_id: recipient_pane_id.cloned(),
    }
}

fn sender_config_root(metadata: &serde_json::Map<String, Value>) -> Option<PathBuf> {
    compatible_home_dir(metadata).map(Into::into)
}

fn post_send_warning(prefix: &str, event: &PostSendHookEvent, error: &AtmError) -> WarningEntry {
    WarningEntry::with_code(
        error.code,
        format!(
            "warning: {prefix} for {}@{} message {} ({}): {}.",
            event.recipient, event.recipient_team, event.message_id, error.code, error.message
        ),
        error.primary_recovery().map(str::to_owned),
    )
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

fn abandon_post_send_hook_stdout_capture(
    stdout_reader: Option<thread::JoinHandle<std::io::Result<Vec<u8>>>>,
    cancellation: &HookCancellationToken,
    command_path: &Path,
) {
    // The timeout/error paths must not block on stdout capture. Cancelling the
    // reader lets the helper exit as soon as the killed child closes stdout.
    // We still give the reader a short bounded join window so it does not
    // outlive the command path under normal teardown.
    cancellation.cancel();
    finish_abandoned_post_send_hook_stdout_capture(stdout_reader, command_path);
}

fn finish_abandoned_post_send_hook_stdout_capture(
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
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use serde_json::{Map, json};
    use tempfile::tempdir;
    use tracing::Level;

    use super::{
        ATM_PROGRAM_ENV, BuiltInNudgeSinkTarget, HookCancellationToken,
        POST_SEND_HOOK_MAX_STDOUT_BYTES, PostSendHookResultLevel, emit_built_in_nudge,
        emit_post_send_effects, finish_abandoned_post_send_hook_stdout_capture,
        hook_matches_recipient, hook_result_log_level, load_post_send_config_for_sender,
        parse_post_send_hook_result, sender_config_root,
    };
    use crate::boundary::{PostSendHookEvent, RosterEntry, RosterHarness, RosterMemberKind};
    use crate::config::AtmConfig;
    use crate::config::types::{HookRecipient, PostSendHookRule};
    use crate::delivery_plan::LogicalMessage;
    use crate::delivery_policy::{DeliveryHarnessPath, DeliveryRecipientSnapshot};
    use crate::error::AtmError;
    use crate::roles::ROLE_TEAM_LEAD;
    #[allow(
        deprecated,
        reason = "Phase AD obsolete: derived compatibility field only. Hook tests intentionally exercise the retained legacy cwd compatibility seam."
    )]
    use crate::schema::agent_member::LEGACY_CWD_METADATA_KEY;
    use crate::schema::{AtmMessageId, HOME_DIR_METADATA_KEY, InboxMessage, TeamConfig};
    use crate::send::ResolvedRecipient;
    use crate::service_runtime::{RetainedMailboxTimeoutPolicy, RetainedServiceRuntime};
    use crate::test_support::{EnvGuard, TEST_SENDER};
    use crate::types::{AgentName, IsoTimestamp, PaneId, TeamName};
    use crate::workflow::WorkflowStateFile;

    struct ConfigLookupRuntime {
        roster_entry: Option<RosterEntry>,
        config_lookup_root: PathBuf,
        config: Option<AtmConfig>,
    }

    impl crate::boundary::sealed::Sealed for ConfigLookupRuntime {}

    impl RetainedServiceRuntime for ConfigLookupRuntime {
        fn load_config(&self, current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
            Ok((current_dir == self.config_lookup_root)
                .then_some(self.config.clone())
                .flatten())
        }

        fn load_team_config_for_doctor_compare(
            &self,
            _team_dir: &Path,
        ) -> Result<TeamConfig, AtmError> {
            unreachable!("config lookup test does not read team config")
        }

        fn team_dir(&self, _home_dir: &Path, _team: &TeamName) -> Result<PathBuf, AtmError> {
            unreachable!("config lookup test does not resolve team dirs")
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            unreachable!("config lookup test does not resolve inbox paths")
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy {
            RetainedMailboxTimeoutPolicy {
                workflow_lock_timeout: Duration::from_millis(1),
            }
        }

        fn rebuild_compat_inbox_projection(
            &self,
            _inbox_path: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<(), AtmError> {
            unreachable!("config lookup test does not rebuild projections")
        }

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _messages: &[InboxMessage],
        ) -> Result<(), AtmError> {
            unreachable!("config lookup test does not deliver outbound payloads")
        }

        fn load_roster_member(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<RosterEntry>, AtmError> {
            Ok(self.roster_entry.clone())
        }

        fn load_team_roster(&self, _team: &TeamName) -> Result<Vec<RosterEntry>, AtmError> {
            Ok(Vec::new())
        }

        fn commit_workflow_state<T, I, F>(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _extra_write_paths: I,
            _timeout: Duration,
            _body: F,
        ) -> Result<T, AtmError>
        where
            I: IntoIterator<Item = PathBuf>,
            F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>,
        {
            unreachable!("config lookup test does not commit workflow state")
        }
    }

    #[test]
    fn hook_matches_recipient_exact_and_wildcard_values() {
        assert!(hook_matches_recipient(
            &HookRecipient::Named(TEST_SENDER.parse().expect("recipient")),
            &TEST_SENDER.parse().expect("candidate")
        ));
        assert!(hook_matches_recipient(
            &HookRecipient::Wildcard,
            &TEST_SENDER.parse().expect("candidate")
        ));
        assert!(!hook_matches_recipient(
            &HookRecipient::Named(ROLE_TEAM_LEAD.parse().expect("recipient")),
            &TEST_SENDER.parse().expect("candidate")
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

    #[test]
    fn hook_cancellation_token_tracks_cancelled_state() {
        let token = HookCancellationToken::default();
        assert!(!token.is_cancelled());

        token.cancel();

        assert!(token.is_cancelled());
    }

    #[test]
    fn bounded_stdout_teardown_returns_promptly_for_completed_reader() {
        let handle = std::thread::spawn(|| Ok::<Vec<u8>, std::io::Error>(Vec::new()));
        finish_abandoned_post_send_hook_stdout_capture(Some(handle), Path::new("hook"));
    }

    #[test]
    #[allow(
        deprecated,
        reason = "Phase AD obsolete: test fixture intentionally exercises the retained legacy cwd compatibility fallback."
    )]
    fn sender_config_root_prefers_home_dir_and_falls_back_to_cwd() {
        let home_dir_metadata =
            Map::from_iter([(HOME_DIR_METADATA_KEY.to_string(), json!("/repo/home"))]);
        assert_eq!(
            sender_config_root(&home_dir_metadata),
            Some(PathBuf::from("/repo/home"))
        );

        let cwd_only_metadata =
            Map::from_iter([(LEGACY_CWD_METADATA_KEY.to_string(), json!("/repo/cwd"))]);
        assert_eq!(
            sender_config_root(&cwd_only_metadata),
            Some(PathBuf::from("/repo/cwd"))
        );
    }

    #[test]
    #[allow(
        deprecated,
        reason = "Phase AD obsolete: test fixture intentionally seeds legacy cwd metadata to verify the bounded compatibility read."
    )]
    fn load_post_send_config_uses_sender_roster_metadata_not_caller_cwd() {
        let config_root = PathBuf::from("/repo/home");
        let runtime = ConfigLookupRuntime {
            roster_entry: Some(RosterEntry {
                team_name: TeamName::from_validated("test-team"),
                agent_name: AgentName::from_validated(TEST_SENDER),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: crate::schema::AgentType::default(),
                model: crate::types::ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::from_iter([(
                    LEGACY_CWD_METADATA_KEY.to_string(),
                    json!(config_root.display().to_string()),
                )]),
            }),
            config_lookup_root: config_root.clone(),
            config: Some(AtmConfig {
                config_root: config_root.clone(),
                ..Default::default()
            }),
        };

        let loaded = load_post_send_config_for_sender(
            &runtime,
            &TeamName::from_validated("test-team"),
            &AgentName::from_validated(TEST_SENDER),
        )
        .expect("config lookup");

        assert_eq!(
            loaded.as_ref().map(|config| &config.config_root),
            Some(&config_root)
        );
    }

    fn tmux_event(recipient_pane_id: Option<PaneId>) -> PostSendHookEvent {
        PostSendHookEvent {
            sender: AgentName::from_validated(TEST_SENDER),
            sender_team: TeamName::from_validated("test-team"),
            recipient: AgentName::from_validated("recipient"),
            recipient_team: TeamName::from_validated("test-team"),
            message_id: crate::schema::AtmMessageId::new(),
            description: "hello".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id,
        }
    }

    fn logical_message(text: &str) -> LogicalMessage {
        LogicalMessage::new(
            InboxMessage {
                from: AgentName::from_validated(TEST_SENDER),
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TeamName::from_validated("test-team")),
                summary: Some(text.to_string()),
                message_id: Some(AtmMessageId::new()),
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            },
            false,
            false,
        )
        .expect("logical message")
    }

    #[test]
    #[serial_test::serial(env)]
    fn built_in_nudge_fallback_invokes_hidden_cli_with_sink_and_payload() {
        let tempdir = tempdir().expect("tempdir");
        let capture_path = tempdir.path().join("capture.txt");
        #[cfg(windows)]
        let atm_path = tempdir.path().join("atm.cmd");
        #[cfg(not(windows))]
        let atm_path = tempdir.path().join("atm");
        #[cfg(windows)]
        fs::write(
            &atm_path,
            "@echo off\r\nsetlocal EnableDelayedExpansion\r\n> \"%ATM_TEST_CAPTURE%\" echo %1^|!ATM_INTERNAL_NUDGE_SINK!^|!ATM_POST_SEND!\r\nexit /b 0\r\n",
        )
        .expect("write atm shim");
        #[cfg(not(windows))]
        fs::write(
            &atm_path,
            "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$1\" \"$ATM_INTERNAL_NUDGE_SINK\" \"$ATM_POST_SEND\" > \"$ATM_TEST_CAPTURE\"\nexit 0\n",
        )
        .expect("write atm shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&atm_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&atm_path, perms).expect("chmod");
        }

        let capture_value = capture_path.display().to_string();
        let atm_bin = atm_path.display().to_string();
        let _env = EnvGuard::set_many([
            (ATM_PROGRAM_ENV, Some(atm_bin.as_str())),
            ("ATM_TEST_CAPTURE", Some(capture_value.as_str())),
        ]);

        emit_built_in_nudge(
            &tmux_event(Some(PaneId::from_cli("%9").expect("pane"))),
            BuiltInNudgeSinkTarget::Tmux,
        )
        .expect("fallback emit");

        let captured = fs::read_to_string(&capture_path).expect("capture");
        assert!(captured.contains("internal-nudge"));
        assert!(captured.contains("|tmux|"));
        assert!(captured.contains("\"description\":\"hello\""));
    }

    #[test]
    #[serial_test::serial(env)]
    fn built_in_nudge_fallback_surfaces_nonzero_exit() {
        let tempdir = tempdir().expect("tempdir");
        #[cfg(windows)]
        let atm_path = tempdir.path().join("atm.cmd");
        #[cfg(not(windows))]
        let atm_path = tempdir.path().join("atm");
        #[cfg(windows)]
        fs::write(&atm_path, "@echo off\r\nexit /b 7\r\n").expect("write atm shim");
        #[cfg(not(windows))]
        fs::write(&atm_path, "#!/bin/sh\nexit 7\n").expect("write atm shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&atm_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&atm_path, perms).expect("chmod");
        }

        let atm_bin = atm_path.display().to_string();
        let _env = EnvGuard::set_many([(ATM_PROGRAM_ENV, Some(atm_bin.as_str()))]);

        let error = emit_built_in_nudge(
            &tmux_event(Some(PaneId::from_cli("%9").expect("pane"))),
            BuiltInNudgeSinkTarget::Graft,
        )
        .expect_err("nonzero exit must fail");

        assert!(error.message.contains("exited unsuccessfully"));
    }

    #[test]
    #[serial_test::serial(env)]
    fn external_post_send_hook_takes_precedence_over_built_in_nudge() {
        let tempdir = tempdir().expect("tempdir");
        let hook_capture = tempdir.path().join("hook-capture.txt");
        let built_in_capture = tempdir.path().join("built-in-capture.txt");
        #[cfg(windows)]
        let hook_path = tempdir.path().join("hook.cmd");
        #[cfg(not(windows))]
        let hook_path = tempdir.path().join("hook");
        #[cfg(windows)]
        let atm_path = tempdir.path().join("atm.cmd");
        #[cfg(not(windows))]
        let atm_path = tempdir.path().join("atm");
        #[cfg(windows)]
        fs::write(
            &hook_path,
            "@echo off\r\nsetlocal EnableDelayedExpansion\r\n> \"%ATM_TEST_HOOK_CAPTURE%\" echo !ATM_POST_SEND!\r\nexit /b 0\r\n",
        )
        .expect("write hook shim");
        #[cfg(not(windows))]
        fs::write(
            &hook_path,
            "#!/bin/sh\nprintf '%s\\n' \"$ATM_POST_SEND\" > \"$ATM_TEST_HOOK_CAPTURE\"\nexit 0\n",
        )
        .expect("write hook shim");
        #[cfg(windows)]
        fs::write(
            &atm_path,
            "@echo off\r\n> \"%ATM_TEST_BUILT_IN_CAPTURE%\" echo invoked\r\nexit /b 0\r\n",
        )
        .expect("write atm shim");
        #[cfg(not(windows))]
        fs::write(
            &atm_path,
            "#!/bin/sh\nprintf 'invoked\\n' > \"$ATM_TEST_BUILT_IN_CAPTURE\"\nexit 0\n",
        )
        .expect("write atm shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&hook_path, &atm_path] {
                let mut perms = fs::metadata(path).expect("metadata").permissions();
                perms.set_mode(0o755);
                fs::set_permissions(path, perms).expect("chmod");
            }
        }

        let hook_capture_value = hook_capture.display().to_string();
        let built_in_capture_value = built_in_capture.display().to_string();
        let atm_bin = atm_path.display().to_string();
        let _env = EnvGuard::set_many([
            ("ATM_TEST_HOOK_CAPTURE", Some(hook_capture_value.as_str())),
            (
                "ATM_TEST_BUILT_IN_CAPTURE",
                Some(built_in_capture_value.as_str()),
            ),
            ("ATM_HOME", tempdir.path().to_str()),
            ("ATM_CONFIG_HOME", tempdir.path().to_str()),
            ("HOME", tempdir.path().to_str()),
            (ATM_PROGRAM_ENV, Some(atm_bin.as_str())),
        ]);

        let config = AtmConfig {
            config_root: tempdir.path().to_path_buf(),
            post_send_hooks: vec![PostSendHookRule {
                recipient: HookRecipient::Named("recipient".parse().expect("recipient")),
                command: vec![hook_path.display().to_string()],
            }],
            ..Default::default()
        };
        let recipient = ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated("test-team"),
        };
        let snapshot = DeliveryRecipientSnapshot {
            agent: recipient.agent.clone(),
            team: recipient.team.clone(),
            harness: DeliveryHarnessPath::ClaudeCode,
            recipient_pane_id: Some(PaneId::from_cli("%9").expect("pane")),
            local_tmux_post_send: true,
            graft_post_send: false,
            roster_backed: true,
        };
        let mut warnings = Vec::new();

        emit_post_send_effects(
            &mut warnings,
            Some(&config),
            None,
            &recipient,
            &snapshot,
            &[logical_message("hello")],
        );

        let captured = fs::read_to_string(&hook_capture).expect("hook capture");
        assert!(captured.contains("\"description\":\"hello\""));
        assert!(!built_in_capture.exists());
    }
}
