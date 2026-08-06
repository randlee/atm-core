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

use super::{
    POST_SEND_HOOK_TIMEOUT, ResolvedRecipient, WarningEntry, nudge_template,
    qualified_nudge_sender_identity,
};
use crate::boundary::{
    BuiltInPostSendDispatch, GraftNudgeTarget, HookExecutionSummary, LocalTmuxNudgeTarget,
    MessageReceivedHookEmitter, PostSendBuiltInTarget, PostSendEmissionOutcome,
    PostSendEmissionPath, PostSendHookEvent, built_in_nudge_template_kind_from_post_send_event,
};
use crate::config::types::HookRecipient;
use crate::config::{self, AtmConfig};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::protocol::{NotificationEvent, NotificationKind};
use crate::schema::authenticated_source_host;
#[cfg(test)]
use crate::schema::compatible_home_dir;
use crate::service_runtime::{RetainedServiceRuntime, append_notification_log};
#[cfg(test)]
use crate::types::{AgentName, TeamName};

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

pub(crate) fn emit_post_send_effects<R>(
    runtime: &R,
    warnings: &mut Vec<WarningEntry>,
    config: Option<&AtmConfig>,
    post_send_emitter: Option<&dyn MessageReceivedHookEmitter>,
    recipient: &ResolvedRecipient,
    delivery_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
    messages: &[crate::delivery_plan::LogicalMessage],
) where
    R: RetainedServiceRuntime + ?Sized,
{
    for message in messages {
        let event = post_send_event_from_message(
            recipient,
            message,
            delivery_snapshot.recipient_pane_id.as_ref(),
        );
        let outcome = emit_post_send_outcome(
            runtime,
            warnings,
            config,
            post_send_emitter,
            delivery_snapshot,
            &event,
        );
        if matches!(outcome, PostSendEmissionOutcome::Delivered { .. })
            && let Err(error) = append_notification_log(&notification_event(&event))
        {
            warnings.push(WarningEntry::with_code(
                error.code(),
                format!(
                    "warning: notification delivery failed for {}@{}: {error}",
                    recipient.agent, recipient.team
                ),
                Some(error.message().to_owned()),
            ));
        }
    }
}

fn emit_post_send_outcome<R>(
    runtime: &R,
    warnings: &mut Vec<WarningEntry>,
    config: Option<&AtmConfig>,
    post_send_emitter: Option<&dyn MessageReceivedHookEmitter>,
    delivery_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
    event: &PostSendHookEvent,
) -> PostSendEmissionOutcome
where
    R: RetainedServiceRuntime + ?Sized,
{
    let hook_summary = config
        .map(|loaded| run_post_send_hooks_for_cli(warnings, loaded, event))
        .unwrap_or_else(|| HookExecutionSummary::new(0, 0, 0));
    if hook_summary.succeeded_rules() > 0 {
        return PostSendEmissionOutcome::Delivered {
            path: PostSendEmissionPath::ExternalHook,
            hook_summary,
        };
    }
    if hook_summary.matched_rules() > 0 {
        let warning = warnings.last().cloned().unwrap_or_else(|| {
            WarningEntry::with_code(
                AtmErrorCode::WarningHookExecutionFailed,
                format!(
                    "warning: post-send hook execution failed for {}@{} message {}.",
                    event.recipient, event.recipient_team, event.message_id
                ),
                Some(
                    "Inspect the matching post-send hook command output and retry once the hook exits successfully."
                        .to_string(),
                ),
            )
        });
        return PostSendEmissionOutcome::Failed {
            hook_summary,
            warning,
        };
    }
    let Some(dispatch) = build_built_in_dispatch(runtime, delivery_snapshot, event) else {
        return PostSendEmissionOutcome::NoCapability { hook_summary };
    };
    // A supplied emitter is the receiver implementation selected by the
    // embedding harness. This is also the seam used by the core tests; it
    // must handle either target rather than making the core tests depend on a
    // running receiver endpoint.
    //
    // Production Graft has no daemon-side emitter. When no harness emitter
    // was selected, core performs the one serialized delivery to Graft's
    // independently published endpoint, where `GraftReceiveHook` executes.
    let emitted = if let Some(post_send_emitter) = post_send_emitter {
        post_send_emitter.emit_message_received(&dispatch)
    } else if matches!(&dispatch.target, PostSendBuiltInTarget::Graft(_)) {
        crate::graft::deliver_published_receiver_hook(runtime, &dispatch)
    } else {
        return PostSendEmissionOutcome::NoCapability { hook_summary };
    };
    match emitted {
        Ok(path) => PostSendEmissionOutcome::Delivered { path, hook_summary },
        Err(error) => {
            let warning = post_send_warning("post-send emission failed", event, &error);
            warnings.push(warning.clone());
            PostSendEmissionOutcome::Failed {
                hook_summary,
                warning,
            }
        }
    }
}

#[cfg(test)]
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
) -> HookExecutionSummary {
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
        return HookExecutionSummary::new(0, 0, 0);
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
}

fn build_built_in_dispatch<R>(
    runtime: &R,
    delivery_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
    event: &PostSendHookEvent,
) -> Option<BuiltInPostSendDispatch>
where
    R: RetainedServiceRuntime + ?Sized,
{
    if delivery_snapshot.local_tmux_post_send {
        let pane_id = event
            .recipient_pane_id
            .clone()
            .or_else(|| delivery_snapshot.recipient_pane_id.as_ref().cloned())?;
        let kind = built_in_nudge_template_kind_from_post_send_event(event);
        let override_row = match runtime.load_nudge_template_override(&event.recipient_team, kind) {
            Ok(row) => row,
            Err(error) => {
                warn!(
                    code = %error.code(),
                    recipient = %event.recipient,
                    recipient_team = %event.recipient_team,
                    message_id = %event.message_id,
                    %error,
                    "failed to load built-in nudge template override; falling back to default"
                );
                None
            }
        };
        let template = nudge_template::resolve_template(override_row, kind);
        let template_body = template.body.as_deref()?;
        let rendered_nudge = match nudge_template::render_built_in_nudge(event, template_body) {
            Ok(rendered) => rendered,
            Err(error) => {
                warn!(
                    code = %error.code(),
                    recipient = %event.recipient,
                    recipient_team = %event.recipient_team,
                    message_id = %event.message_id,
                    %error,
                    "failed to render built-in tmux nudge"
                );
                return None;
            }
        };
        return Some(BuiltInPostSendDispatch {
            event: event.clone(),
            target: PostSendBuiltInTarget::LocalTmux(LocalTmuxNudgeTarget {
                pane_id,
                rendered_nudge,
            }),
        });
    }
    if delivery_snapshot.graft_post_send {
        return Some(BuiltInPostSendDispatch {
            event: event.clone(),
            target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient: event.recipient.clone(),
                recipient_team: event.recipient_team.clone(),
            }),
        });
    }
    None
}

fn execute_post_send_hook(
    warnings: &mut Vec<WarningEntry>,
    config: &AtmConfig,
    rule: &config::types::PostSendHookRule,
    event: &PostSendHookEvent,
) -> bool {
    // This function performs blocking child-process supervision with short
    // sleeps. It is safe for the current CLI call path and must stay off async
    // runtime threads unless wrapped in spawn_blocking by the caller.
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
        "from": qualified_nudge_sender_identity(event),
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
            "authenticated_source_host": event.authenticated_source_host.as_ref().map(ToString::to_string),
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
        sender_chat_id: message.envelope.source_chat_id.clone(),
        sender_team: message
            .envelope
            .source_team
            .clone()
            .unwrap_or_else(|| recipient.team.clone()),
        authenticated_source_host: authenticated_source_host(&message.envelope)
            .unwrap_or_else(|error| {
                warn!(error = %error, "discarding invalid persisted authenticated source host from nudge event");
                None
            }),
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

#[cfg(test)]
fn sender_config_root(metadata: &serde_json::Map<String, Value>) -> Option<PathBuf> {
    compatible_home_dir(metadata).map(Into::into)
}

fn post_send_warning(prefix: &str, event: &PostSendHookEvent, error: &AtmError) -> WarningEntry {
    WarningEntry::with_code(
        error.code(),
        format!(
            "warning: {prefix} for {}@{} message {} ({}): {}.",
            event.recipient,
            event.recipient_team,
            event.message_id,
            error.code(),
            error.message()
        ),
        Some(error.message().to_owned()),
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
    use std::sync::Mutex;

    use serde_json::{Map, Value, json};
    use tempfile::tempdir;
    use tracing::Level;

    use super::{
        HookCancellationToken, POST_SEND_HOOK_MAX_STDOUT_BYTES, PostSendHookResultLevel,
        emit_post_send_effects, finish_abandoned_post_send_hook_stdout_capture,
        hook_matches_recipient, hook_result_log_level, load_post_send_config_for_sender,
        parse_post_send_hook_result, post_send_event_from_message, post_send_hook_payload,
        sender_config_root,
    };
    use crate::boundary::{
        self, BuiltInNudgeTemplateKind, BuiltInPostSendDispatch, GraftNudgeTarget,
        MessageReceivedHookEmitter, PostSendBuiltInTarget, PostSendEmissionPath, RosterEntry,
        RosterHarness, RosterMemberKind, TeamNudgeTemplateOverrideMode,
        TeamNudgeTemplateOverrideRow,
    };
    use crate::config::AtmConfig;
    use crate::config::types::{HookRecipient, PostSendHookRule};
    use crate::delivery_plan::LogicalMessage;
    use crate::delivery_policy::{DeliveryHarnessPath, DeliveryRecipientSnapshot};
    use crate::error::AtmError;
    use crate::error_codes::AtmErrorCode;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::AckIntentFields;
    #[allow(
        deprecated,
        reason = "Phase AD obsolete: derived compatibility field only. Hook tests intentionally exercise the retained legacy cwd compatibility seam."
    )]
    use crate::schema::agent_member::LEGACY_CWD_METADATA_KEY;
    use crate::schema::{AtmMessageId, HOME_DIR_METADATA_KEY, InboxMessage};
    use crate::send::ResolvedRecipient;
    use crate::service_runtime::RetainedServiceRuntime;
    use crate::test_support::{EnvGuard, TEST_SENDER};
    use crate::types::{AgentName, ChatId, IsoTimestamp, PaneId, TeamName};

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
    }

    struct HookEmissionRuntime {
        override_row: Option<TeamNudgeTemplateOverrideRow>,
    }

    impl HookEmissionRuntime {
        fn new(override_row: Option<TeamNudgeTemplateOverrideRow>) -> Self {
            Self { override_row }
        }
    }

    impl crate::boundary::sealed::Sealed for HookEmissionRuntime {}

    impl RetainedServiceRuntime for HookEmissionRuntime {
        fn load_config(&self, _current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
            Ok(None)
        }

        fn load_nudge_template_override(
            &self,
            _team: &TeamName,
            _kind: BuiltInNudgeTemplateKind,
        ) -> Result<Option<TeamNudgeTemplateOverrideRow>, AtmError> {
            Ok(self.override_row.clone())
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            unreachable!("hook emission test does not resolve inbox paths")
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

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &DeliveryRecipientSnapshot,
            _messages: &[InboxMessage],
        ) -> Result<(), AtmError> {
            unreachable!("hook emission test does not deliver non-claude payloads")
        }

        fn load_roster_member(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<RosterEntry>, AtmError> {
            Ok(None)
        }

        fn load_team_roster(&self, _team: &TeamName) -> Result<Vec<RosterEntry>, AtmError> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct RecordingEmitter {
        emitted: Mutex<Vec<BuiltInPostSendDispatch>>,
    }

    impl RecordingEmitter {
        fn emitted(&self) -> Vec<BuiltInPostSendDispatch> {
            self.emitted.lock().expect("emitter lock").clone()
        }
    }

    impl boundary::sealed::Sealed for RecordingEmitter {}

    impl MessageReceivedHookEmitter for RecordingEmitter {
        fn emit_message_received(
            &self,
            dispatch: &BuiltInPostSendDispatch,
        ) -> Result<PostSendEmissionPath, AtmError> {
            self.emitted
                .lock()
                .expect("emitter lock")
                .push(dispatch.clone());
            Ok(match dispatch.target {
                PostSendBuiltInTarget::LocalTmux(_) => PostSendEmissionPath::LocalTmux,
                PostSendBuiltInTarget::Graft(_) => PostSendEmissionPath::GraftPort,
            })
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

    fn logical_message(text: &str) -> LogicalMessage {
        let ack_intent = AckIntentFields::not_required();
        LogicalMessage::new(
            InboxMessage {
                from: AgentName::from_validated(TEST_SENDER),
                source_chat_id: None,
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TeamName::from_validated("test-team")),
                destination_chat_id: None,
                summary: Some(text.to_string()),
                message_id: Some(AtmMessageId::new()),
                requires_ack: ack_intent.requires_ack,
                pending_ack_at: ack_intent.pending_ack_at,
                acknowledged_at: ack_intent.acknowledged_at,
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
    fn post_send_event_preserves_the_canonical_source_chat_id() {
        let mut message = logical_message("chat-scoped nudge");
        let chat_id = "chat-42".parse::<ChatId>().expect("chat id");
        message.envelope.source_chat_id = Some(chat_id.clone());
        let recipient = ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated("test-team"),
        };

        let event = post_send_event_from_message(&recipient, &message, None);

        assert_eq!(event.sender_chat_id, Some(chat_id));
        assert_eq!(
            event.source_address().to_string(),
            "sender-a:chat-42@test-team"
        );
    }

    #[test]
    fn post_send_event_preserves_authenticated_peer_host() {
        let mut message = logical_message("peer nudge");
        message.envelope.extra.insert(
            "sourceHost".to_string(),
            Value::String("rand-m5.local".to_string()),
        );
        let recipient = ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated("test-team"),
        };

        let event = post_send_event_from_message(&recipient, &message, None);

        assert_eq!(
            event
                .authenticated_source_host
                .expect("authenticated host")
                .as_str(),
            "rand-m5.local"
        );
    }

    #[test]
    fn post_send_hook_payload_includes_compact_authenticated_peer_host() {
        let mut message = logical_message("peer nudge");
        message.envelope.extra.insert(
            "sourceHost".to_string(),
            Value::String("rand-m5.local".to_string()),
        );
        let recipient = ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated("test-team"),
        };

        let event = post_send_event_from_message(&recipient, &message, None);

        assert_eq!(
            post_send_hook_payload(&event)["from"],
            "sender-a@test-team.rand-m5"
        );
    }

    fn install_test_home(home_dir: &Path) -> EnvGuard {
        EnvGuard::set_many([
            ("HOME", home_dir.to_str()),
            ("USERPROFILE", None),
            ("ATM_LOG_DIR", None),
        ])
    }

    fn read_notification_events(_home_dir: &Path) -> Vec<crate::protocol::NotificationEvent> {
        let notification_path = crate::home::host_runtime_dir()
            .expect("host runtime dir")
            .join("notifications.jsonl");
        match fs::read_to_string(notification_path) {
            Ok(contents) => contents
                .lines()
                .map(|line| serde_json::from_str(line).expect("notification event"))
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("failed to read notification log: {error}"),
        }
    }

    #[test]
    #[serial_test::serial(env)]
    fn built_in_fallback_dispatches_local_tmux_through_emitter() {
        let tempdir = tempdir().expect("tempdir");
        let _env = install_test_home(tempdir.path());
        let runtime = HookEmissionRuntime::new(None);
        let emitter = RecordingEmitter::default();
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
            &runtime,
            &mut warnings,
            None,
            Some(&emitter),
            &recipient,
            &snapshot,
            &[logical_message("hello")],
        );

        assert!(warnings.is_empty());
        let emitted = emitter.emitted();
        assert_eq!(emitted.len(), 1);
        let dispatch = &emitted[0];
        match &dispatch.target {
            PostSendBuiltInTarget::LocalTmux(target) => {
                assert_eq!(target.pane_id, PaneId::from_cli("%9").expect("pane"));
                assert!(target.rendered_nudge.contains("read atm --team test-team"));
                assert!(
                    target
                        .rendered_nudge
                        .contains(&dispatch.event.message_id.to_string())
                );
                assert!(target.rendered_nudge.contains("hello"));
            }
            other => panic!("expected local tmux dispatch, got {other:?}"),
        }

        let notifications = read_notification_events(tempdir.path());
        assert!(!notifications.is_empty());
    }

    #[test]
    #[serial_test::serial(env)]
    fn external_post_send_hook_takes_precedence_over_built_in_nudge() {
        let tempdir = tempdir().expect("tempdir");
        let hook_capture = tempdir.path().join("hook-capture.txt");
        #[cfg(windows)]
        let hook_path = tempdir.path().join("hook.cmd");
        #[cfg(not(windows))]
        let hook_path = tempdir.path().join("hook");
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&hook_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook_path, perms).expect("chmod");
        }

        let hook_capture_value = hook_capture.display().to_string();
        let _env = EnvGuard::set_many([
            ("ATM_TEST_HOOK_CAPTURE", Some(hook_capture_value.as_str())),
            ("ATM_HOME", tempdir.path().to_str()),
            ("ATM_CONFIG_HOME", tempdir.path().to_str()),
            ("HOME", tempdir.path().to_str()),
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
        let runtime = HookEmissionRuntime::new(None);
        let emitter = RecordingEmitter::default();

        emit_post_send_effects(
            &runtime,
            &mut warnings,
            Some(&config),
            Some(&emitter),
            &recipient,
            &snapshot,
            &[logical_message("hello")],
        );

        let captured = fs::read_to_string(&hook_capture).expect("hook capture");
        assert!(captured.contains("\"description\":\"hello\""));
        assert!(emitter.emitted().is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    #[serial_test::serial(env)]
    fn mixed_success_hook_accounting_preserves_delivery_and_warning() {
        let tempdir = tempdir().expect("tempdir");
        let hook_capture = tempdir.path().join("hook-capture.txt");
        #[cfg(windows)]
        let hook_ok = tempdir.path().join("hook-ok.cmd");
        #[cfg(not(windows))]
        let hook_ok = tempdir.path().join("hook-ok");
        #[cfg(windows)]
        let hook_fail = tempdir.path().join("hook-fail.cmd");
        #[cfg(not(windows))]
        let hook_fail = tempdir.path().join("hook-fail");
        #[cfg(windows)]
        fs::write(
            &hook_ok,
            "@echo off\r\nsetlocal EnableDelayedExpansion\r\n> \"%ATM_TEST_HOOK_CAPTURE%\" echo !ATM_POST_SEND!\r\nexit /b 0\r\n",
        )
        .expect("write ok hook");
        #[cfg(not(windows))]
        fs::write(
            &hook_ok,
            "#!/bin/sh\nprintf '%s\\n' \"$ATM_POST_SEND\" > \"$ATM_TEST_HOOK_CAPTURE\"\nexit 0\n",
        )
        .expect("write ok hook");
        #[cfg(windows)]
        fs::write(&hook_fail, "@echo off\r\nexit /b 7\r\n").expect("write failing hook");
        #[cfg(not(windows))]
        fs::write(&hook_fail, "#!/bin/sh\nexit 7\n").expect("write failing hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&hook_ok, &hook_fail] {
                let mut perms = fs::metadata(path).expect("metadata").permissions();
                perms.set_mode(0o755);
                fs::set_permissions(path, perms).expect("chmod");
            }
        }

        let hook_capture_value = hook_capture.display().to_string();
        let _env = EnvGuard::set_many([
            ("ATM_TEST_HOOK_CAPTURE", Some(hook_capture_value.as_str())),
            ("ATM_HOME", tempdir.path().to_str()),
            ("ATM_CONFIG_HOME", tempdir.path().to_str()),
            ("HOME", tempdir.path().to_str()),
        ]);

        let config = AtmConfig {
            config_root: tempdir.path().to_path_buf(),
            post_send_hooks: vec![
                PostSendHookRule {
                    recipient: HookRecipient::Named("recipient".parse().expect("recipient")),
                    command: vec![hook_ok.display().to_string()],
                },
                PostSendHookRule {
                    recipient: HookRecipient::Named("recipient".parse().expect("recipient")),
                    command: vec![hook_fail.display().to_string()],
                },
            ],
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
        let runtime = HookEmissionRuntime::new(None);
        let emitter = RecordingEmitter::default();
        let mut warnings = Vec::new();

        emit_post_send_effects(
            &runtime,
            &mut warnings,
            Some(&config),
            Some(&emitter),
            &recipient,
            &snapshot,
            &[logical_message("hello")],
        );

        assert_eq!(emitter.emitted().len(), 0);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].code,
            Some(AtmErrorCode::WarningHookExecutionFailed)
        );
        let notifications = read_notification_events(tempdir.path());
        assert!(!notifications.is_empty());
    }

    #[test]
    #[serial_test::serial(env)]
    fn graft_fallback_dispatches_through_emitter_without_tmux_fields() {
        let tempdir = tempdir().expect("tempdir");
        let _env = install_test_home(tempdir.path());
        let runtime = HookEmissionRuntime::new(Some(TeamNudgeTemplateOverrideRow {
            team_name: TeamName::from_validated("test-team"),
            kind: BuiltInNudgeTemplateKind::Delivery,
            mode: TeamNudgeTemplateOverrideMode::Override {
                template_body: "<ignored/>".to_string(),
            },
            updated_at: IsoTimestamp::now(),
        }));
        let emitter = RecordingEmitter::default();
        let recipient = ResolvedRecipient {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated("test-team"),
        };
        let snapshot = DeliveryRecipientSnapshot {
            agent: recipient.agent.clone(),
            team: recipient.team.clone(),
            harness: DeliveryHarnessPath::NonClaude,
            recipient_pane_id: None,
            local_tmux_post_send: false,
            graft_post_send: true,
            roster_backed: true,
        };
        let mut warnings = Vec::new();

        emit_post_send_effects(
            &runtime,
            &mut warnings,
            None,
            Some(&emitter),
            &recipient,
            &snapshot,
            &[logical_message("hello")],
        );

        assert!(warnings.is_empty());
        let emitted = emitter.emitted();
        assert_eq!(emitted.len(), 1);
        match &emitted[0].target {
            PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient,
                recipient_team,
            }) => {
                assert_eq!(recipient, &AgentName::from_validated("recipient"));
                assert_eq!(recipient_team, &TeamName::from_validated("test-team"));
            }
            other => panic!("expected graft dispatch, got {other:?}"),
        }
    }
}
