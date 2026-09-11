# BA.6 — Ephemeral queued messages

| Field | Value |
| --- | --- |
| Wave | 4 |
| Branch | `feature/ba6-ephemeral-queued-messages` |
| Base | `integrate/phase-ba` — **not a stack layer** (PLAN-SCOPE-011). Its files are disjoint from BA.5's command surface; it needs BA.5's merged close semantics, which the PR-completion trigger delivers. |
| Dependency | `must_follow` BA.5, `must_follow` BA.4 (PR-completion). **BA.8 now follows THIS sprint**, not the reverse (PLAN-CRIT-012). |
| recommended_agent | arch-ctm |
| recommended_model | deep-reasoning |

## Goal

One scheduler over one kind of thing. A queued message is scheduled like a
task, so the deferred path is a first-class citizen rather than a second lane.

Removing `atm queue` was considered and **rejected**: it is the only existing
mechanism that delivers without interrupting — the cure for problem (b).

## Deliverables

### D1. A scheduling view — but the current message fields cannot carry it

The intent stands: an ephemeral item is a **scheduling view over the message**.
No task row, no task state, no `task_events` entry. Anything else recreates the
two-authorities problem this phase exists to delete, one layer down.

**SOLAR-BA-007 (BLOCKING) shows the design's stated basis for that view is
wrong, and the item leaks.** Verified on `origin/develop`:

- a successful queue handoff clears `nudge_pending_at`
  (`herdr_queue_wake.rs:739-758`, `nudge_dispatch.rs:23-72`) but does **not**
  mark the message read
- queue origin/mode is not persisted anywhere in `mail_messages` /
  `mail_message_states`; `nudge_pending_at` is the *only* discriminator
- so after handoff the row is still open by the close-on-read rule, yet is no
  longer selectable and no longer distinguishable from an ordinary unread
  immediate message
- if the agent receives the hook and dies before `atm read`, **the item is
  silently lost**
- reading a `requires_ack` message produces Read + PendingAck
  (`read/state.rs:37-49, 117-123`), but the read write unconditionally clears
  `nudge_pending_at` and the pending-claim query requires `read = 0`
  (`writer/stmt_cache.rs:44-53`, `pending_nudge_store.rs:51-83`) — so the item
  is still open by D2's rule yet has vanished from scheduling

Required: the item's durability is defined by its **actual close condition**,
not by `nudge_pending_at`. Deferred-vs-immediate origin must survive handoff,
and a handed-off-but-undischarged item must remain selectable. This can stay as
columns or state **on the existing message row** — that constraint holds — but
the current unread / pending-ack fields alone are insufficient and the sprint
must add what is missing rather than assert the view already works.

**The columns, named (PLAN-CRIT-004, PLAN-SCOPE-012).** ADR-054 says
downstream sprints *implement* its marker/`read = 0` contract rather than
redefine it, and this deliverable changes what stays claimable after handoff.
"State on the message row" is not a reviewable specification, and an earlier
revision demanded the names in an acceptance criterion without supplying
them. Two columns are added to `mail_message_states`
(`shared_db.rs:38-55`, via the existing `ALTER TABLE ... ADD COLUMN` pattern
at `shared_db.rs:817`; timestamps in that table are `TEXT`):

| column | type | default | written by | cleared by |
| --- | --- | --- | --- | --- |
| `delivery_mode` | `TEXT NOT NULL CHECK(delivery_mode IN ('immediate','deferred'))` | `'immediate'` | message admission only, once, from the send's `NudgeMode` | never — it is the message's origin, not its progress |
| `handed_off_at` | `TEXT NULL` | `NULL` | the queue-handoff writer, in the **same transaction** that clears `nudge_pending_at` (`herdr_queue_wake.rs:739-758`, `nudge_dispatch.rs:23-72`) | never — handoff is a fact, not a claim |

Both are backfilled by the migration as `'immediate'` / `NULL`, which is
exactly the behaviour of every message written before this sprint.

What the two columns buy, one defect each from SOLAR-BA-007:

- `delivery_mode` survives handoff, so a deferred item stays distinguishable
  from an ordinary unread immediate message after `nudge_pending_at` is
  cleared. Today `nudge_pending_at` is the only discriminator and it is
  destroyed by the very transition that needs to be remembered.
- `handed_off_at` makes "handed off but not discharged" representable, so the
  item stays **selectable** and the post-handoff cadence below has something
  to key off. Today an agent that takes the hook and dies loses the item
  silently.

**The selection predicate stops reading `nudge_pending_at`.** An item is
selectable when it is open by D2's close rule (`read = 0`, or
`pending_ack_at IS NOT NULL AND acknowledged_at IS NULL` when
`requires_ack`) and `delivery_mode = 'deferred'` and `deleted_at IS NULL` —
independent of `nudge_pending_at` and of `handed_off_at`. That is the whole
of SOLAR-BA-007's first half: durability is defined by the actual close
condition, not by the nudge marker.

`nudge_pending_at` and `nudge_attempts` keep their current meaning — an
in-flight nudge claim and its retry count — and are not repurposed. No third
column, no new table, no new state machine.

BA.11 carries the matching ADR-054 amendment and copies these names from the
**shipped** schema, not from this table.

Also required: a **post-handoff reminder cadence**. Handoff is not discharge,
so an item that was handed off and never read must come back.

### D2. Close trigger

Determined by the existing `requires_ack` flag, not a new concept:

```
requires_ack ? closed-on-ack : closed-on-read
```

### D2a. Pending acknowledgement must not starve the task invariant

**SOLAR-BA-007, second half.** D4 discharges messages before the next task, and
a `requires_ack` message closes only on ack. Together, one read-but-never-acked
message suppresses task selection **forever** while an incomplete assigned task
sits there — which is precisely "the agent stops for no acceptable reason",
problem (a), rebuilt out of problem (b)'s fix.

**PLAN-CRIT-013: "messages always first" and "messages can never starve a
task" are not simultaneously satisfiable, and delegating the bound to the
implementer let two incompatible builds both claim compliance.** The rule is
normative here:

  1. A queued message is offered **before** the next task — D4's intent —
     but each message is offered **at most once** per scheduling pass.
  2. A message that has been offered and not discharged does **not** block the
     pass again. It stays remindable on its own cadence (D1) but loses its
     precedence.
  3. At most **one** message discharge may precede a task selection in a
     single pass. Continuous arrivals therefore cannot stack.
  4. A read-but-unacked `requires_ack` message never blocks task selection at
     all. It is remindable, not blocking.

Together these give D4 its purpose — a queued message is seen before the agent
starts the next task — without letting the mailbox hold the task queue
hostage. AC6 asserts each of the four, not "eventually".

### D3. One ordered list, which is a sort

The selector's queue is one ordered list drawn from two sources — open tasks
and undischarged messages. That is a **sort**, not a state machine: no lane
cursor, no reservation, no fairness state. This is precisely where complexity
accumulated in phase AZ.

Deletes `AttentionLane`, the lane cursors, and the fairness/cycling logic if
any remnant survives BA.4.

### D4. Ordering: messages before the next task

Discharge queued messages **before** taking the next task. Reading costs
seconds, and a queued message may change what the agent should do next —
Rand's example, `atm queue 'please let solar know when all tasks complete'`,
must be received before the tasks complete to be worth anything.

### D5. Ephemeral items never enter `active`

An ephemeral item goes assigned → closed without entering `active`, so it
never touches `one_active_task_per_agent`. An unread message cannot block task
assignment, and no exemption is needed.

## Affected paths

- `crates/atm-http-runtime/src/herdr_queue_wake.rs` — selection ordering
- `crates/atm-http-runtime/src/herdr_queue_wake_reminders.rs`
- `crates/atm-storage-rusqlite/src/pending_nudge_store.rs`
- `crates/atm-storage-rusqlite/src/writer/stmt_cache.rs`
- `crates/atm-core/src/read/state.rs`
- `crates/atm-core/src/nudge_dispatch.rs`
- `docs/agent-conventions.md` — only if observable behaviour changed from
  what BA.7 documented

## Paths that must not change

- `crates/atm/src/commands/task.rs` — BA.5
- the `tasks` table schema — BA.3

## Acceptance criteria

1. A queued informational message is discharged on read and stops being
   scheduled.
2. A queued `requires_ack` message is **not** discharged by reading; it stays
   visible and remindable until ack. This must fail against `origin/develop`,
   where the read clears `nudge_pending_at`.
3. **Handoff then death before read**: the agent receives the hook and the
   process exits before `atm read`; after restart the item is redelivered.
   This must fail against `origin/develop`.
4. Deferred-vs-immediate origin is still distinguishable after handoff.
5. An agent with both an undischarged message and an assigned task receives
   the message first.
6. **Non-starvation**, asserted as four separate cases, not "eventually":
   (a) a message is offered before the next task; (b) an undischarged message
   is not re-offered in the same pass; (c) with messages arriving on every
   tick, at most one discharge precedes each task selection; (d) a
   read-but-unacked `requires_ack` message never blocks task selection while
   remaining remindable.
6a. The shipped schema adds exactly `delivery_mode` and `handed_off_at` to
   `mail_message_states`, with the types, defaults and write sites named in
   D1, and the migration backfills existing rows to `'immediate'` / `NULL`.
   Gate: reviewer diffs the merged DDL against D1's table.
6b. The selection predicate reads neither `nudge_pending_at` nor
   `handed_off_at`. Gate: a test in which a deferred item is handed off,
   `nudge_pending_at` is cleared, the agent dies without reading, and the
   item is still selected on the next pass. Must fail against today's code.
7. An undischarged message never prevents a task assignment and never occupies
   the one-active slot.
8. No new table and no new state machine is introduced. Gate: the diff adds no
   `CREATE TABLE` and no new trait. Columns on the existing message row are
   permitted and expected — D1 requires them.

## Non-closure

- `atm queue`'s own documentation is BA.7's. This sprint owns only a
  follow-up edit if observable behaviour diverges from what BA.7 described.
- Rand has explicitly dismissed the concern that `atm queue "go fix X"` is an
  untracked task: *"Official planned items will explicitly be
  `atm send --task ...`. An unofficial fix that is queued is a judgement call
  (and unofficial thus not a concern)."* Do not re-raise it as a finding.
