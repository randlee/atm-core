use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tracing::warn;

use crate::boundary::PostSendHookEvent;
use crate::error::{AtmError, AtmErrorCode};
use crate::types::{PaneId, TeamName};

use super::POST_SEND_HOOK_TIMEOUT;

#[cfg(test)]
const TMUX_PROGRAM_ENV: &str = "ATM_TEST_TMUX_BIN";
const TMUX_TERMINATE_REAP_TIMEOUT: Duration = Duration::from_millis(250);
const TMUX_TERMINATE_REAP_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(super) fn tmux_nudge_message(team: &TeamName) -> String {
    format!("You have unread ATM messages. Run: atm read --team {team}")
}

pub(super) fn run_tmux_send_keys(
    pane_id: &PaneId,
    message: &str,
    event: &PostSendHookEvent,
) -> Result<(), AtmError> {
    let output = run_tmux_command(
        {
            let mut command = tmux_command();
            command.args(["send-keys", "-t", pane_id.as_str(), "-l", message]);
            command
        },
        pane_id,
        event,
        "send-keys",
    )?;
    ensure_tmux_success(output, pane_id, event, "send literal nudge")
}

pub(super) fn run_tmux_send_enter(
    pane_id: &PaneId,
    event: &PostSendHookEvent,
) -> Result<(), AtmError> {
    let output = run_tmux_command(
        {
            let mut command = tmux_command();
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

fn tmux_command() -> Command {
    #[cfg(test)]
    if let Some(program) = std::env::var_os(TMUX_PROGRAM_ENV).filter(|value| !value.is_empty()) {
        return Command::new(program);
    }
    Command::new("tmux")
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

    let deadline = Instant::now() + TMUX_TERMINATE_REAP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(TMUX_TERMINATE_REAP_POLL_INTERVAL);
            }
            Ok(None) => {
                warn!(
                    code = %AtmErrorCode::PostSendTmuxSendFailed,
                    sender = %event.sender,
                    recipient = %event.recipient,
                    recipient_team = %event.recipient_team,
                    message_id = %event.message_id,
                    pane_id = %pane_id,
                    tmux_action,
                    "timed-out tmux subprocess did not exit within the bounded reap window"
                );
                return;
            }
            Err(error) => {
                warn!(
                    code = %AtmErrorCode::PostSendTmuxSendFailed,
                    sender = %event.sender,
                    recipient = %event.recipient,
                    recipient_team = %event.recipient_team,
                    message_id = %event.message_id,
                    pane_id = %pane_id,
                    tmux_action,
                    %error,
                    "failed while polling timed-out tmux subprocess during bounded reap"
                );
                return;
            }
        }
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
    let error = AtmError::new(
        AtmErrorCode::PostSendTmuxSendFailed,
        format!(
            "local tmux post-send emission failed for {}@{} pane {}: {message}",
            event.recipient, event.recipient_team, pane_id
        ),
    );
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
        Some(_source) => error,
        None => error,
    }
}
