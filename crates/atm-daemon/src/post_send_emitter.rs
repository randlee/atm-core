use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use atm_core::boundary::{
    self, BuiltInPostSendDispatch, GraftPostSendPort, LocalTmuxNudgeTarget, PostSendBuiltInTarget,
    PostSendEmissionPath, PostSendHookEmitter, PostSendHookEvent,
};
use atm_core::error::{AtmError, AtmErrorCode};

const TMUX_DOUBLE_ENTER_DELAY: Duration = Duration::from_millis(275);
const TMUX_SEND_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const TMUX_PROGRAM_ENV: &str = "ATM_TEST_TMUX_BIN";

#[derive(Clone)]
pub(crate) struct DaemonPostSendHookEmitter {
    graft_port: Arc<dyn GraftPostSendPort + Send + Sync>,
}

impl DaemonPostSendHookEmitter {
    pub(crate) fn new(graft_port: Arc<dyn GraftPostSendPort + Send + Sync>) -> Self {
        Self { graft_port }
    }
}

impl boundary::sealed::Sealed for DaemonPostSendHookEmitter {}

impl PostSendHookEmitter for DaemonPostSendHookEmitter {
    fn emit_post_send(
        &self,
        dispatch: &BuiltInPostSendDispatch,
    ) -> Result<PostSendEmissionPath, AtmError> {
        match &dispatch.target {
            PostSendBuiltInTarget::LocalTmux(target) => {
                deliver_tmux_nudge(&dispatch.event, target)?;
                Ok(PostSendEmissionPath::LocalTmux)
            }
            PostSendBuiltInTarget::Graft(target) => {
                self.graft_port.deliver_post_send(&dispatch.event, target)?;
                Ok(PostSendEmissionPath::GraftPort)
            }
        }
    }
}

fn deliver_tmux_nudge(
    event: &PostSendHookEvent,
    target: &LocalTmuxNudgeTarget,
) -> Result<(), AtmError> {
    run_tmux_command(
        {
            let mut command = tmux_command();
            command.args([
                "send-keys",
                "-t",
                target.pane_id.as_str(),
                "-l",
                &target.rendered_nudge,
            ]);
            command
        },
        event,
        "send literal nudge",
    )?;
    run_tmux_command(
        {
            let mut command = tmux_command();
            command.args(["send-keys", "-t", target.pane_id.as_str(), "Enter"]);
            command
        },
        event,
        "send first Enter to nudge pane",
    )?;
    thread::sleep(TMUX_DOUBLE_ENTER_DELAY);
    run_tmux_command(
        {
            let mut command = tmux_command();
            command.args(["send-keys", "-t", target.pane_id.as_str(), "Enter"]);
            command
        },
        event,
        "send second Enter to nudge pane",
    )
}

fn tmux_command() -> Command {
    #[cfg(test)]
    if let Some(program) = std::env::var_os(TMUX_PROGRAM_ENV).filter(|value| !value.is_empty()) {
        return Command::new(program);
    }
    Command::new("tmux")
}

fn run_tmux_command(
    mut command: Command,
    event: &PostSendHookEvent,
    action: &'static str,
) -> Result<(), AtmError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|_source| AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed))?;
    let output = wait_for_tmux_output(child, action)?;
    ensure_tmux_success(output, event, action)
}

fn wait_for_tmux_output(mut child: Child, _action: &'static str) -> Result<Output, AtmError> {
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|_source| AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed));
            }
            Ok(None) if started_at.elapsed() < TMUX_SEND_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed));
            }
            Err(_source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed));
            }
        }
    }
}

fn ensure_tmux_success(
    output: Output,
    _event: &PostSendHookEvent,
    action: &'static str,
) -> Result<(), AtmError> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let _detail = if stderr.is_empty() {
        format!("tmux exited unsuccessfully while trying to {action}")
    } else {
        format!("tmux exited unsuccessfully while trying to {action}: {stderr}")
    };
    Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed))
}
