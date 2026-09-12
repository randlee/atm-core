# BA.3 — Nudge invariant and terminal escalation

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §1, §2, §4.2, §6, §6.1, §6.2, §8 (commit `18db5acc3`) |
| Recommended | arch-ctm / deep-reasoning — replaces the reminder/escalation loop in a 4k-line runtime module |
| Depends on | `must_follow` BA.2 (dev push) — `open_tasks_for_team`, `refusal_run`, `TaskOp::Start`, `position` |
| `parallel_safe` | none — BA.4 and BA.5 both `must_follow` this sprint. This sprint owns every `crates/atm-http-runtime/src/herdr_*` file and `storage_and_nudge_router.rs::reader tick`. |
| Worktree | `feature/ba3-nudge-invariant` off `integrate/phase-ba` (merge BA.2 forward) |
| Governed interfaces | none (Herdr IPC unchanged) |
| Decisions | plan §2 R1 (start = handoff), R4 (mailbox is the episode record), R5 (refusal threshold) |

## Tasks

1. Add the pure disposition module — `crates/atm-http-runtime/src/herdr_task_disposition.rs` (new) (see "Types").
2. Move `TASK_REMINDER_INTERVAL_MS` to storage as `i64` — `herdr_queue_wake.rs:44` → `crates/atm-storage/src/task_store.rs:22`.
3. Reshape `EscalationState` to the episode map; add `escalation_summary`, `episode_already_reported`, `escalate_mail`; add `EscalationKind::{OfflineEscalated, RefusalsEscalated}`, delete `BreakerOpened` — `herdr_escalation.rs` (see "Types").
4. Make every roster member a candidate; build `MemberObservation`; one `open_tasks_for_team` per team per tick; `dispose` → act — `herdr_queue_wake.rs`, `herdr_queue_wake_reminders.rs` (see "Runtime").
5. Replace `maybe_escalate_task` / `escalate_blocked` with `escalate_stalled_task` / `escalate_episode` — `herdr_queue_wake_escalation.rs`.
6. Add `complete_task_handoff` (reminder audit, `TaskOp::Start`, `task_started` receipt) — `herdr_task_start.rs` (new), `crates/atm-core/templates/`.
7. Add the refusal escalation to the tick — `herdr_queue_wake.rs` (see "Consecutive refusals").
8. Fold the `map_or(Unknown, …)` fallback into `runtime_state`; add `still_idle` — `herdr_queue_wake.rs:459-463`, `:934`.
9. Delete the breaker escalation and the paths under "Paths to delete".
10. Add the architecture test — `crates/atm-architecture/tests/escalation_ownership.rs` (new).
11. Write the tests under "Tests".

## Phase AZ code used

| AZ artifact | how |
| --- | --- |
| `herdr_attention_scheduler.rs:120-147` eligibility computed only from an accepted `Idle` observation | **pattern kept**: `dispose()` takes the accepted `RuntimeMemberState` from `apply_roster_runtime_observations` (`herdr_queue_wake.rs:478-489`) |

Not used: `IdleOpportunity`, lane cursors, reservations, `RosterStateRevision`.

## Types — exactly as they land (`herdr_task_disposition.rs`, new, pure, ≤ 120 lines)

```rust
use atm_core::boundary::{RuntimeMemberState, TaskRow, TASK_CONSECUTIVE_REFUSAL_THRESHOLD, TASK_REMINDER_INTERVAL_MS, TASK_STALLED_REMINDER_THRESHOLD};
use atm_storage::types::IsoTimestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskDisposition {
    /// Emit the task nudge for the member's head task.
    Nudge,
    /// The head task has reached the threshold: send the stalled message once.
    EscalateStalled,
    /// Member is Blocked / Offline and this is a new episode: one message, zero nudges.
    EscalateEpisode(EpisodeKind),
    /// Nothing this tick; the string is the log reason.
    Hold(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EpisodeKind { Blocked, Offline }

impl EpisodeKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self { Self::Blocked => "blocked", Self::Offline => "offline" }
    }
}

/// The whole invariant. `head` is the member's lowest-position open task.
/// `mail_pending` = the member has an open `atm queue` item (BA.5).
/// `new_episode` = `EscalationState::observe` just recorded a Blocked/Offline
/// episode this process had not seen.
pub(crate) fn dispose(
    mail_pending: bool,
    state: RuntimeMemberState,
    head: Option<&TaskRow>,
    now: IsoTimestamp,
    new_episode: bool,
    consecutive_refusals: u32,
) -> TaskDisposition {
    use RuntimeMemberState as S;
    use TaskDisposition as D;
    match (mail_pending, state, head) {
        (_, S::Active, _) => D::Hold("active"),
        (_, S::Blocked, _) => if new_episode { D::EscalateEpisode(EpisodeKind::Blocked) } else { D::Hold("episode reported") },
        (_, S::Offline, _) => if new_episode { D::EscalateEpisode(EpisodeKind::Offline) } else { D::Hold("episode reported") },
        (_, S::Unknown, _) => D::Hold("unobserved"),
        (_, S::IdentityConflict, _) => D::Hold("identity conflict"),
        (true, S::Idle, _) => D::Hold("mail pending"),
        (false, S::Idle, None) => D::Hold("no open task"),
        (false, S::Idle, Some(_)) if consecutive_refusals >= TASK_CONSECUTIVE_REFUSAL_THRESHOLD => D::Hold("refusals escalated"),
        (false, S::Idle, Some(task)) if task.lead_notified_count > 0 => D::Hold("stalled"),
        (false, S::Idle, Some(task)) if task.reminder_count >= TASK_STALLED_REMINDER_THRESHOLD => D::EscalateStalled,
        (false, S::Idle, Some(task)) if !reminder_due(task, now) => D::Hold("rate limited"),
        (false, S::Idle, Some(_)) => D::Nudge,
    }
}

/// Due when no reminder was ever sent, or at least the interval has elapsed.
/// A clock that moved backwards (negative elapsed) is "not due".
fn reminder_due(task: &TaskRow, now: IsoTimestamp) -> bool {
    match task.last_reminded_at {
        None => true,
        Some(last) => (now.into_inner() - last.into_inner()).num_milliseconds() >= TASK_REMINDER_INTERVAL_MS,
    }
}
```

Blocked/Offline are matched before `head` so an episode with no open task
still escalates once (design §1).

```rust
// crates/atm-http-runtime/src/herdr_escalation.rs
#[derive(Clone, Default)]
pub(crate) struct EscalationState {
    /// Members currently in a Blocked/Offline episode as seen by this
    /// process. The durable record is each target's mailbox (design §6.1).
    episodes: Arc<Mutex<HashMap<MemberKey, EpisodeKind>>>,
}

impl EscalationState {
    /// Blocked/Offline: insert (or replace on a kind flip) and return `true`
    /// when the map did not already hold this kind for the member. Any other
    /// state: remove the entry and return `false`.
    pub(crate) fn observe(&self, member: &MemberKey, state: RuntimeMemberState) -> bool;
}

pub(crate) enum EscalationKind {
    TaskStalled,        // existing (`lead_notified`)
    BlockedEscalated,   // existing
    OfflineEscalated,   // new
    RefusalsEscalated,  // new (design §4.2)
    // `BreakerOpened` deleted (design §8)
}

impl From<EpisodeKind> for EscalationKind { /* Blocked → BlockedEscalated, Offline → OfflineEscalated */ }

/// The `summary` of every escalation message — the one constructor for the
/// durable "already reported" key. `member` renders as `<agent>@<team>`.
pub(crate) fn escalation_summary(kind: EscalationKind, member: &MemberKey, task: Option<&TaskId>) -> String {
    match task {
        Some(task_id) => format!("escalation:{}:{}:{}", kind.as_str(), member, task_id),
        None => format!("escalation:{}:{}", kind.as_str(), member),
    }
}

/// Design §6.1: the mailbox is the record. One read per target per episode.
pub(crate) async fn episode_already_reported(
    reader: &dyn AsyncMailboxReader,
    target: &MailboxScope,
    summary: &str,
    since: IsoTimestamp,
    deadline: ReadDeadline,
) -> Result<bool, ReadLaneError> {
    let query = MessageQuery { team: target.team.clone(), agent: target.agent.clone(),
        sender: Some(DAEMON_ACTOR.clone()), task_id: None, limit: None };
    let messages = reader.list_messages(target.clone(), query, deadline).await?;
    Ok(messages.iter().any(|m| m.envelope.summary.as_deref() == Some(summary) && m.envelope.timestamp >= since))
}

/// `escalate()` minus the Herdr notification: loads `EscalationTargets`
/// (lead + configured recipients, `herdr_escalation.rs:204-243`) and writes
/// the mail with `summary`. `suppress_since = Some(t)`: skip a target whose
/// mailbox already holds `summary` at or after `t`; `None`: write every
/// target. A failed write or mailbox read is logged at `warn` and not retried.
pub(crate) async fn escalate_mail(
    runtime: &LocalServiceRuntime,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    daemon_home: &Path,
    team: &TeamName,
    summary: &str,
    mail_body: &str,
    kind: EscalationKind,
    suppress_since: Option<IsoTimestamp>,
) -> EscalationOutcome;
```

`DAEMON_ACTOR` is the existing static in `herdr_escalation.rs:29-30`. A
host-qualified recipient (`agent@team.host`) has no local mailbox row, so for
it the in-RAM episode map is the only suppression: one duplicate after a
daemon restart during a still-open episode is accepted (design §6.1 adds no
durable receipt).

## Runtime changes — `herdr_queue_wake*.rs`

| before (develop) | after |
| --- | --- |
| `herdr_candidates` (`herdr_queue_wake.rs:868-900`) skips every non-`HerdrSteer` channel and every `Tmux` backend | every roster member is a candidate; the backend is resolved after disposition by the existing `rebuild_received_hook_dispatch` (`:669-677`); a member whose backend yields no dispatch is `Hold("no delivery channel")`, one `warn` per tick |
| `collect_idle_members` pushes `Idle \| Blocked` only (`:444-518`) | pushes every member with an accepted observation as `MemberObservation { member: MemberKey, state: RuntimeMemberState, state_changed_at: Option<IsoTimestamp> }` built from `apply_roster_runtime_observations(..).current`; `TaskCandidate` deleted |
| `read_due_task` — one `list_tasks` per candidate (`_reminders.rs:81-113`) | one `open_tasks_for_team(team, deadline)` per team per tick, grouped in memory by assignee (already `position`-ordered); head = `.first()` |
| `select_open_task` (`:855-866`) | deleted — the queue order is the storage order |
| `maybe_escalate_task` with multiplicative threshold (`_escalation.rs:37-43`) | `escalate_stalled_task`: `escalate_mail(summary = escalation_summary(TaskStalled, member, Some(&head.task_id)), suppress_since = None)` then `record_lead_notified` (the row's `lead_notified_count` is the durable record) |
| `escalate_blocked` / `escalate_one_blocked` with cooldowns (`_escalation.rs:176-235`) | `escalate_episode`: `escalate_mail(summary = escalation_summary(kind.into(), member, None), body = existing `blocked_body` generalised with the kind word and `since`, suppress_since = Some(since))` where `since` = roster `state_changed_at`, else `now` |
| `maybe_escalate_breaker` / `escalate_breaker_cycle` (`herdr_queue_wake.rs:301-379`, `herdr_breaker_escalation.rs`) | deleted. A Herdr list failure changes no roster state and writes no mail (`:424-435` unchanged); `Offline` is only ever an explicit observation |
| `HERDR_MAX_PROMPTS_PER_TICK` applies to reminders | unchanged; escalation mail does not count |

Tick order per team: (1) roster observations applied; (2) owed starts (below);
(3) queue drain (unchanged; BA.5 passes its `list_pending_members` result on
as `open_mail: &HashSet<MemberKey>`); (4) `open_tasks_for_team`; (5) per
observed member: `new_episode = escalation.observe(member, state)`,
`refusals = refusal_run(team, member).count`,
`dispose(open_mail.contains(member), state, head, now, new_episode, refusals)`
→ act; when `refusals >= TASK_CONSECUTIVE_REFUSAL_THRESHOLD`, also
`escalate_mail(summary = escalation_summary(RefusalsEscalated, member, None), suppress_since = run.started_at)`.

**Start (R1).** `complete_task_handoff(runtime, member, head, now)` in
`herdr_task_start.rs` (new) is the only caller that submits `TaskOp::Start`:
(1) `record_task_reminder(member, head.task_id, now, ReminderOutcome::Emitted)`;
(2) if `head.state == Assigned`, one `WriteRequest` from `atm-daemon` to the
assigner with `task_id = head.task_id`, `task_op = Some(TaskOp::Start)`, body
from a `task_started` template added beside the reminder templates (existing
class; ADR-054 inventory unchanged). The reminder path calls it after
`Emitted`; BA.5 calls it from `complete_successful_claim` for a head-task
assignment message.

**Start owed.** The audit in step (1) sets `last_reminded_at`, so a failed
Start leaves `state = assigned AND position = 1 AND last_reminded_at IS NOT
NULL`. At the top of every tick, before disposition, the pump submits the
idempotent Start for every such head in `open_tasks_for_team`, with no prompt
and whatever the member's state.

**Re-check before emit.** Immediately before handing a nudge to the emitter
the pump re-reads the in-RAM roster record and drops the nudge unless still
`Idle`:

```rust
// herdr_queue_wake.rs (new); the only inspection of RuntimeMemberState outside `dispose`.
fn still_idle(runtime: &LocalServiceRuntime, member: &MemberKey) -> bool {
    runtime.roster_ephemeral_state(member.team(), member.agent()) // service_runtime.rs:668
        .map(|record| record.runtime.state)
        == Some(RuntimeMemberState::Idle)
}

// herdr_queue_wake.rs:934 — signature widened; the only place a RuntimeMemberState is built from Herdr data.
fn runtime_state(status: Option<HerdrAgentStatus>) -> RuntimeMemberState {
    match status {
        None | Some(HerdrAgentStatus::Unknown) => RuntimeMemberState::Unknown,
        Some(HerdrAgentStatus::Idle | HerdrAgentStatus::Done) => RuntimeMemberState::Idle,
        Some(HerdrAgentStatus::Working) => RuntimeMemberState::Active,
        Some(HerdrAgentStatus::Blocked) => RuntimeMemberState::Blocked,
    }
}
```

## Consecutive refusals (design §4.2, plan §2 R5)

One `refusal_run(team, assignee)` read per member per tick (BA.2). The pure
`dispose` receives only the count. At the threshold the member is held from
task prompts and the escalation mail is written once (the mailbox check with
`suppress_since = run.started_at` makes repeats no-ops); a non-refused close,
reassign, or reopen resets the derived run. The refused closes themselves
always succeed.

## Constants

`TASK_REMINDER_INTERVAL_MS = 60_000` (moved to `atm-storage/src/task_store.rs`,
`i64`), `TASK_STALLED_REMINDER_THRESHOLD = 10` (unchanged),
`TASK_CONSECUTIVE_REFUSAL_THRESHOLD = 3` (BA.3). Deleted: `BLOCKED_NOTIFY_MS`,
`BLOCKED_RENOTIFY_MS`. Offline is reported on the first accepted `Offline`
observation with no debounce.

## Paths to delete

- `TaskCandidate`, `select_open_task`, `read_due_task` (`herdr_queue_wake.rs`, `_reminders.rs`)
- `EscalationState.{blocked_since, last_blocked_notice, blocked_cursor}` and
  their methods, `BLOCKED_NOTIFY_MS`, `BLOCKED_RENOTIFY_MS` (`herdr_escalation.rs:48-49,61-110`).
  Design §8: `blocked_since` tracks the episode — that role moves to `episodes`
  (generalised to Offline; `since` is the roster `state_changed_at`);
  `last_blocked_notice` is the cooldown §8 replaces with the mailbox model
- `escalate_blocked`, `escalate_one_blocked`, `blocked_tasks` (`_escalation.rs:176-270`)
- multiplicative threshold block (`_escalation.rs:37-43`)
- the `DeliveryChannel::HerdrSteer` filter and the `Tmux => continue` arm (`herdr_queue_wake.rs:876-890`)
- `crates/atm-http-runtime/src/herdr_breaker_escalation.rs` and its `mod`
  line; fields `breaker_escalation_gates`, `breaker_cycle_opened_at`,
  `breaker_failure_counts` (`herdr_queue_wake.rs:79-82,112-115,137`); methods
  `breaker_cycle_opened_at`, `maybe_escalate_breaker` (`:301-379`);
  `EscalationKind::BreakerOpened` (`herdr_escalation.rs:34,42`)
- `HerdrQueueWakePump.last_task_attempt`, `stamp_task_attempt`, and the
  corresponding `prune_member_state` line; `last_reminded_at` is the sole
  task-reminder rate-limit record
- the `u64` copy of `TASK_REMINDER_INTERVAL_MS` in `herdr_queue_wake.rs`; the
  storage-owned `i64` constant is the sole definition
- tests `blocked_renotifies_after_cooldown`, `escalates_again_at_twenty`,
  every breaker-escalation test, and any test asserting a second stalled
  escalation (grep `lead_notified_count, 2` / `RENOTIFY` / `BreakerOpened`)

## Tests

Pure — `herdr_task_disposition.rs`:

- `dispose_table_is_exhaustive` — 6 states × {no task, assigned fresh,
  assigned rate-limited, assigned at threshold, assigned escalated, active} ×
  `mail_pending` × `new_episode` × refusal hold, written out, no wildcard.
- `blocked_without_task_still_escalates_once`.
- `active_member_with_stalled_task_holds` — reminder_count 10, Active → `Hold("active")`.
- `threshold_is_terminal` — reminder_count 10, lead_notified_count 0 →
  `EscalateStalled`; lead_notified_count 1, reminder_count 25 → `Hold("stalled")`.
- `rate_limit_boundary` — `now − 59_999 ms` → rate limited; `now − 60_000 ms` → Nudge; `None` → Nudge.
- `rate_limit_clock_reversal_is_not_due` — `last_reminded_at = now + 5 s` → rate limited.
- `episode_map_replaces_kind_on_flip_and_clears_on_recovery` — Blocked →
  `true`; Blocked again → `false`; Offline → `true`; Idle → `false` and the
  entry is gone; Blocked → `true`.
- `escalation_summary_is_stable` — byte-equal for the same inputs, `<agent>@<team>`, no timestamp.

Runtime — `crates/atm-http-runtime/tests/herdr_nudge_invariant.rs` (new;
fixture daemons loopback only; roster shapes lead+1, lead+3, two leads, no
lead; backends Herdr steer, tmux, bare-CLI FIFO):

This file is spliced into `herdr_queue_wake`'s test module with `#[path]`; it
is not a standalone Cargo integration-test target.

- `idle_member_with_queued_task_is_nudged_once_per_interval` — ticks at t, t+30s, t+61s → 2 prompts.
- `active_member_is_never_prompted` — 200 ticks Active, Herdr and tmux backends → 0 prompts, 0 mail.
- `non_herdr_idle_member_with_task_is_nudged_through_its_backend` — tmux → one tmux dispatch; FIFO → one append.
- `member_without_dispatchable_backend_holds_and_logs_once` — `Hold("no delivery channel")`, one `warn` per tick, no mail.
- `blocked_member_gets_one_message_zero_nudges_per_episode` — 50 ticks → 1 mail to lead + recipients, 0 prompts; Idle then Blocked → 2nd mail.
- `offline_member_gets_one_message_zero_nudges_per_episode` — via heartbeat ingress, no Herdr backend.
- `daemon_restart_does_not_reescalate_ongoing_episode` — Blocked with `state_changed_at = t0`, 1 mail; fresh `EscalationState`; 50 ticks → 0 further mail; exactly one mailbox read per target per episode start.
- `recovered_then_reblocked_episode_is_reported_again` — new `state_changed_at = t1 > t0` → the old report does not suppress; 1 new mail.
- `recipients_only_episode_is_not_duplicated_after_restart` — two leads, one recipient: 1 mail; restart; 0 further.
- `poll_failure_produces_no_episode` — Herdr list error 20 ticks → 0 mail, 0 prompts, roster unchanged.
- `episode_message_summary_and_body` — `summary == "escalation:blocked_escalated:<agent>@<team>"`, body contains `since`.
- `tenth_reminder_escalates_once_then_silence` — 10 emitted reminders → 1 mail, `lead_notified_count = 1`; 100 ticks → 0 prompts, 0 mail.
- `close_of_stalled_task_resumes_nudging_on_next_task` — next tick nudges the new head with counters 0.
- `reopen_of_stalled_task_escalates_again_at_threshold` — reopen resets counters; 10 more reminders → a second stalled mail.
- `handoff_applies_start_and_sends_receipt_to_assigner` — first emitted nudge → `active`, `started` actor `atm-daemon`, one receipt; second nudge → no second receipt or event.
- `head_already_active_handoff_sends_no_receipt`.
- `failed_start_write_then_active_member_still_starts_once` — Start fails once, member turns Active; next tick → Start via the owed-head pass, no prompt, one receipt.
- `member_turning_active_between_dispose_and_emit_is_not_prompted` — flip the in-RAM record inside the test hook → 0 prompts.
- `idle_member_with_open_mail_holds_mail_pending` — `Hold("mail pending")`, not nudged.
- `third_refusal_holds_and_escalates_once` — 3 refused closes by A → 1 mail with `summary == "escalation:refusals_escalated:A@<team>"`; a 4th → no second mail; later ticks → 0 task prompts; no Herdr notification.
- `non_refused_close_releases_refusal_hold`.
- `escalation_mail_does_not_consume_prompt_budget` — 16 idle members with due tasks + 1 blocked → 16 prompts and 1 mail in one tick.
- `breaker_open_produces_no_escalation_mail` — trip the breaker → 0 mail.
- `escalation_ownership_architecture_test` — `crates/atm-architecture/tests/escalation_ownership.rs`
  (new, standing CI): (a) no `escalate`/`escalate_mail`/`escalation_summary`
  identifier under `crates/atm-core/src`; (b) a `syn` visitor over every
  non-test item in `crates/atm-http-runtime/src/herdr_*.rs` (macro bodies
  scanned as token streams) asserts every path naming `RuntimeMemberState` or
  one of its variants lies inside `dispose`, `runtime_state`, or `still_idle`;
  (c) `grep -c PickerMemberStatus crates/atm-http-runtime/src` = 0. Same
  visitor pattern as `boundary_enforcement.rs:587-620`.

## Acceptance criteria

1. `escalation_ownership_architecture_test` passes in CI: `dispose` is the
   one function that turns a `RuntimeMemberState` into a decision.
2. Every test above exists by name and passes under `just test`; the
   disposition table has exactly the 12 arms quoted above.
3. `grep -rn "BLOCKED_RENOTIFY_MS\|select_open_task\|breaker_escalation_gates\|breaker_cycle_opened_at\|breaker_failure_counts\|HerdrBreakerEscalationGate\|escalate_breaker_cycle\|BreakerOpened\|herdr_breaker_escalation\|HoldReason" crates/` returns nothing.
4. `grep -n "DeliveryChannel::HerdrSteer" crates/atm-http-runtime/src/herdr_queue_wake.rs` returns nothing.
5. `grep -rn "TaskOp::Start" crates/atm-http-runtime/src` → `herdr_task_start.rs` only.
6. `grep -rn "escalate\b\|escalate_mail" crates/atm-core/src` returns nothing.

## Required validation

`just lint`, `just test`, `just lint boundaries`, RULE-003
(`herdr_queue_wake.rs` must not grow; target ≤ 3,700 lines after deletions).
