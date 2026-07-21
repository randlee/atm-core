use std::thread;
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
    FEligible: Fn(&[T]) -> Result<bool, AtmError>,
{
    let timeout = Duration::from_secs(timeout_secs);
    let start = Instant::now();

    loop {
        if start.elapsed() >= timeout {
            return Ok(false);
        }

        let messages = load_messages()?;
        if is_eligible(&messages)? {
            return Ok(true);
        }

        thread::sleep(DEFAULT_WAIT_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::wait_for_eligible_message;
    use crate::error::AtmError;

    #[test]
    fn wait_for_eligible_message_propagates_eligibility_errors() {
        let error = wait_for_eligible_message(
            1,
            || Ok(vec![1_u8]),
            |_| Err(AtmError::mailbox_read("simulated durable reload failure")),
        )
        .expect_err("eligibility errors should propagate");

        assert!(error.code() == crate::error_codes::AtmErrorCode::MailboxReadFailed);
        assert!(error.message().contains("simulated durable reload failure"));
    }
}
