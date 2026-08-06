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
    fn emit_message_received(
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

/// Constructs the daemon-side tmux implementation when the recipient uses a
/// tmux harness. The independent Graft receiver is delivered by core transport
/// and invokes `atm-graft::GraftReceiveHook` in the client process.
pub(crate) fn message_received_emitter_for_harness(
    member: &RosterEntry,
) -> Option<Box<dyn MessageReceivedHookEmitter>> {
    let uses_tmux = member.recipient_pane_id.is_some()
        || member
            .metadata_json
            .get("backendType")
            .and_then(serde_json::Value::as_str)
            == Some("tmux");
    if uses_tmux {
        return Some(Box::new(TmuxMessageReceivedHookEmitter));
    }
    None
}

/// Removed. The daemon crate denies deprecations, so any attempted use is a
/// compilation error and must select one concrete receiver-side emitter.
#[deprecated(
    note = "DaemonPostSendHookEmitter was replaced by TmuxMessageReceivedHookEmitter; Graft delivery is endpoint transport, not a daemon emitter"
)]
#[allow(dead_code)]
pub(crate) struct DaemonPostSendHookEmitter {
    _private: (),
}

fn deliver_tmux_nudge(
    _event: &PostSendHookEvent,
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
        "send literal nudge",
    )?;
    run_tmux_command(
        {
            let mut command = tmux_command();
            command.args(["send-keys", "-t", target.pane_id.as_str(), "Enter"]);
            command
        },
        "send first Enter to nudge pane",
    )?;
    thread::sleep(TMUX_DOUBLE_ENTER_DELAY);
    run_tmux_command(
        {
            let mut command = tmux_command();
            command.args(["send-keys", "-t", target.pane_id.as_str(), "Enter"]);
            command
        },
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

fn run_tmux_command(mut command: Command, action: &'static str) -> Result<(), AtmError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|_source| AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed))?;
    let output = wait_for_tmux_output(child, action)?;
    ensure_tmux_success(output, action)
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

#[cfg(test)]
mod tests {
    use std::process::{ExitStatus, Output};

    use atm_core::error::AtmErrorCode;

    use super::ensure_tmux_success;

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;

    #[test]
    #[cfg(unix)]
    fn tmux_failure_retains_exit_diagnostics_as_error_cause() {
        let output = Output {
            status: ExitStatus::from_raw(3 << 8),
            stdout: Vec::new(),
            stderr: b"pane is gone\n".to_vec(),
        };

        let error = ensure_tmux_success(output, "send literal nudge").unwrap_err();

        assert_eq!(error.code(), AtmErrorCode::PostSendTmuxSendFailed);
        assert_eq!(
            error.cause(),
            Some("tmux exited unsuccessfully while trying to send literal nudge: pane is gone")
        );
    }
}
