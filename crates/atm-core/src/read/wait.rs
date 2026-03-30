use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::AtmError;
use crate::home;
use crate::mailbox;
use crate::schema::MessageEnvelope;

pub fn wait_for_eligible_message<F>(
    home_dir: &Path,
    team: &str,
    agent: &str,
    timeout_secs: u64,
    is_eligible: F,
) -> Result<bool, AtmError>
where
    F: Fn(&MessageEnvelope) -> bool,
{
    let inbox_path = home::inbox_path_from_home(home_dir, team, agent)?;
    let timeout = Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_millis(100);
    let start = Instant::now();

    loop {
        let messages = mailbox::read_messages(&inbox_path)?;
        if messages.iter().any(&is_eligible) {
            return Ok(true);
        }

        if start.elapsed() >= timeout {
            return Ok(false);
        }

        thread::sleep(poll_interval);
    }
}
