use std::sync::Mutex;

use atm_core::error::AtmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RuntimeLifecycleState {
    Starting,
    Running,
    Draining,
    #[default]
    Stopped,
}

/// Serializes legal daemon runtime ownership transitions.
#[derive(Debug, Default)]
pub(crate) struct RuntimeLifecycle {
    /// A single mutex is sufficient here because lifecycle transitions are
    /// serialized control-plane events, not a high-frequency data path.
    state: Mutex<RuntimeLifecycleState>,
}

impl RuntimeLifecycle {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn state(&self) -> RuntimeLifecycleState {
        *self.state.lock().expect("runtime lifecycle state lock")
    }

    /// Transition the daemon runtime lifecycle to `next`.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] with
    /// [`atm_core::error_codes::AtmErrorCode::Validation`] when `next` would
    /// violate the documented state machine, or
    /// [`atm_core::error_codes::AtmErrorCode::DaemonUnavailable`] when the
    /// lifecycle lock is poisoned.
    pub(crate) fn transition(
        &self,
        next: RuntimeLifecycleState,
    ) -> Result<RuntimeLifecycleState, AtmError> {
        let mut state = self.state.lock().map_err(|_| {
            AtmError::daemon_unavailable("runtime lifecycle state lock poisoned").with_recovery(
                "Restart atm-daemon; runtime lifecycle transitions can no longer be trusted after the poisoned state lock.",
            )
        })?;
        let current = *state;
        if !matches!(
            (current, next),
            (
                RuntimeLifecycleState::Stopped,
                RuntimeLifecycleState::Starting
            ) | (
                RuntimeLifecycleState::Starting,
                RuntimeLifecycleState::Running
            ) | (
                RuntimeLifecycleState::Starting,
                RuntimeLifecycleState::Stopped
            ) | (
                RuntimeLifecycleState::Running,
                RuntimeLifecycleState::Draining
            ) | (
                RuntimeLifecycleState::Draining,
                RuntimeLifecycleState::Stopped
            )
        ) {
            return Err(AtmError::validation(format!(
                "illegal daemon runtime lifecycle transition: {current:?} -> {next:?}"
            ))
            .with_recovery("Enter daemon exclusively through RuntimeComposition::start()."));
        }
        *state = next;
        Ok(next)
    }

    /// Force the daemon runtime lifecycle back to `Stopped`.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] with
    /// [`atm_core::error_codes::AtmErrorCode::DaemonUnavailable`] when the
    /// lifecycle lock is poisoned while resetting the runtime state.
    pub(crate) fn force_stopped(&self) -> Result<(), AtmError> {
        let mut state = self.state.lock().map_err(|_| {
            AtmError::daemon_unavailable("runtime lifecycle state lock poisoned").with_recovery(
                "Restart atm-daemon; runtime lifecycle transitions can no longer be trusted after the poisoned state lock.",
            )
        })?;
        *state = RuntimeLifecycleState::Stopped;
        Ok(())
    }
}
