//! Bounded, daemon-lifetime FIFO storage for bare-CLI queue pulls.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use atm_core::boundary::{MemberKey, NudgeKind};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::QueuedNudgeMessage;

/// Maximum number of undelivered bare-CLI messages retained per member.
pub const BARE_CLI_FIFO_CAPACITY: usize = 32;

/// Composition-root-owned FIFO map. It intentionally has no persistence or
/// claim state: a daemon restart drops stale pull notifications while the
/// durable mailbox remains authoritative.
pub type BareCliFifo = Arc<Mutex<HashMap<MemberKey, VecDeque<QueuedNudgeMessage>>>>;

/// Cumulative drop count for FIFO overflow, kept separate from runtime health.
pub type BareCliQueueFullDrops = Arc<AtomicU64>;

fn lock_fifo(
    fifo: &BareCliFifo,
) -> Result<MutexGuard<'_, HashMap<MemberKey, VecDeque<QueuedNudgeMessage>>>, AtmError> {
    fifo.lock().map_err(|_| {
        AtmError::new(
            AtmErrorCode::InternalError,
            "bare-CLI queue FIFO lock is poisoned",
        )
    })
}

/// Append one handed-off message, dropping the oldest item at capacity.
pub fn append_bare_cli_message(
    fifo: &BareCliFifo,
    drops: &BareCliQueueFullDrops,
    member: MemberKey,
    message: QueuedNudgeMessage,
) -> Result<(), AtmError> {
    let mut queues = lock_fifo(fifo)?;
    let queue = queues.entry(member).or_default();
    if queue.len() >= BARE_CLI_FIFO_CAPACITY {
        queue.pop_front();
        drops.fetch_add(1, Ordering::Relaxed);
    }
    queue.push_back(message);
    Ok(())
}

/// Drain all steer messages and at most the oldest queue message for a member.
pub fn drain_bare_cli_messages(
    fifo: &BareCliFifo,
    member: &MemberKey,
) -> Result<Vec<QueuedNudgeMessage>, AtmError> {
    let mut queues = lock_fifo(fifo)?;
    let Some(mut queue) = queues.remove(member) else {
        return Ok(Vec::new());
    };
    let mut messages = Vec::with_capacity(queue.len());
    let mut queue_taken = false;
    let mut retained = VecDeque::new();
    while let Some(message) = queue.pop_front() {
        if message.kind == NudgeKind::Steer || !queue_taken {
            queue_taken |= message.kind == NudgeKind::Queue;
            messages.push(message);
        } else {
            retained.push_back(message);
        }
    }
    if !retained.is_empty() {
        queues.insert(member.clone(), retained);
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::{
        BARE_CLI_FIFO_CAPACITY, BareCliFifo, BareCliQueueFullDrops, append_bare_cli_message,
        drain_bare_cli_messages,
    };
    use atm_core::boundary::{MemberKey, NudgeKind};
    use atm_core::protocol::QueuedNudgeMessage;
    use atm_core::schema::AtmMessageId;
    use atm_core::types::{AgentName, TeamName};
    use std::sync::atomic::Ordering;

    fn member() -> MemberKey {
        MemberKey::new(
            TeamName::from_validated("test-team"),
            AgentName::from_validated("test-agent"),
        )
    }

    fn message(kind: NudgeKind, digit: char) -> QueuedNudgeMessage {
        QueuedNudgeMessage {
            kind,
            msg_id: AtmMessageId::new(),
            body: digit.to_string(),
        }
    }

    fn fifo() -> (BareCliFifo, BareCliQueueFullDrops) {
        (Default::default(), Default::default())
    }

    #[test]
    fn queue_items_drain_oldest_one_at_a_time() {
        let (fifo, drops) = fifo();
        let key = member();
        append_bare_cli_message(&fifo, &drops, key.clone(), message(NudgeKind::Queue, 'a'))
            .expect("append");
        append_bare_cli_message(&fifo, &drops, key.clone(), message(NudgeKind::Queue, 'b'))
            .expect("append");
        assert_eq!(
            drain_bare_cli_messages(&fifo, &key).expect("drain").len(),
            1
        );
        assert_eq!(
            drain_bare_cli_messages(&fifo, &key).expect("drain").len(),
            1
        );
        assert!(
            drain_bare_cli_messages(&fifo, &key)
                .expect("drain")
                .is_empty()
        );
    }

    #[test]
    fn steer_items_all_drain_with_one_queue_item() {
        let (fifo, drops) = fifo();
        let key = member();
        for (kind, digit) in [
            (NudgeKind::Steer, 'a'),
            (NudgeKind::Queue, 'b'),
            (NudgeKind::Steer, 'c'),
            (NudgeKind::Queue, 'd'),
        ] {
            append_bare_cli_message(&fifo, &drops, key.clone(), message(kind, digit))
                .expect("append");
        }
        let drained = drain_bare_cli_messages(&fifo, &key).expect("drain");
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].body, "a");
        assert_eq!(drained[1].body, "b");
        assert_eq!(drained[2].body, "c");
        assert_eq!(
            drain_bare_cli_messages(&fifo, &key).expect("drain")[0].body,
            "d"
        );
    }

    #[test]
    fn overflow_drops_oldest_and_counts_the_drop() {
        let (fifo, drops) = fifo();
        let key = member();
        for digit in 0..=BARE_CLI_FIFO_CAPACITY {
            append_bare_cli_message(
                &fifo,
                &drops,
                key.clone(),
                message(
                    NudgeKind::Queue,
                    char::from_digit(digit as u32, 36).expect("digit"),
                ),
            )
            .expect("append");
        }
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert_eq!(
            drain_bare_cli_messages(&fifo, &key).expect("drain").len(),
            1
        );
    }
}
