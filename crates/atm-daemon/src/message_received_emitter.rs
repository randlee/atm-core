use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use atm_core::RequestDeadline;
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
    fn emit_received_message(
        &self,
        dispatch: &BuiltInPostSendDispatch,
        deadline: RequestDeadline,
    ) -> Result<PostSendEmissionPath, AtmError> {
        let PostSendBuiltInTarget::LocalTmux(target) = &dispatch.target else {
            return Err(AtmError::validation(
                "tmux message-received emitter received a non-tmux target",
            ));
        };
        deliver_tmux_nudge(&dispatch.event, target, deadline)?;
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
    deadline: RequestDeadline,
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
        deadline,
        "send literal nudge",
    )?;
    run_tmux_command(
        {
            let mut command = tmux_command();
            command.args(["send-keys", "-t", target.pane_id.as_str(), "Enter"]);
            command
        },
        event,
        deadline,
        "send first Enter to nudge pane",
    )?;
    let delay = tmux_remaining_budget(deadline)?.min(TMUX_DOUBLE_ENTER_DELAY);
    thread::sleep(delay);
    // A shortened sleep can consume the final positive duration exactly. Do
    // not start a third command after the inherited request budget is spent.
    tmux_remaining_budget(deadline)?;
    run_tmux_command(
        {
            let mut command = tmux_command();
            command.args(["send-keys", "-t", target.pane_id.as_str(), "Enter"]);
            command
        },
        event,
        deadline,
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
    deadline: RequestDeadline,
    action: &'static str,
) -> Result<(), AtmError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|_source| AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed))?;
    let output = wait_for_tmux_output(child, deadline, action)?;
    ensure_tmux_success(output, event, action)
}

fn wait_for_tmux_output(
    mut child: Child,
    deadline: RequestDeadline,
    _action: &'static str,
) -> Result<Output, AtmError> {
    let safety_deadline = Instant::now() + TMUX_SEND_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|_source| AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed));
            }
            Ok(None) => {
                let Some(request_remaining) = deadline.remaining() else {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed));
                };
                let Some(safety_remaining) = safety_deadline.checked_duration_since(Instant::now())
                else {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed));
                };
                thread::sleep(
                    Duration::from_millis(50)
                        .min(request_remaining)
                        .min(safety_remaining),
                );
            }
            Err(_source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed));
            }
        }
    }
}

fn tmux_remaining_budget(deadline: RequestDeadline) -> Result<Duration, AtmError> {
    deadline
        .remaining()
        .map(|remaining| remaining.min(TMUX_SEND_TIMEOUT))
        .ok_or_else(|| AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed))
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[cfg(unix)]
    use std::process::{Command, Stdio};
    #[cfg(unix)]
    use std::time::Instant;

    use atm_core::RequestDeadline;
    #[cfg(unix)]
    use atm_core::error_codes::AtmErrorCode;

    #[cfg(unix)]
    use super::wait_for_tmux_output;
    use super::{TMUX_SEND_TIMEOUT, tmux_remaining_budget};

    #[test]
    fn hook_safety_cap_never_enlarges_the_request_budget() {
        let capped = tmux_remaining_budget(RequestDeadline::after(Duration::from_secs(30)))
            .expect("positive request budget");

        assert!(capped <= TMUX_SEND_TIMEOUT);
    }

    #[cfg(unix)]
    #[test]
    fn stalled_tmux_child_is_killed_when_the_inherited_request_budget_expires() {
        let child = Command::new("sh")
            .args(["-c", "sleep 1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn stalled child");
        let started = Instant::now();

        let error = wait_for_tmux_output(
            child,
            RequestDeadline::after(Duration::from_millis(25)),
            "test stalled child",
        )
        .expect_err("the inherited request deadline must stop a stalled hook");

        assert_eq!(error.code(), AtmErrorCode::PostSendTmuxSendFailed);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "the hook must not wait for the child safety cap after request expiry"
        );
    }
}
