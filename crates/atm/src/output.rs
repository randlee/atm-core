use anyhow::Result;
use atm_core::read::ReadOutcome;
use atm_core::send::SendOutcome;
use atm_core::types::DisplayBucket;

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

pub fn print_read_result(outcome: &ReadOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
        return Ok(());
    }

    println!("Queue: {}@{}", outcome.agent, outcome.team);
    println!(
        "Unread: {} | Pending-Ack: {} | History: {}",
        outcome.bucket_counts.unread,
        outcome.bucket_counts.pending_ack,
        outcome.bucket_counts.history
    );

    print_bucket(outcome, DisplayBucket::Unread, "Unread");
    print_bucket(outcome, DisplayBucket::PendingAck, "Pending Ack");

    if !outcome.history_collapsed {
        print_bucket(outcome, DisplayBucket::History, "History");
    } else if outcome.bucket_counts.history > 0 {
        println!();
        println!(
            "History: {} older messages hidden. Use --history or --all to show them.",
            outcome.bucket_counts.history
        );
    }

    Ok(())
}

fn print_bucket(outcome: &ReadOutcome, bucket: DisplayBucket, label: &str) {
    let messages = outcome
        .messages
        .iter()
        .filter(|message| message.bucket == bucket)
        .collect::<Vec<_>>();

    if messages.is_empty() {
        return;
    }

    println!();
    println!("{label}:");
    for message in messages {
        println!(
            "- {} {}: {}",
            message.message.timestamp.to_rfc3339(),
            message.message.from,
            message
                .message
                .summary
                .as_deref()
                .unwrap_or(message.message.text.as_str())
        );
        if let Some(message_id) = message.message.message_id {
            println!("  message_id: {message_id}");
        }
    }
}
