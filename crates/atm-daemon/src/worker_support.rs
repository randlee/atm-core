use atm_core::error::AtmError;
use std::sync::Mutex;
use std::thread::JoinHandle;

#[derive(Debug, Default)]
pub(crate) struct JoinHandleOwner {
    // Narrow RBP-006 exception: this mutex owns only the install-once /
    // take-once handoff for one worker JoinHandle and must not expand into
    // general runtime coordination or request-state ownership.
    join_handle: Mutex<Option<JoinHandle<()>>>,
}

impl JoinHandleOwner {
    pub(crate) fn install(&self, handle: JoinHandle<()>) -> Result<(), AtmError> {
        let mut slot = self.join_handle.lock().map_err(|_| {
            AtmError::daemon_unavailable("worker join-handle ownership lock poisoned")
                .with_recovery(
                    "Restart atm-daemon; background worker join ownership can no longer be trusted.",
                )
        })?;
        if slot.is_some() {
            return Err(AtmError::validation(
                "worker join-handle ownership already contains a live handle",
            )
            .with_recovery(
                "Restart atm-daemon; a duplicate worker install violated the daemon worker-ownership contract.",
            ));
        }
        *slot = Some(handle);
        Ok(())
    }

    pub(crate) fn take(&self) -> Result<Option<JoinHandle<()>>, AtmError> {
        let mut slot = self.join_handle.lock().map_err(|_| {
            AtmError::daemon_unavailable("worker join-handle ownership lock poisoned")
                .with_recovery(
                    "Restart atm-daemon; background worker join ownership can no longer be trusted.",
                )
        })?;
        Ok(slot.take())
    }
}
