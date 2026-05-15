use std::time::{Duration, Instant};

use crate::error::AtmError;

pub(crate) const DEFAULT_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn wait_for_eligible_message<T, FLoad, FEligible>(
    timeout_secs: u64,
    mut load_messages: FLoad,
    is_eligible: FEligible,
) -> Result<bool, AtmError>
where
    FLoad: FnMut() -> Result<Vec<T>, AtmError>,
    FEligible: Fn(&[T]) -> bool,
{
    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();

    loop {
        if start.elapsed() >= timeout {
            return Ok(false);
        }

        let messages = load_messages()?;
        if is_eligible(&messages) {
            return Ok(true);
        }

        std::thread::park_timeout(DEFAULT_WAIT_POLL_INTERVAL);
    }
}
