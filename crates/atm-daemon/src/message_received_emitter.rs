use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use atm_core::boundary::{
    self, BuiltInPostSendDispatch, LocalTmuxNudgeTarget, MessageReceivedHookEmitter,
    PostSendBuiltInTarget, PostSendEmissionPath, PostSendHookEvent, RosterEntry,
};
use atm_core::error::{AtmError, AtmErrorCode};

const TMUX_DOUBLE_ENTER_DELAY: Duration = Duration::from_millis(275);
const TMUX_SEND_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const TMUX_PROGRAM_ENV: &str = "ATM_TEST_TMUX_BIN";

#[derive(Clone, Default)]
pub(crate) struct TmuxMessageReceivedHookEmitter;

impl boundary::sealed::Sealed for TmuxMessageReceivedHookEmitter {}

impl MessageReceivedHookEmitter for TmuxMessageReceivedHookEmitter {
    fn emit_post_send(
        &self,
        dispatch: &BuiltInPostSendDispatch,
    ) -> Result<PostSendEmissionPath, AtmError> {
        let PostSendBuiltInTarget::LocalTmux(target) = &dispatch.target else {
            return Err(AtmError::validation(
                "tmux message-received emitter received a non-tmux target",
            ));
        };
        deliver_tmux_nudge(&dispatch.event, target)?;
        Ok(PostSendEmissionPath::LocalTmux)
    }
}

/// Selects the daemon's tmux receiver implementation. Graft's independently
/// running receiver is reached by core endpoint transport, not a daemon
/// implementation or an `atm-graft` dependency.
pub(crate) fn message_received_emitter_for_harness(
    member: &RosterEntry,
) -> Option<Box<dyn MessageReceivedHookEmitter>> {
    let uses_tmux = member.recipient_pane_id.is_some()
        || member
            .metadata_json
            .get("backendType")
            .and_then(serde_json::Value::as_str)
            == Some("tmux");
    uses_tmux
        .then(|| Box::new(TmuxMessageReceivedHookEmitter) as Box<dyn MessageReceivedHookEmitter>)
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
