use anyhow::Result;
use atm_core::ack::AckOutcome;
use atm_core::clear::ClearOutcome;
use atm_core::log::{LogQueryResult, LogRecord};
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

pub fn print_ack_result(outcome: &AckOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
    } else {
        println!(
            "Acknowledged {} for {}@{} and sent reply {} to {}",
            outcome.message_id,
            outcome.agent,
            outcome.team,
            outcome.reply_message_id,
            outcome.reply_target
        );
    }

    Ok(())
}

pub fn print_clear_result(outcome: &ClearOutcome, dry_run: bool, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
        return Ok(());
    }

    if dry_run {
        println!(
            "Dry run: would remove {} message(s) from {}@{}",
            outcome.removed_total, outcome.agent, outcome.team
        );
    } else {
        println!(
            "Cleared {} message(s) from {}@{}",
            outcome.removed_total, outcome.agent, outcome.team
        );
    }

    println!(
        "Acknowledged: {} | Read: {} | Remaining: {}",
        outcome.removed_by_class.acknowledged,
        outcome.removed_by_class.read,
        outcome.remaining_total
    );

    Ok(())
}

pub fn print_log_result(outcome: &LogQueryResult, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(outcome)?);
        return Ok(());
    }

    for record in &outcome.records {
        print_log_human(record);
    }

    Ok(())
}

pub fn print_log_record(record: &LogRecord, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(record)?);
    } else {
        print_log_human(record);
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
            message.envelope.timestamp.into_inner().to_rfc3339(),
            message.envelope.from,
            message
                .envelope
                .summary
                .as_deref()
                .unwrap_or(message.envelope.text.as_str())
        );
        if let Some(message_id) = message.envelope.message_id {
            println!("  message_id: {message_id}");
        }
    }
}

fn print_log_human(record: &LogRecord) {
    let message = record.message.as_deref().unwrap_or(record.event.as_str());
    let mut field_pairs = record
        .fields
        .iter()
        .map(|(key, value)| format!("{key}={}", render_value(value)))
        .collect::<Vec<_>>();
    field_pairs.sort();

    if field_pairs.is_empty() {
        println!(
            "{} {:?} {} {}",
            record.timestamp.into_inner().to_rfc3339(),
            record.level,
            record.service,
            message
        );
    } else {
        println!(
            "{} {:?} {} {} {}",
            record.timestamp.into_inner().to_rfc3339(),
            record.level,
            record.service,
            message,
            field_pairs.join(" ")
        );
    }
}

fn render_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => value.to_string(),
    }
}
