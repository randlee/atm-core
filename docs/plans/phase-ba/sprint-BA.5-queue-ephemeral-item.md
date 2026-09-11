# BA.5 — `atm queue` as an ephemeral item

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §6, §9 (commit `9b5c7d876`) |
| Outcomes | B13 |
| Recommended | arch-ctm / deep-reasoning — changes the at-most-once claim predicate the whole nudge path relies on |
| Depends on | `must_follow` BA.3 (dev push) |
| `parallel_safe` | BA.4 — this sprint owns `crates/atm-storage-rusqlite/src/pending_nudge_store.rs`, `crates/atm-storage-rusqlite/src/writer/stmt_cache.rs::mark_message_read` (`:44-53`, the canonical read transition — FNX-BA-CRIT-030), the `PendingNudgeStore` trait block in `crates/atm-storage/src/contract.rs:1290-1320`, `crates/atm-core/src/nudge_dispatch.rs`, `herdr_queue_wake.rs::complete_successful_claim`, and every rename site of the handoff helper: `crates/atm-daemon-bootstrap/src/{lib.rs:1385, queue_drain.rs:417, received_hook_selector.rs:617}`, `crates/atm-core/tests/nudge_mode.rs:132`, `crates/atm-architecture/tests/boundary_enforcement.rs:566-623`. **Shared file with BA.4:** `storage_and_nudge_router.rs` — this sprint renames only the test-fixture adapter method inside `mod tests` (`:1240-1245`); BA.4 owns the `TaskMove` arm of `dispatch_non_write` (`:484`). Disjoint regions; the merge is mechanical (PLAN-SCOPE-008). Every other file above is BA.5-only. |
| Worktree | `feature/ba5-queue-ephemeral-item` off `integrate/phase-ba` (merge BA.3 forward) |
| Governed interfaces | none (no DDL change; `PendingNudgeStore` is a sealed internal capability) |
| Decisions | plan §4 R3 (the deferred marker is the item; it lives until the item closes) |

## Scope

An `atm queue` message is a scheduling view over the message (design §9):
open while unread — or while unacknowledged when `requires_ack` — closed
otherwise, prompted before the member's task, never `active`. No row, no
column, no cursor, no second record. **The existing deferred marker
(`mail_message_states.nudge_pending_at`) is the item.** Today it is set at
admission (`pending_nudge_store.rs:35-36`) and cleared on the first
successful handoff (`clear_pending_on_handoff`, `:143-148`, called from
`nudge_dispatch::clear_queue_marker_after_handoff`, `:30`), after which an
undelivered-but-prompted queue item is indistinguishable from an
immediately-sent message — the gap plan §4 R3 records. This sprint keeps
the marker set until the item **closes**, and reads it as *"next prompt due
at"*: admission → due now; successful handoff → due now + 60 s; read (or
ack) → `NULL`. The claim that already exists is the reminder. Nothing
outside `atm queue` ever carries the marker, so nothing else is ever
reminded (FNX-BA-CRIT-017: no widening to "any open message").

## Deliverables

| id | deliverable | where |
| --- | --- | --- |
| D1 | `OPEN_ITEM_SQL` predicate used by `claim_next_pending` (eligibility adds `nudge_pending_at <= now`) **and** `list_pending_members` (FNX-BA-CRIT-031); `requeue_pending` backs off to the interval at `MAX_NUDGE_ATTEMPTS` instead of stopping (FNX-BA-CRIT-033); `rearm_pending_after_handoff` (replaces `clear_pending_on_handoff`); `clear_pending_on_read` deleted | `crates/atm-storage-rusqlite/src/pending_nudge_store.rs` |
| D1a | `mark_message_read` — the production read transition — closes or re-arms the marker | `crates/atm-storage-rusqlite/src/writer/stmt_cache.rs:44-53` |
| D2 | `PendingNudgeStore` trait: `clear_pending_on_handoff` → `rearm_pending_after_handoff(member, msg, next_due)`; `clear_pending_on_read` removed (no production caller on develop: only `contract.rs:1674`, router `:1237` and `received_hook_selector.rs:978`, all `#[cfg(test)]`); doc comments state the due-at semantics | `crates/atm-storage/src/contract.rs:1290-1320` |
| D3 | `nudge_dispatch::rearm_queue_marker_after_handoff` (renamed) computes `next_due = now + TASK_REMINDER_INTERVAL_MS`; every caller and the name-pinning boundary test renamed | `crates/atm-core/src/nudge_dispatch.rs:30`; callers `herdr_queue_wake.rs:754`, `queue_drain.rs:417`, `received_hook_selector.rs:617`; adapter `storage_and_nudge_router.rs:1240`; tests `boundary_enforcement.rs:566-623`, `nudge_mode.rs:132`, bootstrap `lib.rs:1385` |
| D4 | `complete_successful_claim` calls D3; `HoldReason::MailPending` for a member the drain prompted this tick | `herdr_queue_wake.rs:739-770`, `herdr_task_disposition.rs` |
| D5 | Verify the committed ADR-054 Phase-BA amendment (`docs/adr/ADR-054-nudge-taxonomy-and-queue-mechanism.md:292-303`, already rewritten in this docs PR to the due-at lifecycle) still matches the shipped statements; edit only on drift (ATM-QA-002) | `docs/adr/ADR-054-…md` Phase-BA amendment |
| D6 | tests named below | `pending_nudge_store.rs` tests, `tests/herdr_queue_ephemeral.rs` |
| D7 | ADR-054 frozen-inventory gate: in `ALLOWED_NUDGE_IDENTIFIERS` replace `clear_pending_on_handoff` with `rearm_pending_after_handoff` and delete `clear_pending_on_read` (`:89-90`); add `rearm_queue_marker_after_handoff` only if `just lint nudge-taxonomy` flags it. Rename-only, no new nudge kind — ruling: plan §10 (RBQA-F004) | `scripts/check-nudge-taxonomy.py:89-90` |

## Phase AZ code used

None. AZ's two-lane selector, `AttentionScheduleStore`, cursors and
reservations are the structure design §9 rejects. The
`received_hook_selector.rs` on AZ is not consulted.

## Storage — exactly as it lands (`crates/atm-storage-rusqlite/src/pending_nudge_store.rs`)

One predicate, used by every statement below (design §9: "the message's
own unread/pending-ack state IS the state"):

```rust
/// A queue item is open while unread, or while an ack is still owed.
/// Columns: mail_message_states (shared_db.rs:38-55).
const OPEN_ITEM_SQL: &str =
    "(read = 0 OR (pending_ack_at IS NOT NULL AND acknowledged_at IS NULL)) AND deleted_at IS NULL";
```

| statement | before (`develop`) | after |
| --- | --- | --- |
| `mark_pending` (`:35-36`) | `SET nudge_pending_at = now, nudge_attempts = 0 … WHERE read = 0 AND deleted_at IS NULL` | unchanged — due now |
| `claim_next_pending` (`:60-66`) | `WHERE … nudge_pending_at IS NOT NULL AND read = 0 AND deleted_at IS NULL AND nudge_attempts < ?max … RETURNING` and `SET nudge_pending_at = NULL` | `WHERE … nudge_pending_at IS NOT NULL AND nudge_pending_at <= ?now AND {OPEN_ITEM_SQL} AND nudge_attempts < ?max`; still `SET nudge_pending_at = NULL` while the prompt is in flight (the at-most-once mechanism is unchanged: one conditional `UPDATE … RETURNING`) |
| `requeue_pending` (`:94-97`, failed dispatch) | `SET nudge_pending_at = now, nudge_attempts = attempt + 1` | `SET nudge_pending_at = CASE WHEN ?next_attempt >= ?max THEN ?next_due ELSE ?now END, nudge_attempts = CASE WHEN ?next_attempt >= ?max THEN 0 ELSE ?next_attempt END` — five immediate retries, then one retry per interval for as long as the item is open; an open item never leaves the selectable set (design §9; FNX-BA-CRIT-033). `MAX_NUDGE_ATTEMPTS = 5` (`contract.rs:1238`) keeps its value and its role as the burst bound |
| `release_pending` (`:118-121`, refused for lifecycle) | unchanged | unchanged |
| `clear_pending_on_handoff` (`:143-148`) | `SET nudge_pending_at = NULL` | **renamed `rearm_pending_after_handoff(member, msg, next_due)`**: `SET nudge_pending_at = ?next_due, updated_at = now WHERE … AND {OPEN_ITEM_SQL}` — a message read between claim and handoff stays `NULL` |
| `clear_pending_on_read` (`:135-140`) | `SET nudge_pending_at = NULL` | **deleted** — it has no production caller; the read transition is `mark_message_read` below (FNX-BA-CRIT-030) |
| `mark_message_read` (`writer/stmt_cache.rs:44-53`, run by `execute_read_display_state`, `writer/ops.rs:264-284`, for every `atm read` / pull) | `SET read = 1, updated_at = ?4, nudge_pending_at = NULL` | `SET read = 1, updated_at = ?4, nudge_pending_at = CASE WHEN nudge_pending_at IS NOT NULL AND pending_ack_at IS NOT NULL AND acknowledged_at IS NULL THEN ?5 ELSE NULL END` with `?5 = next_due` computed by the writer op from `TASK_REMINDER_INTERVAL_MS` (atm-storage) — closed-on-read, or re-armed until the ack (design §9). The `nudge_pending_at IS NOT NULL` guard keeps immediate sends unmarked |
| `list_pending_members` (`:151-158`) | `WHERE nudge_pending_at IS NOT NULL AND read = 0 AND deleted_at IS NULL` | `WHERE nudge_pending_at IS NOT NULL AND {OPEN_ITEM_SQL}` — a read-but-unacked member is discovered by the pump on a fresh tick (FNX-BA-CRIT-031) |
| ack write (`writer/ops.rs::mark_source_acknowledged`, `:528-532`, persisted by the `stmt_cache.rs:33` upsert) | does not touch the marker | **unchanged.** The ack sets `acknowledged_at`; `{OPEN_ITEM_SQL}` in the claim then excludes the item. A stale non-NULL `nudge_pending_at` on a closed item is inert (never claimable) and is not cleaned up — no ack-writer edit, no shared file with BA.4 |

`next_due = now + TASK_REMINDER_INTERVAL_MS` (60 s, design §6 — the one
rate limit for messages and tasks; BA.3 moves the constant to
`atm-storage/src/task_store.rs` so atm-core, atm-storage-rusqlite and the
runtime import the same value — FNX-BA-CRIT-032). `nudge_attempts` keeps its
meaning (failed dispatches only); a successfully prompted item that is
simply not read is re-armed indefinitely at the interval, exactly like a
task reminder, until it closes; a failing dispatch retries five times at
once and then once per interval. There is **no** terminal threshold for a
message: nothing about an unread message is "stalled" in the design, and a
Blocked/Offline member is already escalated by BA.3 with zero prompts.

```rust
// crates/atm-storage/src/contract.rs — PendingNudgeStore (sealed), changed method
    /// After a successful handoff the item stays pending with its next due
    /// time; a closed item (read, or acked when required) is not re-armed.
    fn rearm_pending_after_handoff(&self, member: &MemberKey, msg: &AtmMessageId, next_due: IsoTimestamp) -> Result<(), AtmError>;
```

The boundary manifests (`boundaries/atm-storage/pending-nudge-store.toml`,
`boundaries/atm-storage-rusqlite/pending-nudge-store-sqlite.toml`) are
unchanged: `io_owns = ["deferred_nudge_marker_lifecycle", …]` already names
this lifecycle. The architecture test that pins the helper name
(`crates/atm-architecture/tests/boundary_enforcement.rs:566-623` — exactly one
workspace definition of the helper; no direct store call outside it) is
updated to the new names and keeps both assertions.

## Runtime changes

| before (BA.3 head) | after |
| --- | --- |
| tick: drain pending claims (messages) → `dispose` per member for tasks | unchanged order — messages are discharged before tasks (design §9); a member the drain prompted this tick is `Hold(MailPending)` for tasks |
| `complete_successful_claim` → `clear_queue_marker_after_handoff` (`herdr_queue_wake.rs:754`, `nudge_dispatch.rs:30`) | → `rearm_queue_marker_after_handoff(runtime, member, msg, next_due)` |
| bare-CLI `queue_get_next` (`storage_and_nudge_router.rs:698`) and graft receivers | unchanged code; the pull reads the message, the read closes the item |
| `atm queue` = `NudgeMode::Deferred` (`commands/queue.rs`) | unchanged |

Nothing marks a queue message `active`; nothing creates a task row for it;
no `list_messages` call is added anywhere (the open predicate is SQL on
`mail_message_states` — FNX-BA-CRIT-018 does not arise).

## Paths to delete

- `clear_pending_on_handoff` (trait + impl + the router/bootstrap adapters
  `storage_and_nudge_router.rs:1240`) — renamed, not kept alongside
- `clear_pending_on_read` (trait `contract.rs`, impl `pending_nudge_store.rs:135-140`,
  test adapters) — deleted; no production caller on develop
- `nudge_dispatch::clear_queue_marker_after_handoff` — renamed
- the untyped `prompted_by_drain` skip in the task pass → `Hold(MailPending)`
- the `clear_pending_on_handoff` arm of the boundary test's forbidden-call visitor (`boundary_enforcement.rs:622`) → renamed, not duplicated

## Tests

Storage — `pending_nudge_store.rs` tests (existing module):

- `claim_skips_items_not_yet_due` — `nudge_pending_at = now + 30 s` → `None`;
  `now` → claimed; `now − 1 ms` → claimed.
- `rearm_after_handoff_sets_next_due_and_keeps_attempts` — attempts stay
  0; `nudge_pending_at == next_due` byte-equal.
- `rearm_after_handoff_is_noop_when_read_meanwhile` — mark read between
  claim and re-arm → marker stays `NULL`.
- `mark_message_read_closes_item_without_ack_requirement` — through
  `execute_read_display_state`, the real read path.
- `mark_message_read_rearms_item_when_ack_owed` — `pending_ack_at` set, not
  acked → `nudge_pending_at == next_due`; after `acknowledged_at` is set the
  item is never claimed again (marker value irrelevant).
- `mark_message_read_leaves_unmarked_message_null` — an immediate send read
  → `NULL` before and after.
- `list_pending_members_includes_read_but_unacked_member` — `read = 1`,
  `pending_ack_at` set, marker due → member listed (FNX-BA-CRIT-031).
- `requeue_at_max_attempts_backs_off_to_interval_and_resets` — 5th failure
  → `nudge_pending_at == now + 60 s`, `nudge_attempts == 0`.
- `claim_selects_read_but_unacked_item` — `read = 1`, `pending_ack_at` set,
  `acknowledged_at NULL`, due → claimed (the old `read = 0` predicate would
  have skipped it).
- `immediate_send_never_carries_marker` — an `Immediate` write leaves
  `nudge_pending_at NULL` at every point; 20 claims → `None`.
- `mark_pending_is_unchanged_from_develop` — pins the statement text.

Runtime — `crates/atm-http-runtime/tests/herdr_queue_ephemeral.rs` (new):

- `queued_message_is_prompted_before_task_for_idle_member` — assign task,
  `atm queue` a message; ticks → first prompt is the mail, task prompt only
  after the message is read (`Hold(MailPending)` observed in the log).
- `unread_queue_item_is_reprompted_every_interval` — prompts at t, t+60,
  t+120; `atm read` at t+130 → silence for 100 more ticks.
- `read_message_is_never_reminded` — `atm read` before the first interval
  → 0 further prompts.
- `requires_ack_message_reminded_until_acked` — read (real `atm read`), then
  a **fresh** tick with an empty in-memory candidate set → the pump
  rediscovers the member and reminds at the interval; `atm ack` → silence.
- `immediate_send_is_never_reminded` — plain `atm send` to an Idle member,
  never read → 0 queue prompts over 200 ticks (only the one immediate
  nudge at send time). **This is the R3 boundary.**
- `failed_dispatch_backs_off_after_max_attempts_and_still_closes_on_read` —
  force 5 failed dispatches → no claim until t+60 s, then one claim per
  interval; `atm read` → no further claims (FNX-BA-CRIT-033).
- `mail_and_task_share_one_prompt_per_tick` — open item + due task → one
  prompt per tick per member, mail first.
- `blocked_member_with_open_item_escalates_not_reminded`.
- `bare_cli_pull_closes_item` — FIFO member: `queue_get_next` → read →
  marker `NULL`; no re-arm.
- `queue_creates_no_task_row_and_no_state_column` — `PRAGMA table_info(mail_message_states)`
  column set equals develop's; `tasks` count unchanged.
- `no_mailbox_list_read_in_the_queue_pass` — count `list_messages` calls
  over 50 ticks → 0.

## Acceptance criteria

1. Schema of `mail_message_states` is unchanged (column list asserted).
2. All tests above pass.
3. `grep -rn "clear_pending_on_handoff\|clear_queue_marker_after_handoff\|clear_pending_on_read" crates/ scripts/` → nothing, and `just lint nudge-taxonomy` passes.
4. `grep -rn "lane\|cursor\|reservation" crates/atm-http-runtime/src/herdr_*` → nothing new.
5. `grep -rn "list_messages" crates/atm-http-runtime/src/herdr_*` → no new call.
6. ADR-054's Phase-BA amendment describes the due-at marker lifecycle.

## Required validation

`just lint`, `just test`, RULE-003, `just lint-boundaries`.

## Out of scope

A persisted `delivery_mode` column (R3 alternative, design-excluded);
reminding any message that never carried the marker; changes to `atm queue`
CLI flags; a stalled threshold for messages.
