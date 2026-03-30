use anyhow::Result;
use atm_core::send::SendOutcome;

pub fn print_send_result(outcome: &SendOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!(
            "Sent to {}@{} [message_id: {}]",
            outcome.agent, outcome.team, outcome.message_id
        );
    }

    Ok(())
}
