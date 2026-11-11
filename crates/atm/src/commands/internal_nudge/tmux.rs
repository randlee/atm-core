//! Synchronous tmux delivery. Called only through `spawn_blocking`.
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use atm_core::boundary::{PostSendHookEvent, TMUX_DOUBLE_ENTER_DELAY, TMUX_NUDGE_CONFIRM_KEY};
use atm_core::error::{AtmError, AtmErrorCode};

const TMUX_SEND_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct TmuxNudgeSink;

impl TmuxNudgeSink {
    pub(super) fn deliver(&self, event: &PostSendHookEvent, rendered: &str) -> Result<()> {
        self.deliver_with(event, rendered, run_tmux_command, thread::sleep)
    }

    pub(super) fn deliver_with(
        &self,
        event: &PostSendHookEvent,
        rendered: &str,
        mut run: impl FnMut(Command, &'static str) -> Result<()>,
        mut delay: impl FnMut(Duration),
    ) -> Result<()> {
        let pane_id = event
            .recipient_pane_id
            .as_ref()
            .ok_or_else(|| AtmError::for_code(AtmErrorCode::PostSendPaneMissing))?;
        run(
            {
                let mut command = tmux_command();
                command.args(["send-keys", "-t", pane_id.as_str(), "-l", rendered]);
                command
            },
            "send literal nudge",
        )?;
        run(
            {
                let mut command = tmux_command();
                command.args(["send-keys", "-t", pane_id.as_str(), TMUX_NUDGE_CONFIRM_KEY]);
                command
            },
            "send first Enter to nudge pane",
        )?;
        delay(TMUX_DOUBLE_ENTER_DELAY);
        run(
            {
                let mut command = tmux_command();
                command.args(["send-keys", "-t", pane_id.as_str(), TMUX_NUDGE_CONFIRM_KEY]);
                command
            },
            "send second Enter to nudge pane",
        )?;
        Ok(())
    }
}

fn tmux_command() -> Command {
    Command::new("tmux")
}

fn run_tmux_command(mut command: Command, action: &'static str) -> Result<()> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command.spawn().map_err(|source| {
        AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed).with_cause(source)
    })?;
    let output = wait_for_tmux_output(child, action)?;
    ensure_tmux_success(output, action).map_err(Into::into)
}

#[sc_lint_attributes::sc_lint(allow(SCB-RUNTIME-003), reason = "Tmux subprocess polling runs only on the dedicated spawn_blocking thread, with a bounded deadline.")]
fn wait_for_tmux_output(mut child: Child, _action: &'static str) -> Result<Output> {
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().map_err(|source| {
                    AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed)
                        .with_cause(source)
                        .into()
                });
            }
            Ok(None) if started_at.elapsed() < TMUX_SEND_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed).into());
            }
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed)
                    .with_cause(source)
                    .into());
            }
        }
    }
}

fn ensure_tmux_success(output: Output, action: &'static str) -> Result<(), AtmError> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if stderr.is_empty() {
        format!("tmux exited unsuccessfully while trying to {action}")
    } else {
        format!("tmux exited unsuccessfully while trying to {action}: {stderr}")
    };
    Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed).with_cause(detail))
}
