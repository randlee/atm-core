# BA.5 — `atm queue` as an ephemeral item

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §9 (commit `9b5c7d876`) |
| Outcomes | B13 |
| Recommended | arch-ctm / deep-reasoning — touches the BA.3 selection pass |
| Depends on | `must_follow` BA.3 (dev push) |
| `parallel_safe` | BA.4 |
| Worktree | `feature/ba5-queue-ephemeral-item` off `integrate/phase-ba` (merge BA.3 forward) |
| Governed interfaces | none |
| Decisions | plan §4 R3 (any open message is remindable) |

## Scope

An `atm queue` message is a scheduling view over the message: open while
unread (or unacked when `requires_ack`), closed otherwise, selected before
the member's task, never `active`. No row, no column, no cursor. The
existing deferred-nudge drain stays as the first attempt; this sprint adds
the reminder for open messages that the drain has exhausted
(`nudge_attempts >= MAX_NUDGE_ATTEMPTS = 5`, `contract.rs:1238`) and the
ordering rule.

## Phase AZ code used

None. AZ's two-lane selector, `AttentionScheduleStore`, cursors and
reservations are the structure design §9 rejects. The
`received_hook_selector.rs` on AZ is not consulted.

## Types — exactly as they land (`crates/atm-http-runtime/src/herdr_task_disposition.rs`)

```rust
/// What the pump does for one Idle member this tick. Mail before task (design §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attention {
    /// Re-prompt for open mail; the task waits.
    RemindMail { open_messages: u32 },
    Task(TaskDisposition),
}

/// `open_messages` counts the member's open messages per `is_open` below.
pub(crate) fn select_attention(
    state: RuntimeMemberState,
    open_messages: u32,
    head: Option<&TaskRow>,
    now: IsoTimestamp,
    episode_notified: bool,
    last_mail_reminder: Option<IsoTimestamp>,
) -> Attention {
    let task = dispose(state, head, now, episode_notified);
    match (state, open_messages) {
        (RuntimeMemberState::Idle, n) if n > 0 && mail_reminder_due(last_mail_reminder, now) => Attention::RemindMail { open_messages: n },
        (RuntimeMemberState::Idle, n) if n > 0 => Attention::Task(TaskDisposition::Hold(HoldReason::MailPending)),
        _ => Attention::Task(task),
    }
}
```

`HoldReason` gains `MailPending`. Escalation dispositions
(`EscalateStalled`, `EscalateEpisode`) are **not** overridden by open mail —
the match above only intercepts `Idle`; Blocked/Offline fall through to
`dispose`. Rate limit reuses `TASK_REMINDER_INTERVAL_MS` and the pump's
existing `last_task_attempt` map (`herdr_queue_wake.rs:77`), renamed
`last_attention_attempt` — one stamp per member per tick, mail or task.

Open predicate, one SQL, `crates/atm-storage-rusqlite/src/mailbox_reader.rs`
(the `AsyncMailboxReader` implementation) — **no trait change**: the count
is derived by the pump from the existing `list_messages(scope = Unread)`
result filtered in memory:

```rust
/// design §9: open = unread, or unacked when the message requires an ack.
fn is_open(message: &Message) -> bool {
    !message.envelope.read
        || (message.envelope.requires_ack && message.envelope.acknowledged_at.is_none())
}
```

The pump already has a mailbox reader handle for the drain; one
`list_messages` per Idle member per tick is the cost, bounded by
`HERDR_MAX_PROMPTS_PER_TICK` candidates. Non-Idle members are not read.

## Runtime changes

| before (BA.3 head) | after |
| --- | --- |
| tick: drain pending nudges → `dispose` per member | tick: drain pending nudges → for each Idle member not prompted by the drain: `open_messages = count(is_open)` → `select_attention` |
| `RemindMail` has no emitter | reuses the queue nudge emitter with `NudgeKind::Queue` and the existing mail-summary template — ADR-054 inventory unchanged; the prompt names the oldest open message |
| `atm queue` = `NudgeMode::Deferred` (`commands/queue.rs`) | unchanged |
| `clear_pending_on_read` / `clear_pending_on_handoff` (`pending_nudge_store.rs:135-150`) | unchanged — the marker lifecycle is the first attempt; `is_open` is the truth after it |

Nothing marks a queue message `active`; nothing creates a task row for it
(architecture test: `atm queue` fixture run leaves `tasks` row count
unchanged).

## Paths to delete

None. (`MAX_NUDGE_ATTEMPTS` stays: it bounds the *immediate* retry burst;
the reminder above is the slow path.)

## Tests

Pure:

- `select_attention_table` — Idle × open_messages {0,1,5} × mail-reminder
  due {yes,no} × head {none, due, stalled} → expected; plus Blocked/Offline
  with open mail → `EscalateEpisode`, Active with open mail → `Hold(Active)`.
  All rows listed.
- `is_open_predicate` — 4 cases: unread; read+no-ack-required; read+requires
  ack+unacked (open); read+requires ack+acked (closed).

Runtime — `crates/atm-http-runtime/tests/herdr_queue_ephemeral.rs` (new):

- `queued_message_is_prompted_before_task_for_idle_member` — assign task,
  `atm queue` a message; ticks → first prompt is the mail, task prompt only
  after the message is read.
- `read_message_is_never_reminded` — `atm read` then 20 ticks → 0 mail prompts.
- `requires_ack_message_reminded_until_acked` — read but unacked → reminded
  at interval; `atm ack` → silence.
- `exhausted_deferred_nudge_falls_back_to_slow_reminder` — force
  `nudge_attempts = 5`; next interval → one `RemindMail` prompt.
- `mail_and_task_share_one_rate_limit` — open mail + due task: prompts at
  t (mail), t+60 (mail, still unread), read at t+70, task at t+120 — never
  two prompts in one interval for one member.
- `blocked_member_with_open_mail_escalates_not_reminded`.
- `queue_creates_no_task_row_and_no_state_column` — `PRAGMA table_info(mail_message_states)`
  column set equals develop's; `tasks` count unchanged.
- `non_idle_members_incur_no_mailbox_read` — count reader calls with 3
  Active members → 0.

## Acceptance criteria

1. Schema of `mail_message_states` is unchanged (column list asserted).
2. All tests above pass.
3. `grep -rn "lane\|cursor\|reservation" crates/atm-http-runtime/src/herdr_*` → nothing new.

## Required validation

`just lint`, `just test`, RULE-003, `just lint-boundaries`.

## Out of scope

A persisted `delivery_mode` column (R3 alternative); changes to `atm queue`
CLI flags.
