# BA.5 — `atm queue` as an ephemeral item

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §6, §9 (commit `18db5acc3`) |
| Recommended | arch-ctm / deep-reasoning — changes the at-most-once claim predicate the whole nudge path relies on |
| Depends on | `must_follow` BA.3 (dev push) |
| `parallel_safe` | BA.4 — this sprint owns `crates/atm-storage-rusqlite/src/pending_nudge_store.rs`, `writer/stmt_cache.rs::mark_message_read` (`:44-53`), the `PendingNudgeStore` trait block (`contract.rs:1290-1320`), `crates/atm-core/src/nudge_dispatch.rs`, `herdr_queue_wake.rs::complete_successful_claim`, the test adapters at `storage_and_nudge_router.rs:1240-1245`, `writer/ops.rs::execute_read_display_state` (`:264-284`), and the handoff-helper rename sites (`atm-daemon-bootstrap/src/{lib.rs:1385, queue_drain.rs:417, received_hook_selector.rs:617}`, `atm-core/tests/nudge_mode.rs:132`, `atm-architecture/tests/boundary_enforcement.rs:566-623`). Shared files with BA.4 (`storage_and_nudge_router.rs`, `writer/ops.rs`) are disjoint regions (plan §4). |
| Worktree | `feature/ba5-queue-ephemeral-item` off `integrate/phase-ba` (merge BA.3 forward) |
| Governed interfaces | none (no DDL change; `PendingNudgeStore` is a sealed internal capability) |
| Decisions | plan §2 R3 (the deferred marker is the item; it lives until the item closes) |

## Tasks

1. Add `OPEN_ITEM_SQL` and use it in `claim_next_pending`, `list_pending_members`, `requeue_pending`, `release_pending` — `crates/atm-storage-rusqlite/src/pending_nudge_store.rs` (see "Storage").
2. Make the claim write a one-interval lease and add `nudge_pending_at <= now` to its predicate — `pending_nudge_store.rs:60-66`.
3. Make `requeue_pending` back off to the interval at `MAX_NUDGE_ATTEMPTS` — `pending_nudge_store.rs:94-97`.
4. Rename `clear_pending_on_handoff` → `rearm_pending_after_handoff(member, msg, next_due)`; delete `clear_pending_on_read` — `contract.rs:1290-1320`, `pending_nudge_store.rs:135-148`.
5. Make `mark_message_read` close or re-arm the marker; pass `next_due` from `execute_read_display_state` — `writer/stmt_cache.rs:44-53`, `writer/ops.rs:264-284`.
6. Rename `nudge_dispatch::clear_queue_marker_after_handoff` → `rearm_queue_marker_after_handoff` and every caller — `crates/atm-core/src/nudge_dispatch.rs:30` plus the sites above.
7. Pass the drain's `list_pending_members` result to the task pass as `open_mail`; call BA.3's `complete_task_handoff` for head-task assignment messages — `herdr_queue_wake.rs:739-770` (see "Runtime").
8. Update the ADR-054 inventory gate — `scripts/check-nudge-taxonomy.py:89-90` (ruling: plan §8).
9. Verify the ADR-054 Phase-BA amendment (`docs/adr/ADR-054-…md:292-303`) still matches; edit only on drift.
10. Delete the paths under "Paths to delete"; write the tests under "Tests".

## Phase AZ code used

None. AZ's two-lane selector, `AttentionScheduleStore`, cursors and
reservations are the structure design §9 rejects.

## Storage — exactly as it lands (`pending_nudge_store.rs`)

An `atm queue` message is open while unread, or while an ack is still owed;
closed otherwise. No row, no column, no cursor. The existing deferred marker
`mail_message_states.nudge_pending_at` is the item, read as "next prompt due
at": admission → now; successful handoff → now + 60 s; read (or ack) →
`NULL`. Nothing outside `atm queue` carries the marker, so nothing else is
ever reminded. One predicate, used by every statement below:

```rust
/// A queue item is open while unread, or while an ack is still owed.
/// Columns: mail_message_states (shared_db.rs:38-55).
const OPEN_ITEM_SQL: &str =
    "(read = 0 OR (pending_ack_at IS NOT NULL AND acknowledged_at IS NULL)) AND deleted_at IS NULL";
```

| statement | before (`develop`) | after |
| --- | --- | --- |
| `mark_pending` (`:35-36`) | `SET nudge_pending_at = now, nudge_attempts = 0 … WHERE read = 0 AND deleted_at IS NULL` | unchanged — due now |
| `claim_next_pending` (`:60-66`) | `WHERE … nudge_pending_at IS NOT NULL AND read = 0 AND deleted_at IS NULL AND nudge_attempts < ?max … RETURNING`; `SET nudge_pending_at = NULL` | `WHERE … nudge_pending_at IS NOT NULL AND nudge_pending_at <= ?now AND {OPEN_ITEM_SQL} AND nudge_attempts < ?max`; `SET nudge_pending_at = ?now + TASK_REMINDER_INTERVAL_MS` — a one-interval lease, never `NULL`: a daemon exit between claim and re-arm delays the item by at most one interval and never drops it. At-most-once claiming is unchanged (one conditional `UPDATE … RETURNING`) |
| `requeue_pending` (`:94-97`, failed dispatch) | `SET nudge_pending_at = now, nudge_attempts = attempt + 1` | `SET nudge_pending_at = CASE WHEN ?next_attempt >= ?max THEN ?next_due ELSE ?now END, nudge_attempts = CASE WHEN ?next_attempt >= ?max THEN 0 ELSE ?next_attempt END … AND {OPEN_ITEM_SQL}` — five immediate retries, then one per interval while open. `MAX_NUDGE_ATTEMPTS = 5` (`contract.rs:1253`) keeps its value |
| `release_pending` (`:118-121`) | unchanged | `… AND {OPEN_ITEM_SQL}` |
| `clear_pending_on_handoff` (`:143-148`) | `SET nudge_pending_at = NULL` | **renamed `rearm_pending_after_handoff(member, msg, next_due)`**: `SET nudge_pending_at = ?next_due, updated_at = now WHERE … AND {OPEN_ITEM_SQL}`; a message read between claim and handoff stays `NULL` |
| `clear_pending_on_read` (`:135-140`) | `SET nudge_pending_at = NULL` | **deleted** — no production caller; the read transition is `mark_message_read` |
| `mark_message_read` (`writer/stmt_cache.rs:44-53`, run by `execute_read_display_state`, `writer/ops.rs:264-284`) | `SET read = 1, updated_at = ?4, nudge_pending_at = NULL` | `SET read = 1, updated_at = ?4, nudge_pending_at = CASE WHEN nudge_pending_at IS NOT NULL AND pending_ack_at IS NOT NULL AND acknowledged_at IS NULL THEN ?5 ELSE NULL END`, `?5 = next_due` computed by the writer op — closed on read, or re-armed until the ack. The `IS NOT NULL` guard keeps immediate sends unmarked |
| `list_pending_members` (`:151-158`) | `WHERE nudge_pending_at IS NOT NULL AND read = 0 AND deleted_at IS NULL` | `WHERE nudge_pending_at IS NOT NULL AND {OPEN_ITEM_SQL}` |
| ack (`mark_source_acknowledged`, `writer/ops.rs:529-533` + upsert `stmt_cache.rs:28-39`) | sets `read = 1`, `acknowledged_at`; the upsert sets `nudge_pending_at = NULL` because `excluded.read = 1` | **unchanged** — the item closes on ack with no new code; pinned by a test |

`next_due = now + TASK_REMINDER_INTERVAL_MS` (BA.3 moved the constant to
`atm-storage/src/task_store.rs`). `nudge_attempts` keeps its meaning (failed
dispatches only). There is no terminal threshold for a message: a
Blocked/Offline member is already escalated by BA.3 with zero prompts.

```rust
// crates/atm-storage/src/contract.rs — PendingNudgeStore (sealed), changed method
    /// After a successful handoff the item stays pending with its next due
    /// time; a closed item (read, or acked when required) is not re-armed.
    fn rearm_pending_after_handoff(&self, member: &MemberKey, msg: &AtmMessageId, next_due: IsoTimestamp) -> Result<(), AtmError>;
```

The `pending-nudge-store*.toml` manifests are unchanged (`io_owns` already
names `deferred_nudge_marker_lifecycle`). The helper-name architecture test
(`boundary_enforcement.rs:566-623`) is updated to the new names and keeps
both assertions.

## Runtime changes

| before (BA.3 head) | after |
| --- | --- |
| tick: drain pending claims → `dispose` per member | unchanged order (messages before tasks, design §9); the drain's `list_pending_members` result is passed on as `open_mail: &HashSet<MemberKey>`; a member in it is `Hold("mail pending")` until its last queue item closes |
| `complete_successful_claim` → `clear_queue_marker_after_handoff` (`herdr_queue_wake.rs:754`, `nudge_dispatch.rs:30`) | → `rearm_queue_marker_after_handoff(runtime, member, msg, next_due)`; then, when the claimed envelope has a `task_id` equal to the member's head (`position = 1`, `state = assigned`), BA.3's `complete_task_handoff` — the assignment message is the task's first nudge and its Start |
| bare-CLI `queue_get_next` (`storage_and_nudge_router.rs:698`) and graft receivers | unchanged; the pull reads the message, the read closes the item |
| `atm queue` = `NudgeMode::Deferred` (`commands/queue.rs`) | unchanged |

Nothing marks a queue message `active`; nothing creates a task row for it;
no `list_messages` call is added.

## Paths to delete

- `clear_pending_on_handoff` (trait, impl, router/bootstrap adapters `storage_and_nudge_router.rs:1240`) — renamed
- `clear_pending_on_read` (trait, impl `:135-140`, test adapters) — deleted
- `nudge_dispatch::clear_queue_marker_after_handoff` — renamed
- the untyped `prompted_by_drain` skip in the task pass → `open_mail`
- the `clear_pending_on_handoff` arm of the boundary test's forbidden-call visitor (`boundary_enforcement.rs:622`) — renamed

## Tests

Storage — `pending_nudge_store.rs` tests (existing module):

- `claim_skips_items_not_yet_due` — `now + 30 s` → `None`; `now` → claimed; a second claim within the lease → `None`.
- `restart_after_claim_reexposes_unread_item` — claim, drop the runtime without re-arm, advance one interval → `list_pending_members` lists the member and the same item is claimed; `nudge_attempts` unchanged.
- `rearm_after_handoff_sets_next_due_and_keeps_attempts`.
- `rearm_after_handoff_is_noop_when_read_meanwhile` — marker stays `NULL`.
- `mark_message_read_closes_item_without_ack_requirement` — through `execute_read_display_state`.
- `mark_message_read_rearms_item_when_ack_owed` — marker equals `next_due`.
- `ack_clears_marker_via_existing_upsert` — `read = 1`, `acknowledged_at` set, marker `NULL`; grep gate: 0 occurrences of `nudge_pending_at` in the `writer/ops.rs` ack region.
- `mark_message_read_leaves_unmarked_message_null`.
- `requeue_and_release_keep_closed_items_closed` — closed by read and by ack; an unread marker with an owed ack stays eligible.
- `list_pending_members_includes_read_but_unacked_member`.
- `requeue_at_max_attempts_backs_off_to_interval_and_resets` — 5th failure → `now + 60 s`, `nudge_attempts == 0`.
- `claim_selects_read_but_unacked_item`.
- `immediate_send_never_carries_marker` — 20 claims → `None`.

Runtime — `crates/atm-http-runtime/tests/herdr_queue_ephemeral.rs` (new):

This file is spliced into `herdr_queue_wake`'s test module with `#[path]`; it
is not a standalone Cargo integration-test target.

- `task_prompt_waits_while_queue_item_open_across_ticks` — item prompted at t0; ticks at +5 s, +10 s, +55 s → 0 task prompts; read at +58 s; tick at +60 s → task prompt.
- `open_mail_set_is_read_each_tick_not_cached`.
- `unread_queue_item_is_reprompted_every_interval` — t, t+60, t+120; `atm read` at t+130 → silence for 100 ticks.
- `requires_ack_message_reminded_until_acked` — read, then a fresh tick rediscovers the member and reminds; ack → marker `NULL`, no further prompt.
- `handoff_started_task_closed_without_read_acknowledges_assignment` — close via `atm task close` with no read/ack → `complete`, assignment `acknowledged_at` set, marker `NULL`, zero later nudges.
- `immediate_send_is_never_reminded` — plain `atm send`, never read → 0 queue prompts over 200 ticks. **The R3 boundary.**
- `failed_dispatch_backs_off_after_max_attempts_and_still_closes_on_read`.
- `mail_and_task_share_one_prompt_per_tick` — mail first.
- `blocked_member_with_open_item_escalates_not_reminded`.
- `bare_cli_pull_closes_item` — FIFO member: `queue_get_next` → marker `NULL`, no re-arm.
- `deferred_assignment_handoff_starts_head_task_once` — task `active`, `reminder_count 1`, one receipt; a non-head task does not start; roster Active 20 ticks → nothing further.
- `queue_creates_no_task_row_and_no_state_column` — `PRAGMA table_info(mail_message_states)` equals develop's; `tasks` count unchanged.
- `no_mailbox_list_read_in_the_queue_pass` — 50 ticks → 0 `list_messages` calls.

## Acceptance criteria

1. `mail_message_states` column list is unchanged (asserted by test).
2. Every test above exists by name and passes under `just test`.
3. `grep -rn "clear_pending_on_handoff\|clear_queue_marker_after_handoff\|clear_pending_on_read" crates/ scripts/` → nothing, and `just lint nudge-taxonomy` passes.
4. `grep -rn "lane\|cursor\|reservation" crates/atm-http-runtime/src/herdr_*` → nothing new.
5. `grep -rn "list_messages" crates/atm-http-runtime/src/herdr_*` → no new call.
6. ADR-054's Phase-BA amendment matches the shipped due-at lifecycle.

## Required validation

`just lint`, `just test`, RULE-003, `just lint boundaries`.
