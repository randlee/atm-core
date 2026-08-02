use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::Duration;

use atm_core::error::AtmError;

/// Retry interval used only after a bounded local-work queue is full.
pub(crate) const BOUNDED_ADMISSION_RETRY_INTERVAL: Duration = Duration::from_millis(1);

/// Hand work to a bounded queue with one common saturation contract.
///
/// The caller supplies the lifecycle or deadline check that applies after a
/// full queue. The normal path remains one `try_send` with no retry delay.
pub(crate) fn send_with_bounded_admission<T>(
    sender: &SyncSender<T>,
    work: T,
    mut retry_delay: impl FnMut() -> Result<Duration, AtmError>,
    disconnected_message: &'static str,
) -> Result<(), AtmError> {
    let mut work = work;
    loop {
        match sender.try_send(work) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(returned)) => {
                work = returned;
                std::thread::sleep(retry_delay()?);
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(AtmError::daemon_unavailable(disconnected_message));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn full_queue_stops_when_the_retry_boundary_expires() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        sender.send(()).expect("fill queue");

        let error = send_with_bounded_admission(
            &sender,
            (),
            || Err(AtmError::daemon_unavailable("test admission stopped")),
            "test receiver stopped",
        )
        .expect_err("retry boundary must stop a full queue");

        assert!(error.message().contains("test admission stopped"));
    }
}
