//! Hook and cron error constructors (design §9.1).
//!
//! Each constructor fixes its wire code and display prefix. Codes whose
//! catalog guidance is non-empty get the usual `Recovery:` line; the hook's
//! own refusal text is returned verbatim.

use std::path::PathBuf;

use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::hook_types::PhaseId;

/// Why a hook process produced no usable reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookExitCause {
    /// The hook exited non-zero; `stderr_tail` is the last bytes of its stderr.
    Exit { code: i32, stderr_tail: String },
    /// The hook was terminated by a signal.
    Signal(i32),
    /// The hook could not be started at `path`.
    Spawn { path: PathBuf, cause: String },
    /// The runner could not read the hook's `stdout` or `stderr` pipe.
    Read { stream: String, cause: String },
    /// The hook runner panicked; `payload` is the panic message.
    Panic(String),
    /// A required interpreter or environment entry is missing.
    Missing(String),
}

impl AtmError {
    pub fn hook_exit(cause: HookExitCause) -> Self {
        let detail = match cause {
            HookExitCause::Exit { code, stderr_tail } => format!("exit {code}: {stderr_tail}"),
            HookExitCause::Signal(signal) => format!("terminated by signal {signal}"),
            HookExitCause::Spawn { path, cause } => {
                format!("cannot run {}: {cause}", path.display())
            }
            HookExitCause::Read { stream, cause } => format!("cannot read {stream}: {cause}"),
            HookExitCause::Panic(payload) => format!("hook runner panicked: {payload}"),
            HookExitCause::Missing(what) => what,
        };
        Self::new(AtmErrorCode::HookExit, format!("HOOK.EXIT: {detail}"))
    }

    pub fn hook_timeout(phase: &PhaseId, detail: &str) -> Self {
        Self::new(
            AtmErrorCode::HookTimeout,
            format!("HOOK.TIMEOUT: phase {phase}: {detail}"),
        )
    }

    pub fn hook_malformed(detail: &str) -> Self {
        Self::new(
            AtmErrorCode::HookMalformed,
            format!("HOOK.MALFORMED: {detail}"),
        )
    }

    pub fn hook_module_invalid(detail: &str) -> Self {
        Self::new(
            AtmErrorCode::HookModuleInvalid,
            format!("HOOK.MODULE_INVALID: {detail}"),
        )
    }

    /// The hook's refusal message, shown verbatim with no `Recovery:` line.
    pub fn hook_refused(message: &str) -> Self {
        Self::new(AtmErrorCode::HookRefused, message)
    }

    pub fn cron_expr_invalid(detail: &str) -> Self {
        Self::new(
            AtmErrorCode::CronExprInvalid,
            format!("CRON.EXPR_INVALID: {detail}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::HookExitCause;
    use crate::error::AtmError;
    use crate::hook_types::PhaseId;

    #[test]
    fn hook_error_constructors_format_and_classify() {
        let exit = |cause| AtmError::hook_exit(cause).into_message();
        let recovery = "Fix the hook module so it starts and exits with a valid reply, then retry.";
        for (cause, detail) in [
            (
                HookExitCause::Exit {
                    code: 2,
                    stderr_tail: "boom".into(),
                },
                "exit 2: boom",
            ),
            (HookExitCause::Signal(9), "terminated by signal 9"),
            (
                HookExitCause::Spawn {
                    path: PathBuf::from("/h.py"),
                    cause: "denied".into(),
                },
                "cannot run /h.py: denied",
            ),
            (
                HookExitCause::Read {
                    stream: "stderr".into(),
                    cause: "connection reset".into(),
                },
                "cannot read stderr: connection reset",
            ),
            (
                HookExitCause::Panic("oops".into()),
                "hook runner panicked: oops",
            ),
            (
                HookExitCause::Missing("python3 not found on PATH".into()),
                "python3 not found on PATH",
            ),
        ] {
            assert_eq!(
                exit(cause),
                format!("HOOK.EXIT: {detail}\n  Recovery: {recovery}")
            );
        }

        let phase = PhaseId::parse("bk").expect("phase");
        let timeout = AtmError::hook_timeout(&phase, "no reply within 2500 ms");
        assert_eq!(
            timeout.message(),
            concat!(
                "HOOK.TIMEOUT: phase bk: no reply within 2500 ms\n  Recovery: ",
                "Make the hook reply within its timeout or raise the timeout within the budget, then retry."
            )
        );
        assert_eq!(
            AtmError::hook_malformed("empty atm-action list").message(),
            "HOOK.MALFORMED: empty atm-action list\n  Recovery: Make the hook reply match the JSON reply contract, then retry."
        );
        assert_eq!(
            AtmError::hook_module_invalid("cron.tick is not a task hook").message(),
            concat!(
                "HOOK.MODULE_INVALID: cron.tick is not a task hook\n  Recovery: ",
                "Register a task hook module path and name the hook from the supported list, then retry."
            )
        );
        assert_eq!(
            AtmError::cron_expr_invalid("bad field").message(),
            concat!(
                "CRON.EXPR_INVALID: bad field\n  Recovery: ",
                "Use a five-field cron expression or an --until time later than now, then retry."
            )
        );

        let refused = AtmError::hook_refused("bd not ready");
        assert_eq!(refused.message(), "bd not ready");
        assert_eq!(refused.to_string(), "bd not ready");
        assert!(!refused.to_string().contains("Recovery:"));

        let validation_codes = [
            AtmError::hook_module_invalid("x"),
            AtmError::cron_expr_invalid("x"),
            AtmError::hook_refused("x"),
        ];
        for error in &validation_codes {
            assert!(error.is_validation(), "{}", error.code().as_str());
        }
        for error in [
            AtmError::hook_exit(HookExitCause::Signal(1)),
            timeout,
            AtmError::hook_malformed("x"),
        ] {
            assert!(!error.is_validation(), "{}", error.code().as_str());
        }
    }
}
