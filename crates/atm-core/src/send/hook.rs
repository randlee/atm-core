use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
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

use crate::boundary::{GraftPostSendPort, PostSendHookEmitter, PostSendHookEvent};
use crate::config::types::HookRecipient;
use crate::config::{self, AtmConfig};
use crate::error::{AtmError, AtmErrorKind};
use crate::error_codes::AtmErrorCode;
use crate::protocol::{NotificationEvent, NotificationKind};
use crate::service_runtime::append_notification_log;
use crate::types::{AgentName, PaneId, TeamName};

use super::{ResolvedRecipient, WarningEntry, qualified_sender_identity};

const POST_SEND_HOOK_TIMEOUT: Duration = Duration::from_secs(5);
const POST_SEND_HOOK_MAX_STDOUT_BYTES: usize = 8 * 1024;
const POST_SEND_HOOK_STDOUT_JOIN_TIMEOUT: Duration = Duration::from_millis(500);

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

pub(crate) struct ConfiguredPostSendHookEmitter<'a> {
    config: &'a AtmConfig,
}

impl<'a> ConfiguredPostSendHookEmitter<'a> {
    pub(crate) fn new(config: &'a AtmConfig) -> Self {
        Self { config }
    }
}

impl crate::boundary::sealed::Sealed for ConfiguredPostSendHookEmitter<'_> {}

impl PostSendHookEmitter for ConfiguredPostSendHookEmitter<'_> {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError> {
        let mut warnings = Vec::new();
        run_post_send_hooks_for_cli(&mut warnings, self.config, event);
        warnings_to_result(&warnings)
    }
}

pub(crate) struct LocalTmuxPostSendEmitter;

impl crate::boundary::sealed::Sealed for LocalTmuxPostSendEmitter {}

impl PostSendHookEmitter for LocalTmuxPostSendEmitter {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError> {
        let Some(pane_id) = event.recipient_pane_id.as_ref() else {
            let error = AtmError::new_with_code(
                AtmErrorCode::PostSendPaneMissing,
                AtmErrorKind::Validation,
                format!(
                    "recipient {}@{} has tmux-backed post-send capability but no pane id",
                    event.recipient, event.recipient_team
                ),
            )
            .with_recovery(format!(
                "Repair the roster row with `atm teams update-member --team {} --member {} --tmux-pane-id <pane>`.",
                event.recipient_team, event.recipient
            ));
            warn!(
                code = %AtmErrorCode::PostSendPaneMissing,
                sender = %event.sender,
                recipient = %event.recipient,
                recipient_team = %event.recipient_team,
                message_id = %event.message_id,
                "local tmux post-send emission is missing authoritative pane metadata"
            );
            return Err(error);
        };

        let message = tmux_nudge_message(&event.recipient_team);
        run_tmux_send_keys(pane_id, &message, event)?;
        run_tmux_send_enter(pane_id, event)?;
        Ok(())
    }
}

pub(crate) struct GraftPostSendEmitter<'a> {
    port: &'a dyn GraftPostSendPort,
}

impl<'a> GraftPostSendEmitter<'a> {
    pub(crate) fn new(port: &'a dyn GraftPostSendPort) -> Self {
        Self { port }
    }
}

impl crate::boundary::sealed::Sealed for GraftPostSendEmitter<'_> {}

impl PostSendHookEmitter for GraftPostSendEmitter<'_> {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError> {
        self.port.deliver_post_send(event)
    }
}

pub(crate) fn emit_post_send_effects(
    warnings: &mut Vec<WarningEntry>,
    config: Option<&AtmConfig>,
    graft_port: Option<&dyn GraftPostSendPort>,
    recipient: &ResolvedRecipient,
    delivery_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
    messages: &[crate::delivery_plan::LogicalMessage],
) {
    let hook_emitter = config.map(ConfiguredPostSendHookEmitter::new);
    let graft_emitter = graft_port.map(GraftPostSendEmitter::new);
    let tmux_emitter = delivery_snapshot
        .local_tmux_post_send
        .then_some(LocalTmuxPostSendEmitter);
    for message in messages {
        let event = post_send_event_from_message(
            recipient,
            message,
            delivery_snapshot.recipient_pane_id.as_ref(),
        );
        if let Err(error) = append_notification_log(&notification_event(&event)) {
            warnings.push(WarningEntry::new(
                format!(
                    "warning: notification delivery failed for {}@{}: {error}",
                    recipient.agent, recipient.team
                ),
                error.primary_recovery().map(str::to_owned),
            ));
        }
        if let Some(emitter) = tmux_emitter.as_ref()
            && let Err(error) = emitter.emit(&event)
        {
            warnings.push(post_send_warning(
                "post-send emission failed",
                &event,
                &error,
            ));
        }
        if delivery_snapshot.graft_post_send
            && let Some(emitter) = graft_emitter.as_ref()
            && let Err(error) = emitter.emit(&event)
        {
            warnings.push(post_send_warning(
                "post-send emission failed",
                &event,
                &error,
            ));
        }
        if let Some(emitter) = hook_emitter.as_ref()
            && let Err(error) = emitter.emit(&event)
        {
            warnings.push(post_send_warning("post-send hook failed", &event, &error));
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
) {
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
        return;
    }

    for rule in matching_rules {
        execute_post_send_hook(warnings, config, rule, event);
    }
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

fn tmux_nudge_message(team: &TeamName) -> String {
    format!("You have unread ATM messages. Run: atm read --team {team}")
}

fn run_tmux_send_keys(
    pane_id: &PaneId,
    message: &str,
    event: &PostSendHookEvent,
) -> Result<(), AtmError> {
    let output = run_tmux_command(
        {
            let mut command = Command::new("tmux");
            command.args(["send-keys", "-t", pane_id.as_str(), "-l", message]);
            command
        },
        pane_id,
        event,
        "send-keys",
    )?;
    ensure_tmux_success(output, pane_id, event, "send literal nudge")
}

fn run_tmux_send_enter(pane_id: &PaneId, event: &PostSendHookEvent) -> Result<(), AtmError> {
    let output = run_tmux_command(
        {
            let mut command = Command::new("tmux");
            command.args(["send-keys", "-t", pane_id.as_str(), "Enter"]);
            command
        },
        pane_id,
        event,
        "send-keys Enter",
    )?;
    ensure_tmux_success(output, pane_id, event, "send Enter to nudge pane")
}

fn run_tmux_command(
    mut command: Command,
    pane_id: &PaneId,
    event: &PostSendHookEvent,
    tmux_action: &str,
) -> Result<Output, AtmError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command.spawn().map_err(|error| {
        tmux_send_failed_error(
            pane_id,
            event,
            format!("failed to start tmux {tmux_action}: {error}"),
            Some(error),
        )
    })?;
    wait_for_tmux_output(child, pane_id, event, tmux_action)
}

fn wait_for_tmux_output(
    mut child: Child,
    pane_id: &PaneId,
    event: &PostSendHookEvent,
    tmux_action: &str,
) -> Result<Output, AtmError> {
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().map_err(|error| {
                    tmux_send_failed_error(
                        pane_id,
                        event,
                        format!("failed to collect tmux {tmux_action} output: {error}"),
                        Some(error),
                    )
                });
            }
            Ok(None) if started_at.elapsed() < POST_SEND_HOOK_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                terminate_tmux_child(&mut child, pane_id, event, tmux_action);
                let _ = child.wait_with_output();
                return Err(tmux_send_failed_error(
                    pane_id,
                    event,
                    format!(
                        "tmux {tmux_action} timed out after {}s",
                        POST_SEND_HOOK_TIMEOUT.as_secs()
                    ),
                    None::<std::io::Error>,
                ));
            }
            Err(error) => {
                terminate_tmux_child(&mut child, pane_id, event, tmux_action);
                let _ = child.wait_with_output();
                return Err(tmux_send_failed_error(
                    pane_id,
                    event,
                    format!("failed while waiting for tmux {tmux_action}: {error}"),
                    Some(error),
                ));
            }
        }
    }
}

fn terminate_tmux_child(
    child: &mut Child,
    pane_id: &PaneId,
    event: &PostSendHookEvent,
    tmux_action: &str,
) {
    if let Err(error) = child.kill()
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        warn!(
            code = %AtmErrorCode::PostSendTmuxSendFailed,
            sender = %event.sender,
            recipient = %event.recipient,
            recipient_team = %event.recipient_team,
            message_id = %event.message_id,
            pane_id = %pane_id,
            tmux_action,
            %error,
            "failed to terminate timed-out tmux subprocess"
        );
    }
}

fn ensure_tmux_success(
    output: Output,
    pane_id: &PaneId,
    event: &PostSendHookEvent,
    action: &str,
) -> Result<(), AtmError> {
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if stderr.is_empty() {
        format!("tmux exited unsuccessfully while trying to {action}")
    } else {
        format!("tmux exited unsuccessfully while trying to {action}: {stderr}")
    };
    Err(tmux_send_failed_error(
        pane_id,
        event,
        detail,
        None::<std::io::Error>,
    ))
}

fn tmux_send_failed_error<E>(
    pane_id: &PaneId,
    event: &PostSendHookEvent,
    message: String,
    source: Option<E>,
) -> AtmError
where
    E: std::error::Error + Send + Sync + 'static,
{
    let error = AtmError::new_with_code(
        AtmErrorCode::PostSendTmuxSendFailed,
        AtmErrorKind::Internal,
        format!(
            "local tmux post-send emission failed for {}@{} pane {}: {message}",
            event.recipient, event.recipient_team, pane_id
        ),
    )
    .with_recovery(format!(
        "Verify tmux pane {} still exists and repair stale pane metadata with `atm teams update-member --team {} --member {} --tmux-pane-id <pane>`.",
        pane_id, event.recipient_team, event.recipient
    ));
    warn!(
        code = %AtmErrorCode::PostSendTmuxSendFailed,
        sender = %event.sender,
        recipient = %event.recipient,
        recipient_team = %event.recipient_team,
        pane_id = %pane_id,
        message_id = %event.message_id,
        error = %message,
        "local tmux post-send emission failed"
    );
    match source {
        Some(source) => error.with_source(source),
        None => error,
    }
}

fn notification_event(event: &PostSendHookEvent) -> NotificationEvent {
    NotificationEvent {
        kind: NotificationKind::Delivery,
        detail: serde_json::to_string(&json!({
            "sender": event.sender.as_str(),
            "sender_team": event.sender_team.as_str(),
            "message_id": event.message_id.to_string(),
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
        message: message
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
    metadata
        .get("home_dir")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            metadata
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from)
        })
}

fn warnings_to_result(warnings: &[WarningEntry]) -> Result<(), AtmError> {
    if warnings.is_empty() {
        return Ok(());
    }

    let message = warnings
        .iter()
        .map(|warning| warning.message.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut error = AtmError::new_with_code(
        AtmErrorCode::WarningHookExecutionFailed,
        AtmErrorKind::Internal,
        message,
    );
    for recovery in warnings
        .iter()
        .filter_map(|warning| warning.recovery.as_deref())
    {
        error = error.with_recovery(recovery.to_string());
    }
    Err(error)
}

fn post_send_warning(prefix: &str, event: &PostSendHookEvent, error: &AtmError) -> WarningEntry {
    WarningEntry::new(
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
    use std::time::{Duration, Instant};

    use serde_json::{Map, json};
    use tempfile::tempdir;
    use tracing::Level;

    use super::{
        GraftPostSendEmitter, HookCancellationToken, LocalTmuxPostSendEmitter,
        POST_SEND_HOOK_MAX_STDOUT_BYTES, PostSendHookResultLevel,
        finish_abandoned_post_send_hook_stdout_capture, hook_matches_recipient,
        hook_result_log_level, load_post_send_config_for_sender, parse_post_send_hook_result,
        sender_config_root, tmux_nudge_message,
    };
    use crate::boundary::{
        GraftPostSendPort, PostSendHookEmitter, PostSendHookEvent, ProjectionAppendMode,
        RosterEntry, RosterHarness, RosterMemberKind,
    };
    use crate::config::AtmConfig;
    use crate::config::types::HookRecipient;
    use crate::error::AtmError;
    use crate::error_codes::AtmErrorCode;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{InboxMessage, TeamConfig};
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

        fn append_compat_inbox_message(
            &self,
            _inbox_path: &Path,
            _message: &InboxMessage,
        ) -> Result<(), AtmError> {
            unreachable!("config lookup test does not append messages")
        }

        fn append_compat_inbox_message_set(
            &self,
            _inbox_path: &Path,
            _mode: ProjectionAppendMode,
            _messages: &[InboxMessage],
        ) -> Result<(), AtmError> {
            unreachable!("config lookup test does not append message sets")
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

    #[derive(Default)]
    struct RecordingGraftPort {
        delivered: std::sync::Mutex<Vec<PostSendHookEvent>>,
        fail_message: Option<String>,
    }

    impl crate::boundary::sealed::Sealed for RecordingGraftPort {}

    impl GraftPostSendPort for RecordingGraftPort {
        fn deliver_post_send(&self, event: &PostSendHookEvent) -> Result<(), AtmError> {
            if let Some(message) = &self.fail_message {
                return Err(AtmError::daemon_unavailable(message.clone()));
            }
            self.delivered
                .lock()
                .expect("graft deliveries")
                .push(event.clone());
            Ok(())
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
    fn sender_config_root_prefers_home_dir_and_falls_back_to_cwd() {
        let home_dir_metadata = Map::from_iter([("home_dir".to_string(), json!("/repo/home"))]);
        assert_eq!(
            sender_config_root(&home_dir_metadata),
            Some(PathBuf::from("/repo/home"))
        );

        let cwd_only_metadata = Map::from_iter([("cwd".to_string(), json!("/repo/cwd"))]);
        assert_eq!(
            sender_config_root(&cwd_only_metadata),
            Some(PathBuf::from("/repo/cwd"))
        );
    }

    #[test]
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
                    "cwd".to_string(),
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
            message: "hello".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id,
        }
    }

    #[test]
    fn local_tmux_post_send_emitter_requires_authoritative_pane_id() {
        let emitter = LocalTmuxPostSendEmitter;
        let error = emitter
            .emit(&tmux_event(None))
            .expect_err("missing pane must fail");

        assert_eq!(error.code, AtmErrorCode::PostSendPaneMissing);
        assert!(error.message.contains("no pane id"));
    }

    #[test]
    #[serial_test::serial(env)]
    fn local_tmux_post_send_emitter_uses_authoritative_pane_id() {
        let tempdir = tempdir().expect("tempdir");
        let tmux_log = tempdir.path().join("tmux.log");
        #[cfg(windows)]
        let tmux_path = tempdir.path().join("tmux.cmd");
        #[cfg(not(windows))]
        let tmux_path = tempdir.path().join("tmux");
        #[cfg(windows)]
        fs::write(
            &tmux_path,
            "@echo off\r\n>> \"%ATM_TEST_TMUX_LOG%\" echo %*\r\nexit /b 0\r\n",
        )
        .expect("write tmux shim");
        #[cfg(not(windows))]
        fs::write(
            &tmux_path,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$ATM_TEST_TMUX_LOG\"\nexit 0\n",
        )
        .expect("write tmux shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&tmux_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&tmux_path, perms).expect("chmod");
        }

        let mut path_entries = vec![tempdir.path().to_path_buf()];
        if let Some(path) = std::env::var_os("PATH") {
            path_entries.extend(std::env::split_paths(&path));
        }
        let shim_path = std::env::join_paths(path_entries).expect("join shim path");
        let shim_path = shim_path.to_str().expect("utf8 shim path").to_owned();
        let tmux_log = tmux_log.to_str().expect("utf8 tmux log").to_owned();
        let _env = EnvGuard::set_many([
            ("PATH", Some(shim_path.as_str())),
            ("ATM_TEST_TMUX_LOG", Some(tmux_log.as_str())),
        ]);

        let result =
            LocalTmuxPostSendEmitter.emit(&tmux_event(Some(PaneId::from_cli("%9").expect("pane"))));

        result.expect("tmux send");
        let logged = fs::read_to_string(&tmux_log).expect("tmux log");
        assert!(logged.contains("send-keys"));
        assert!(logged.contains("%9"));
        assert!(logged.contains(&tmux_nudge_message(&TeamName::from_validated("test-team"))));
        assert!(logged.contains("Enter"));
    }

    #[test]
    #[serial_test::serial(env)]
    fn local_tmux_post_send_emitter_times_out_hung_tmux() {
        let tempdir = tempdir().expect("tempdir");
        #[cfg(windows)]
        let tmux_path = tempdir.path().join("tmux.cmd");
        #[cfg(not(windows))]
        let tmux_path = tempdir.path().join("tmux");
        #[cfg(windows)]
        fs::write(
            &tmux_path,
            "@echo off\r\nsetlocal EnableDelayedExpansion\r\nfor /f \"tokens=1-4 delims=:.\" %%a in (\"%time%\") do (\r\n  set /a start=%%a*3600+%%b*60+%%c\r\n)\r\n:loop\r\nfor /f \"tokens=1-4 delims=:.\" %%a in (\"%time%\") do (\r\n  set /a now=%%a*3600+%%b*60+%%c\r\n)\r\nif !now! lss !start! set /a now+=86400\r\nset /a elapsed=now-start\r\nif !elapsed! lss 30 goto loop\r\nexit /b 0\r\n",
        )
        .expect("write tmux shim");
        #[cfg(not(windows))]
        fs::write(
            &tmux_path,
            "#!/usr/bin/env python3\nimport time\ntime.sleep(30)\n",
        )
        .expect("write tmux shim");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&tmux_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&tmux_path, perms).expect("chmod");
        }

        let mut path_entries = vec![tempdir.path().to_path_buf()];
        if let Some(path) = std::env::var_os("PATH") {
            path_entries.extend(std::env::split_paths(&path));
        }
        let shim_path = std::env::join_paths(path_entries).expect("join shim path");
        let shim_path = shim_path.to_str().expect("utf8 shim path").to_owned();
        let _env = EnvGuard::set_many([("PATH", Some(shim_path.as_str()))]);

        let started_at = Instant::now();
        let error = LocalTmuxPostSendEmitter
            .emit(&tmux_event(Some(PaneId::from_cli("%9").expect("pane"))))
            .expect_err("hung tmux must time out");

        assert!(started_at.elapsed() < Duration::from_secs(8));
        assert_eq!(error.code, AtmErrorCode::PostSendTmuxSendFailed);
        assert!(error.message.contains("timed out"));
    }

    #[test]
    fn graft_post_send_emitter_delegates_to_graft_port() {
        let port = RecordingGraftPort::default();
        let event = tmux_event(Some(PaneId::from_cli("%9").expect("pane")));

        GraftPostSendEmitter::new(&port)
            .emit(&event)
            .expect("graft send");

        let delivered = port.delivered.lock().expect("graft deliveries");
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0], event);
    }

    #[test]
    fn graft_post_send_emitter_surfaces_port_failure() {
        let port = RecordingGraftPort {
            delivered: std::sync::Mutex::new(Vec::new()),
            fail_message: Some("synthetic graft failure".to_string()),
        };

        let error = GraftPostSendEmitter::new(&port)
            .emit(&tmux_event(Some(PaneId::from_cli("%9").expect("pane"))))
            .expect_err("graft failure");

        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        assert!(error.message.contains("synthetic graft failure"));
    }
}
