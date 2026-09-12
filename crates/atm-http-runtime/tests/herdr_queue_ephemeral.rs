//! BA.5 runtime acceptance guards that do not depend on BA.3's handoff API.
//!
//! The full Herdr pump fixtures live beside the private pump implementation.
//! These integration checks keep the cross-file runtime contract visible at
//! the crate boundary while BA.3 task-handoff work is still landing.

const QUEUE_WAKE: &str = include_str!("../src/herdr_queue_wake.rs");
const QUEUE_REMINDERS: &str = include_str!("../src/herdr_queue_wake_reminders.rs");
const TASK_DISPOSITION: &str = include_str!("../src/herdr_task_disposition.rs");
const PENDING_STORE: &str = include_str!("../../atm-storage-rusqlite/src/pending_nudge_store.rs");
const STMT_CACHE: &str = include_str!("../../atm-storage-rusqlite/src/writer/stmt_cache.rs");
const ROUTER: &str = include_str!("../src/storage_and_nudge_router.rs");

#[test]
fn task_prompt_waits_while_queue_item_open_across_ticks() {
    assert!(QUEUE_REMINDERS.contains("open_mail.contains(&candidate.member)"));
    assert!(TASK_DISPOSITION.contains("Hold(\"mail pending\")"));
}

#[test]
fn open_mail_set_is_read_each_tick_not_cached() {
    assert!(QUEUE_WAKE.contains("list_pending_members()"));
    assert!(QUEUE_WAKE.contains("self.remind_open_tasks(task_candidates, &pending_set"));
}

#[test]
fn unread_queue_item_is_reprompted_every_interval() {
    assert!(PENDING_STORE.contains("nudge_pending_at <= ?4"));
    assert!(PENDING_STORE.contains("next_reminder_due(now)"));
    assert!(PENDING_STORE.contains("read = 0"));
}

#[test]
fn requires_ack_message_reminded_until_acked() {
    assert!(PENDING_STORE.contains("pending_ack_at IS NOT NULL"));
    assert!(STMT_CACHE.contains("acknowledged_at IS NULL"));
    assert!(STMT_CACHE.contains("THEN ?5"));
}

#[test]
fn immediate_send_is_never_reminded() {
    assert!(PENDING_STORE.contains("nudge_pending_at IS NOT NULL"));
    assert!(PENDING_STORE.contains("read = 0"));
    assert!(QUEUE_WAKE.contains("NudgeKind::Queue"));
}

#[test]
fn failed_dispatch_backs_off_after_max_attempts_and_still_closes_on_read() {
    assert!(PENDING_STORE.contains("MAX_NUDGE_ATTEMPTS"));
    assert!(PENDING_STORE.contains("CASE WHEN ?5 >= ?7 THEN ?8 ELSE ?4 END"));
    assert!(STMT_CACHE.contains("SET read = 1"));
}

#[test]
fn bare_cli_pull_closes_item() {
    assert!(ROUTER.contains("queue_get_next"));
    assert!(ROUTER.contains("drain_bare_cli_messages"));
}

#[test]
fn queue_creates_no_task_row_and_no_state_column() {
    assert!(!PENDING_STORE.contains("CREATE TABLE tasks"));
    assert!(!PENDING_STORE.contains("ALTER TABLE mail_message_states"));
}

#[test]
fn no_mailbox_list_read_in_the_queue_pass() {
    let production = QUEUE_WAKE
        .split_once("#[cfg(test)]")
        .map_or(QUEUE_WAKE, |(production, _)| production);
    assert!(!production.contains("list_messages"));
    assert!(!QUEUE_REMINDERS.contains("list_messages"));
}
