# BA.3 — Nudge invariant and terminal escalation

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §1, §2, §6, §6.1, §6.2 (commit `9b5c7d876`) |
| Outcomes | B1, B2, B11, B12 |
| Recommended | arch-ctm / deep-reasoning — replaces the reminder/escalation loop in a 4k-line runtime module |
| Depends on | `must_follow` BA.2 (dev push) — `open_tasks_for_team`, `TaskOp::Start`, `position` |
| `parallel_safe` | BA.4 — this sprint owns `crates/atm-http-runtime/src/herdr_*` only |
| Worktree | `feature/ba3-nudge-invariant` off `integrate/phase-ba` (merge BA.2 forward) |
| Governed interfaces | none (Herdr IPC unchanged) |
| Decisions | plan §4 R1 (start = handoff), R4 (restart duplicates) |

## Scope

One pure disposition function decides, per member per tick, from exact
`RuntimeMemberState` and that member's head task: nudge, escalate, or hold.
Escalation is terminal at the threshold. Blocked and Offline are one message
per episode with zero nudges. A successful task handoff to an `Idle` member
applies `TaskOp::Start`.

## Phase AZ code used

| AZ artifact | how |
| --- | --- |
| `herdr_attention_scheduler.rs:120-147` `persistent_candidate` / `task_reminder_due` — eligibility computed **only** from an accepted `Idle` observation, never from raw Herdr list results | **pattern kept**: `dispose()` below takes the accepted `RuntimeMemberState`, and `collect_idle_members` already routes through `apply_roster_runtime_observations` (`herdr_queue_wake.rs:478-489`) |

Not used: `IdleOpportunity`, lane cursors, reservations, `RosterStateRevision`
revalidation (design §1 "no edge detection/revision revalidation").

## Types — exactly as they land (`crates/atm-http-runtime/src/herdr_task_disposition.rs`, new, pure, ≤ 150 lines)

```rust
use atm_core::boundary::{RuntimeMemberState, TaskRow, TASK_STALLED_REMINDER_THRESHOLD};
use atm_storage::types::IsoTimestamp;

pub(crate) const TASK_REMINDER_INTERVAL_MS: u64 = 60_000; // moved here from herdr_queue_wake.rs:41

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskDisposition {
    /// Emit the task nudge for the member's head task.
    Nudge,
    /// The head task has reached the threshold: send the stalled message once.
    EscalateStalled,
    /// Member is Blocked / Offline: one message per episode, zero nudges.
    EscalateEpisode(EpisodeKind),
    Hold(HoldReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EpisodeKind { Blocked, Offline }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoldReason {
    NoOpenTask,
    Active,            // never divert
    Unobserved,        // Unknown
    IdentityConflict,
    RateLimited,       // idle, < TASK_REMINDER_INTERVAL_MS since last reminder
    Stalled,           // idle, already escalated once; wait for a state change
    EpisodeNotified,   // blocked/offline, message already sent this episode
}

/// The whole invariant. `head` is the member's lowest-position open task.
/// `episode_notified` is whether this Blocked/Offline episode already produced its message.
pub(crate) fn dispose(
    state: RuntimeMemberState,
    head: Option<&TaskRow>,
    now: IsoTimestamp,
    episode_notified: bool,
) -> TaskDisposition {
    use RuntimeMemberState as S;
    match (state, head) {
        (S::Blocked, _) if episode_notified => TaskDisposition::Hold(HoldReason::EpisodeNotified),
        (S::Blocked, _) => TaskDisposition::EscalateEpisode(EpisodeKind::Blocked),
        (S::Offline, _) if episode_notified => TaskDisposition::Hold(HoldReason::EpisodeNotified),
        (S::Offline, _) => TaskDisposition::EscalateEpisode(EpisodeKind::Offline),
        (S::Active, _) => TaskDisposition::Hold(HoldReason::Active),
        (S::Unknown, _) => TaskDisposition::Hold(HoldReason::Unobserved),
        (S::IdentityConflict, _) => TaskDisposition::Hold(HoldReason::IdentityConflict),
        (S::Idle, None) => TaskDisposition::Hold(HoldReason::NoOpenTask),
        (S::Idle, Some(task)) if task.lead_notified_count > 0 => TaskDisposition::Hold(HoldReason::Stalled),
        (S::Idle, Some(task)) if task.reminder_count >= TASK_STALLED_REMINDER_THRESHOLD => TaskDisposition::EscalateStalled,
        (S::Idle, Some(task)) if !reminder_due(task, now) => TaskDisposition::Hold(HoldReason::RateLimited),
        (S::Idle, Some(_)) => TaskDisposition::Nudge,
    }
}

fn reminder_due(task: &TaskRow, now: IsoTimestamp) -> bool {
    task.last_reminded_at
        .is_none_or(|last| now.millis_since(last) >= TASK_REMINDER_INTERVAL_MS)
}
```

Blocked/Offline are matched before `head` so an episode with no open task
still escalates once (design §1: "blocked/dead → escalate immediately").

Episode state replaces `EscalationState` (`herdr_escalation.rs:61-103`):

```rust
// crates/atm-http-runtime/src/herdr_escalation.rs
#[derive(Clone, Default)]
pub(crate) struct EpisodeState {
    episodes: Arc<Mutex<HashMap<MemberKey, Episode>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Episode {
    pub(crate) kind: EpisodeKind,
    pub(crate) since: IsoTimestamp,
    pub(crate) notified: bool,
}

impl EpisodeState {
    /// Records the observation. Returns the current episode when `state` is
    /// Blocked/Offline (starting one if needed, or replacing one of the other
    /// kind); clears and returns `None` for every other state.
    pub(crate) fn observe(&self, member: &MemberKey, state: RuntimeMemberState, now: IsoTimestamp) -> Option<Episode>;
    pub(crate) fn mark_notified(&self, member: &MemberKey);
}
```

In-RAM only (R4): a daemon restart starts a fresh episode and may re-send;
the message body carries `since`, so a reader can tell the duplicate.

## Runtime changes — `crates/atm-http-runtime/src/herdr_queue_wake*.rs`

| before (develop) | after |
| --- | --- |
| `collect_idle_members(…, task_candidates: &mut Vec<TaskCandidate>)` pushes `Idle | Blocked` only (`herdr_queue_wake.rs:444-518`) | pushes every member with an accepted observation as `MemberObservation { member: HerdrCandidate, state: RuntimeMemberState }`; `TaskCandidate { blocked: bool }` deleted |
| `read_due_task` — one `list_tasks(team, Some(member))` per candidate (`_reminders.rs:81-113`) | one `open_tasks_for_team(team, deadline)` per team per tick; grouped in memory `HashMap<AgentName, Vec<TaskRow>>` (already `position`-ordered); head = `.first()` |
| `select_open_task` (`herdr_queue_wake.rs:855-866`, Active-first then `assigned_at`) | deleted — the queue order is the storage order |
| `emit_task_reminder` → `record_task_outcome` → `maybe_escalate_task` with multiplicative threshold (`_escalation.rs:37-43`) | `emit_task_reminder` runs only on `Nudge`; `EscalateStalled` calls `escalate_stalled_task` (renamed `maybe_escalate_task`, threshold test removed — `dispose` decided) which sends the one message and `record_lead_notified` |
| `escalate_blocked` / `escalate_one_blocked` with `BLOCKED_NOTIFY_MS`, `blocked_cooldown`, `next_blocked_batch` (`_escalation.rs:176-235`) | `escalate_episode(member, episode, open_tasks)` on `EscalateEpisode`; body = existing `blocked_body` generalised with the kind word; `mark_notified` on `reached_anyone()` |
| `HERDR_MAX_PROMPTS_PER_TICK` guard applies to reminders | unchanged; escalation messages do not count against it (they are mail, not prompts) |

Tick order per team: (1) roster observations applied, (2) queue drain
(unchanged; BA.5 reorders), (3) `open_tasks_for_team`, (4) for each observed
member `dispose(...)` → act. A member prompted by the drain in this tick is
`Hold`-equivalent for tasks (existing `prompted_by_drain` skip, kept).

**Start (R1):** when the task nudge for `head` is handed to Herdr
successfully (`ReminderOutcome::Emitted`), the pump submits one
`WriteRequest` from `caller_identity = atm-daemon` to the **assigner** with
`task_id = head.task_id`, `task_op = Some(TaskOp::Start)`, body from the
existing `task_started` template class (add one template alongside the
reminder templates; no new nudge kind — ADR-054 inventory unchanged). If the
head is already `Active` the writer's idempotent arm makes this a no-op and
the receipt is **not** sent (the pump checks `head.state` first and skips the
write). Failure to write the start is logged and retried next tick; the
nudge already went out — the receipt is at-least-once.

**Re-check before emit:** immediately before handing a nudge to Herdr, the
pump re-reads the member's roster record (`service_runtime` in-RAM, no I/O)
and drops the nudge unless it is still `Idle`. A check, not state.

## Constants

`TASK_REMINDER_INTERVAL_MS = 60_000` (moved), `TASK_STALLED_REMINDER_THRESHOLD = 10`
(unchanged, `atm-storage/src/task_store.rs:20`). Deleted: `BLOCKED_NOTIFY_MS`,
`BLOCKED_RENOTIFY_MS`. Offline is reported on the first accepted `Offline`
observation with no debounce — the roster acceptance path is the only filter
(design §1). If this proves noisy the fix is one constant, added by ruling.

## Paths to delete

- `TaskCandidate`, `select_open_task`, `read_due_task` (`herdr_queue_wake.rs`, `_reminders.rs`)
- `EscalationState` and its four methods, `BLOCKED_NOTIFY_MS`, `BLOCKED_RENOTIFY_MS` (`herdr_escalation.rs:49,61-110`)
- `escalate_blocked`, `escalate_one_blocked`, `blocked_tasks` per-member read (`_escalation.rs:176-270`)
- multiplicative threshold block (`_escalation.rs:37-43`)
- tests: `blocked_renotifies_after_cooldown`, `escalates_again_at_twenty` and
  any test asserting a second stalled escalation (grep `lead_notified_count, 2` / `RENOTIFY`)

## Tests

Pure — `herdr_task_disposition.rs`:

- `dispose_table_is_exhaustive` — all 6 states × {no task, assigned fresh,
  assigned rate-limited, assigned at threshold, assigned escalated, active}
  × episode_notified {false,true} → expected disposition; **72 rows**
  written out, no wildcard.
- `blocked_without_task_still_escalates_once`.
- `active_member_with_stalled_task_holds` — reminder_count 10, state Active → `Hold(Active)`.
- `threshold_is_terminal` — reminder_count 10, lead_notified_count 0 →
  `EscalateStalled`; lead_notified_count 1, reminder_count 25 → `Hold(Stalled)`.
- `rate_limit_boundary` — `last_reminded_at = now − 59_999 ms` → RateLimited;
  `now − 60_000 ms` → Nudge.
- `episode_state_replaces_kind_on_flip` — Blocked then Offline → new
  episode, `notified = false`; `observe(Idle)` clears; `observe(Blocked)`
  again → fresh `since`.

Runtime — `crates/atm-http-runtime/tests/herdr_nudge_invariant.rs` (new;
fixture daemons loopback only; roster shapes: lead+1, lead+3, two leads,
no lead):

- `idle_member_with_queued_task_is_nudged_once_per_interval` — 3 ticks at
  t, t+30s, t+61s → exactly 2 prompts.
- `active_member_is_never_prompted` — 200 ticks, head assigned, state Active → 0 prompts, 0 mail.
- `blocked_member_gets_one_message_zero_nudges_per_episode` — 50 ticks
  Blocked → 1 mail to lead + recipients, 0 prompts; then Idle → Blocked
  again → 2nd mail.
- `offline_member_gets_one_message_zero_nudges_per_episode` — same shape.
- `blocked_with_no_open_task_still_escalates`.
- `tenth_reminder_escalates_once_then_silence` — drive 10 emitted
  reminders → 1 escalation mail, `lead_notified_count = 1`; 100 more ticks
  → 0 prompts, 0 mail.
- `close_of_stalled_task_resumes_nudging_on_next_task` — after terminal
  escalation, close head (via `TaskOp::Close`) → next tick nudges the new
  head with counters 0.
- `start_resets_counters_and_resumes` — stalled head; `TaskOp::Start`
  applied → `Hold(Stalled)` no longer; nudge at next interval.
- `handoff_applies_start_and_sends_receipt_to_assigner` — first emitted
  nudge → task `active`, `started` event actor `atm-daemon`, one message in
  the assigner's mailbox; second emitted nudge (agent still idle) → **no**
  second receipt, no second event.
- `head_already_active_handoff_sends_no_receipt`.
- `member_turning_active_between_dispose_and_emit_is_not_prompted` — flip
  the in-RAM roster record inside the test hook between `dispose` and the
  Herdr call → 0 prompts.
- `two_leads_escalation_goes_to_recipients_only` and
  `no_lead_no_recipients_escalation_is_logged_not_sent` (existing
  `EscalationTargets` behaviour, re-asserted under the new path).
- `one_team_read_per_tick` — count `open_tasks_for_team` calls with 5
  idle members → 1 per tick.
- `daemon_restart_reescalates_ongoing_blocked_episode_with_same_since_in_body`
  — R4 accepted behaviour, pinned so it is visible.
- `escalation_mail_does_not_consume_prompt_budget` — 16 idle members with
  due tasks + 1 blocked → 16 prompts and 1 mail in one tick.
- `picker_member_status_is_not_referenced` — `grep -c PickerMemberStatus
  crates/atm-http-runtime/src` = 0 (architecture test in `atm-architecture`).

## Acceptance criteria

1. `dispose` is the only place that decides nudge/escalate/hold; grep for
   `RuntimeMemberState::` in `herdr_queue_wake*.rs` shows only the
   observation mapping and the pre-emit re-check.
2. All tests above pass; the 72-row table is present.
3. `BLOCKED_RENOTIFY_MS`, `EscalationState`, `select_open_task` no longer exist.
4. `doctor` and `atm task events` show `started` events with actor
   `atm-daemon` on every handed-off task in the fixture run.

## Required validation

`just lint`, `just test`, RULE-003 (`herdr_queue_wake.rs` must not grow;
target ≤ 3,800 lines after deletions), `just lint-boundaries`.

## Out of scope

Mail-before-task ordering and the ephemeral message reminder (BA.5); CLI (BA.4).
