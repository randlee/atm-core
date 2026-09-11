# BA.3 — Nudge invariant and terminal escalation

| Field | Value |
| --- | --- |
| Design | [`nudge-task-design.md`](./nudge-task-design.md) §1, §2, §4.2, §6, §6.1, §6.2, §8 (commit `9b5c7d876`) |
| Outcomes | B1, B2, B11, B12 |
| Recommended | arch-ctm / deep-reasoning — replaces the reminder/escalation loop in a 4k-line runtime module |
| Depends on | `must_follow` BA.2 (dev push) — `open_tasks_for_team`, `TaskOp::Start`, `position`, `WriteOutcome.task_close` |
| `parallel_safe` | none — BA.4 and BA.5 both `must_follow` this sprint (BA.4's `TaskMove` router arm and BA.5's handoff re-arm land in files this sprint rewrites: `storage_and_nudge_router.rs`, `herdr_queue_wake.rs`). This sprint owns every file under `crates/atm-http-runtime/src/` it touches: `herdr_*`, `herdr_breaker_escalation.rs` (deleted), `storage_and_nudge_router.rs::commit_write`. |
| Worktree | `feature/ba3-nudge-invariant` off `integrate/phase-ba` (merge BA.2 forward) |
| Governed interfaces | none (Herdr IPC unchanged) |
| Decisions | plan §4 R1 (start = handoff), R4 (episode suppression from the mailbox), R5 (refusal threshold — emitted here) |

## Scope

One pure disposition function decides, per roster member per tick, from
exact `RuntimeMemberState` and that member's head task: nudge, escalate, or
hold. Every roster member with an accepted observation is disposed,
whatever its delivery backend (design §1/§2: the invariant is over agent
state, the roster is the SSOT). Escalation is terminal at the threshold.
Blocked and Offline are one message per episode with zero nudges, and the
lead's mailbox — not RAM — is what says an episode was already reported
(design §6.1). A successful task handoff to an `Idle` member applies
`TaskOp::Start`. Consecutive-refusal escalation (design §4.2) is emitted
from this crate's post-write seam. The breaker escalation machinery listed
in design §8 is deleted.

## Deliverables

| id | deliverable | where |
| --- | --- | --- |
| D1 | `TaskDisposition`, `EpisodeKind`, `HoldReason`, `dispose()`, `reminder_due()`, `TASK_REMINDER_INTERVAL_MS` | `crates/atm-http-runtime/src/herdr_task_disposition.rs` (new, pure) |
| D2 | `EscalationState` re-shaped to `{episodes}`, `Episode`, `escalation_summary()`, `episode_already_reported(target, summary, since)` — per-target, episode-bounded | `crates/atm-http-runtime/src/herdr_escalation.rs` |
| D3 | `EscalationKind::RefusalsEscalated`; `escalate_mail()` factored out of `escalate()`; `write_escalation_mail` gains `summary` | `herdr_escalation.rs` |
| D4 | roster-wide candidate sweep (`herdr_candidates` filter removed), `MemberObservation`, one `open_tasks_for_team` per team per tick, `dispose` → act, pre-emit re-check | `herdr_queue_wake.rs`, `herdr_queue_wake_reminders.rs` |
| D5 | `escalate_stalled_task`, `escalate_episode` | `herdr_queue_wake_escalation.rs` |
| D6 | Start handoff write + `task_started` template | `herdr_queue_wake_reminders.rs`, `crates/atm-core/templates/` (reminder template class) |
| D7 | refusal escalation in `commit_write` | `storage_and_nudge_router.rs:273-337` |
| D8 | deletions listed under "Paths to delete", incl. `herdr_breaker_escalation.rs` | — |
| D9 | tests named below | `herdr_task_disposition.rs`, `tests/herdr_nudge_invariant.rs` |

## Phase AZ code used

| AZ artifact | how |
| --- | --- |
| `herdr_attention_scheduler.rs:120-147` `persistent_candidate` / `task_reminder_due` — eligibility computed **only** from an accepted `Idle` observation, never from raw Herdr list results | **pattern kept**: `dispose()` below takes the accepted `RuntimeMemberState`, and `collect_idle_members` already routes through `apply_roster_runtime_observations` (`herdr_queue_wake.rs:478-489`) |

Not used: `IdleOpportunity`, lane cursors, reservations, `RosterStateRevision`
revalidation (design §1 "no edge detection/revision revalidation").

## Types — exactly as they land (`crates/atm-http-runtime/src/herdr_task_disposition.rs`, new, pure, ≤ 150 lines)

```rust
use atm_core::boundary::{RuntimeMemberState, TaskRow, TASK_REMINDER_INTERVAL_MS, TASK_STALLED_REMINDER_THRESHOLD};
use atm_storage::types::IsoTimestamp;

// `TASK_REMINDER_INTERVAL_MS` moves out of this crate: `herdr_queue_wake.rs:44`
// (`pub const TASK_REMINDER_INTERVAL_MS: u64`) is deleted and
// `pub const TASK_REMINDER_INTERVAL_MS: i64 = 60_000;` lands in
// `crates/atm-storage/src/task_store.rs`, next to `TASK_STALLED_REMINDER_THRESHOLD`
// (`:22`), re-exported through `atm_core::boundary`. atm-storage is below both
// atm-core and atm-http-runtime, so BA.5's `nudge_dispatch` (atm-core) and this
// crate read one constant (design §6; FNX-BA-CRIT-032).

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

impl EpisodeKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self { Self::Blocked => "blocked", Self::Offline => "offline" }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoldReason {
    NoOpenTask,
    Active,            // never divert
    Unobserved,        // Unknown
    IdentityConflict,
    RateLimited,       // idle, < TASK_REMINDER_INTERVAL_MS since last reminder
    Stalled,           // idle, already escalated once; wait for a state change
    EpisodeNotified,   // blocked/offline, message already sent this episode
    NoDeliveryChannel, // Nudge decided, but the member's backend has no built-in dispatch (logged once per tick)
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

/// Due when no reminder was ever sent, or at least the interval has elapsed.
/// `IsoTimestamp` exposes only `into_inner()` (`atm-storage/src/types.rs:359-371`);
/// the difference is a signed `chrono::Duration`, so a clock that moved
/// backwards (negative elapsed) is "not due" until real time catches up —
/// never a burst of reminders (FNX-BA-CRIT-008).
fn reminder_due(task: &TaskRow, now: IsoTimestamp) -> bool {
    match task.last_reminded_at {
        None => true,
        Some(last) => (now.into_inner() - last.into_inner()).num_milliseconds() >= TASK_REMINDER_INTERVAL_MS,
    }
}
```

Blocked/Offline are matched before `head` so an episode with no open task
still escalates once (design §1: "blocked/dead → escalate immediately").

`EscalationState` (`herdr_escalation.rs:61-103`) is **kept** (design §8
correction) and its fields replaced — the cooldown maps go, the episode map
and the "last seen healthy" map come:

```rust
// crates/atm-http-runtime/src/herdr_escalation.rs
#[derive(Clone, Default)]
pub(crate) struct EscalationState {
    /// Members currently in a Blocked/Offline episode, as seen by this process.
    /// The only in-RAM escalation state; the durable record is the mailbox
    /// (design §6.1) and the episode bound is `Episode.since`.
    episodes: Arc<Mutex<HashMap<MemberKey, Episode>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Episode {
    pub(crate) kind: EpisodeKind,
    /// Roster `state_changed_at` (`contract.rs:1034`) when present, else the
    /// observation time. Goes in the message body for the human reader.
    pub(crate) since: IsoTimestamp,
    pub(crate) notified: bool,
}

impl EscalationState {
    /// Records the observation. Blocked/Offline → the current episode
    /// (started at `state_changed_at`, else `now`; replaced when the kind
    /// flipped), else removes any episode and returns `None`.
    pub(crate) fn observe(&self, member: &MemberKey, state: RuntimeMemberState, state_changed_at: Option<IsoTimestamp>, now: IsoTimestamp) -> Option<Episode>;
    pub(crate) fn mark_notified(&self, member: &MemberKey);
}

/// The `summary` of every escalation message — the one constructor for the
/// durable "already reported" key (RBP-F002). `member` renders through
/// `MemberKey::Display` (`types.rs:1153-1156`, `<agent>@<team>`); `task` is
/// appended for task-scoped kinds (stalled). No timestamp in it — `since`
/// goes in the body.
pub(crate) fn escalation_summary(kind: EscalationKind, member: &MemberKey, task: Option<&TaskId>) -> String {
    match task {
        Some(task_id) => format!("escalation:{}:{}:{}", kind.as_str(), member, task_id),
        None => format!("escalation:{}:{}", kind.as_str(), member),
    }
}

/// Newest-first scan bound for the mailbox check (`mailbox_reader.rs:207`
/// orders `message_at DESC`); the query is already narrowed to
/// `sender = daemon`, so 500 covers any real mailbox. Overflow means one
/// duplicate mail, never a lost one (RSH-002).
pub(crate) const ESCALATION_MAILBOX_SCAN_LIMIT: usize = 500;

/// Design §6.1: the mailbox is the record. Called once per **target** when an
/// episode is first seen by this process (not per tick). A report counts only
/// if it is at or after the episode's start, so a report from an earlier,
/// cleared episode never suppresses a new one (FNX-BA-CRIT-028).
pub(crate) async fn episode_already_reported(
    reader: &dyn AsyncMailboxReader,
    target: &MailboxScope,          // lead: (team, lead); recipient: parsed from its address
    summary: &str,
    since: IsoTimestamp,            // Episode.since
    deadline: ReadDeadline,
) -> Result<bool, ReadLaneError> {
    let query = MessageQuery { team: target.team.clone(), agent: target.agent.clone(),
        sender: Some(daemon_actor()), task_id: None, limit: Some(ESCALATION_MAILBOX_SCAN_LIMIT) };
    let messages = reader.list_messages(target.clone(), query, deadline).await?;
    Ok(messages.iter().any(|m| m.envelope.summary.as_deref() == Some(summary)
        && m.envelope.timestamp >= since))
}
```

`daemon_actor()` is the existing construction of the `atm-daemon`
`AgentName` used by `write_escalation_mail` (`DAEMON_ACTOR_NAME`,
`atm-storage/src/task_state.rs:13`); no new constant. Targets are exactly
the existing `EscalationTargets { lead: Option<AgentName>, recipients:
Vec<String> }` (`herdr_escalation.rs:204-207`): the lead when unique, and
every configured recipient. `write_escalation_mail` writes each target
through the local write path, so the local store holds the row under the
recipient's `(team, agent)` scope whatever host it forwards to; a recipient
address that does not parse into a mailbox scope is written unconditionally
(existing behaviour). Each target is checked and written **independently**
(design §6.2): a target with no report since `since` is written this tick,
whatever happened to the others, so a lead-success/recipient-failure tick is
completed on the next tick rather than suppressed forever.

Restart behaviour (R4): `since` is the roster's `state_changed_at`. A member
Herdr still lists as Blocked keeps its original `state_changed_at` across a
daemon restart, so the existing report suppresses. A heartbeat-only member
whose first post-restart observation starts a new `state_changed_at` may be
reported once more — the mirror-image residual design §6.1 already accepts,
stated here so QA does not file it.

```rust
pub(crate) enum EscalationKind {
    TaskStalled,        // existing (`lead_notified`)
    BlockedEscalated,   // existing
    OfflineEscalated,   // new: design §1 offline episode
    RefusalsEscalated,  // new: design §4.2, plan §4 R5
    // `BreakerOpened` deleted (design §8)
}

impl From<EpisodeKind> for EscalationKind {
    fn from(kind: EpisodeKind) -> Self {
        match kind { EpisodeKind::Blocked => Self::BlockedEscalated, EpisodeKind::Offline => Self::OfflineEscalated }
    }
}

/// `escalate()` minus the Herdr notification: loads targets and writes the
/// mail (lead + recipients) with `summary`. `escalate()` calls this then
/// notifies; the refusal path (below) calls only this — design §6.1:
/// "escalation is a message".
pub(crate) async fn escalate_mail(
    runtime: &LocalServiceRuntime,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    daemon_home: &Path,
    team: &TeamName,
    summary: &str,
    mail_body: &str,
    kind: EscalationKind,
    /// `Some(since)`: skip every target already holding this `summary` at or
    /// after `since` — episodes (`Episode.since`), stalled tasks
    /// (`head.assigned_at`), refusal runs (`run_started_at`). Every terminal
    /// escalation uses the same verify-and-retry path (RSH-001). `None` is
    /// reserved for callers with no episode bound; none exist in this phase.
    suppress_since: Option<IsoTimestamp>,
) -> EscalationOutcome;
```

## Runtime changes — `crates/atm-http-runtime/src/herdr_queue_wake*.rs`

| before (develop) | after |
| --- | --- |
| `herdr_candidates` (`herdr_queue_wake.rs:868-900`) `continue`s every member whose channel is not `DeliveryChannel::HerdrSteer` (`:876-884`) and every `Tmux` backend (`:890`) — non-Herdr members never reach state evaluation | every roster member is a candidate; the backend is resolved **after** disposition by the existing `rebuild_received_hook_dispatch` (`:669-677`, per-backend `BuiltInPostSendDispatch`) and emitted through the existing `AsyncMessageReceivedHookEmitter`; a member whose backend yields no dispatch is `Hold(NoDeliveryChannel)` (FNX-BA-CRIT-007). Escalation needs no channel — it is mail. |
| `collect_idle_members(…, task_candidates: &mut Vec<TaskCandidate>)` pushes `Idle | Blocked` only (`herdr_queue_wake.rs:444-518`) | pushes every member with an accepted observation as `MemberObservation { member: MemberKey, state: RuntimeMemberState, state_changed_at: Option<IsoTimestamp> }` — roster identity only; no `HerdrCandidate`, no Herdr agent name or session (those are resolved per backend after disposition, FNX-BA-CRIT-024); `TaskCandidate { blocked: bool }` deleted |
| `read_due_task` — one `list_tasks(team, Some(member))` per candidate (`_reminders.rs:81-113`) | one `open_tasks_for_team(team, deadline)` per team per tick; grouped in memory `HashMap<AgentName, Vec<TaskRow>>` (already `position`-ordered); head = `.first()` |
| `select_open_task` (`herdr_queue_wake.rs:855-866`, Active-first then `assigned_at`) | deleted — the queue order is the storage order |
| `emit_task_reminder` → `record_task_outcome` → `maybe_escalate_task` with multiplicative threshold (`_escalation.rs:37-43`) | `emit_task_reminder` runs only on `Nudge`; `EscalateStalled` calls `escalate_stalled_task` (renamed `maybe_escalate_task`, threshold test removed — `dispose` decided): `escalate_mail(…, summary = escalation_summary(EscalationKind::TaskStalled, member, Some(&head.task_id)), …, suppress_since = Some(head.assigned_at))`, then `record_lead_notified` **only when every target was written or skipped-as-reported**; a failed target leaves `lead_notified_count = 0`, so `dispose` returns `EscalateStalled` again next tick and only the missing target is written (same verify-and-retry pattern as episodes — RSH-001) |
| `escalate_blocked` / `escalate_one_blocked` with `BLOCKED_NOTIFY_MS`, `blocked_cooldown`, `next_blocked_batch` (`_escalation.rs:176-235`) | `escalate_episode(member, episode, open_tasks)` on `EscalateEpisode`: `escalate_mail(…, summary = escalation_summary(episode.kind.into(), member, None), body = existing `blocked_body` (`_escalation.rs:279`) generalised with the kind word and `since`, kind, suppress_since = Some(episode.since))` — per target: already reported since `since` → skip; else write. `mark_notified` when every target is either skipped or written; a failed target leaves `notified = false` so the next tick retries **that target only** (the others are then skipped by their own report) |
| `maybe_escalate_breaker` / `escalate_breaker_cycle` on breaker open (`herdr_queue_wake.rs:301-379`, `herdr_breaker_escalation.rs`) | deleted (design §8). A Herdr list failure changes **no** roster state: the observation is recorded unavailable (`herdr_queue_wake.rs:424-435`, unchanged), no member enters an episode, no mail is written (requirements.md:4196-4203: a failed poll preserves state and triggers nothing). `Offline` is only ever an explicit observation (FNX-BA-CRIT-025). |
| `HERDR_MAX_PROMPTS_PER_TICK` guard applies to reminders | unchanged; escalation messages do not count against it (they are mail, not prompts) |

Tick order per team: (1) roster observations applied, (2) queue drain
(unchanged; BA.5 reorders), (3) `open_tasks_for_team`, (4) for each observed
member `dispose(...)` → act. A member prompted by the drain in this tick is
`Hold`-equivalent for tasks (existing `prompted_by_drain` skip, kept).

**Start (R1):** when the task nudge for `head` is handed off successfully
(`ReminderOutcome::Emitted`), the pump submits one `WriteRequest` from
`caller_identity = atm-daemon` to the **assigner** with `task_id =
head.task_id`, `task_op = Some(TaskOp::Start)`, body from the existing
`task_started` template class (add one template alongside the reminder
templates; no new nudge kind — ADR-054 inventory unchanged). If the head is
already `Active` the pump skips the write (the writer's idempotent arm would
make it a no-op and the receipt must not repeat).

**Start owed (FNX-BA-CRIT-029):** the handoff audit (`record_task_reminder`,
unchanged) has already set `last_reminded_at`, so a failed `Start` write
leaves a row with `state = assigned AND position = 1 AND last_reminded_at IS
NOT NULL` — the "start owed" predicate (BA.2 keeps `last_reminded_at` across
`Start`). At the top of every tick, before disposition, the pump submits the
idempotent `Start` write for every owed head in `open_tasks_for_team`, with
no prompt and regardless of the member's state — an `Active` member (the
common case after a successful handoff) gets its receipt this way. Rows at
position ≥ 2 are never owed (only heads are handed off), so a migrated
member with several reminded `assigned` rows starts exactly its head.

**Re-check before emit:** immediately before handing a nudge to the
emitter, the pump re-reads the member's roster record (`service_runtime`
in-RAM, no I/O) and drops the nudge unless it is still `Idle`. A check, not
state.

## Consecutive-refusal escalation (design §4.2, plan §4 R5)

Seam: `StorageAndNudgeRouter::commit_write` (`storage_and_nudge_router.rs:273-337`)
already holds the `WriteOutcome` after `prepared.finish(...)` (`:297`).
This crate depends on `atm-core`, owns `EscalationTargets`, and is the only
place that may call the escalation path (`escalate` is `pub(crate)` at
`herdr_escalation.rs:161`; `atm-core` cannot call it without a dependency
cycle — FNX-BA-CRIT-014 / PLAN-SCOPE-001). Exact addition, after `:297`:

```rust
        // `outcome: WriteOutcome` (an enum, write/pipeline.rs:13-16); the
        // router has `service_runtime: LocalServiceRuntime` and `daemon_home:
        // PathBuf` fields (storage_and_nudge_router.rs:54-62) and no task_store
        // field — the store comes from the runtime as herdr_queue_wake.rs:371
        // already does (FNX-BA-CRIT-026).
        if let atm_core::write::WriteOutcome::Sent(sent) = &outcome {
            if let Some(applied) = sent.task_close.as_ref() {
                if applied.outcome == TaskCloseOutcome::Refused
                    && applied.consecutive_refusals >= TASK_CONSECUTIVE_REFUSAL_THRESHOLD
                {
                    let team = canonical_request.caller_team.clone();
                    // keyed to the refusing assignee, never the caller — the
                    // assigner or the unique lead may have submitted the close
                    // (FNX-BA-CRIT-027)
                    let member = MemberKey::new(team.clone(), applied.assignee.clone());
                    let summary = crate::herdr_escalation::escalation_summary(
                        EscalationKind::RefusalsEscalated, &member, None);
                    let body = refusals_body(applied); // assignee, count, task id
                    let task_store = self.service_runtime.task_store().ok();
                    // `>=` plus suppression bounded to the run start: the 3rd
                    // refusal writes the mail; if that write failed, the 4th
                    // refusal finds no report since `run_started_at` and writes
                    // it; the 5th finds it and is silent (RSH-001).
                    let _ = crate::herdr_escalation::escalate_mail(
                        &self.service_runtime, task_store.as_ref(), &self.daemon_home,
                        &team, &summary, &body, EscalationKind::RefusalsEscalated,
                        Some(applied.run_started_at),
                    ).await;
                }
            }
        }
```

A compile-pinning test (`refusal_seam_compiles_against_write_outcome`, in
the router's test module) constructs a `WriteOutcome::Sent` with
`task_close = Some(TaskCloseApplied { consecutive_refusals: 3, .. })` and
asserts one `escalate_mail` call; the seam is code, not pseudocode.

One message per run: from the third refusal on, every refused close checks
each target's mailbox for this summary since `run_started_at` and writes
only what is missing; a non-refused close resets the run (BA.2 counts), so
the next run of three is reported again. The close has already committed —
a failed escalation write is logged, never surfaced to the closing caller,
and repaired by the next refusal in the run.

## Constants

`TASK_REMINDER_INTERVAL_MS = 60_000` (moved to `atm-storage/src/task_store.rs`,
now `i64` for the signed difference; deleted from `herdr_queue_wake.rs:44`), `TASK_STALLED_REMINDER_THRESHOLD = 10` (unchanged,
`atm-storage/src/task_store.rs:20`), `TASK_CONSECUTIVE_REFUSAL_THRESHOLD = 3`
(BA.2). Deleted: `BLOCKED_NOTIFY_MS`, `BLOCKED_RENOTIFY_MS`. Offline is
reported on the first accepted `Offline` observation with no debounce — the
roster acceptance path is the only filter (design §1). If this proves noisy
the fix is one constant, added by ruling.

## Paths to delete

- `TaskCandidate`, `select_open_task`, `read_due_task` (`herdr_queue_wake.rs`, `_reminders.rs`)
- `EscalationState.{blocked_since, last_blocked_notice, blocked_cursor}` and
  their four methods, `BLOCKED_NOTIFY_MS`, `BLOCKED_RENOTIFY_MS`
  (`herdr_escalation.rs:48-49,61-110`)
- `escalate_blocked`, `escalate_one_blocked`, `blocked_tasks` per-member read (`_escalation.rs:176-270`)
- multiplicative threshold block (`_escalation.rs:37-43`)
- the `DeliveryChannel::HerdrSteer` filter and the `Tmux => continue` arm in
  `herdr_candidates` (`herdr_queue_wake.rs:876-890`)
- **breaker escalation (design §8, PLAN-SCOPE-002):** the file
  `crates/atm-http-runtime/src/herdr_breaker_escalation.rs`
  (`HerdrBreakerEscalationGate`, `escalate_breaker_cycle`,
  `breaker_opened_mail_body`, its tests); the fields
  `breaker_escalation_gates`, `breaker_cycle_opened_at`,
  `breaker_failure_counts` (`herdr_queue_wake.rs:79-82`, initialisers
  `:112-115,137`) and the methods `breaker_cycle_opened_at`,
  `maybe_escalate_breaker` and their callers (`:301-379`);
  `EscalationKind::BreakerOpened` and its `as_str` arm
  (`herdr_escalation.rs:34,42`); the `mod herdr_breaker_escalation;` line
- tests: `blocked_renotifies_after_cooldown`, `escalates_again_at_twenty`,
  every breaker-escalation test, and any test asserting a second stalled
  escalation (grep `lead_notified_count, 2` / `RENOTIFY` / `BreakerOpened`)

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
  `now − 60_000 ms` → Nudge; `last_reminded_at = None` → Nudge.
- `rate_limit_clock_reversal_is_not_due` — `last_reminded_at = now + 5 s`
  (clock stepped back) → RateLimited, not Nudge.
- `episode_state_replaces_kind_on_flip` — Blocked then Offline → new
  episode, `notified = false`; `observe(Idle)` removes the episode;
  `observe(Blocked)` again with a newer `state_changed_at` → fresh `since`.
- `episode_since_prefers_roster_state_changed_at`.
- `escalation_summary_is_stable` — same `(kind, member, task)` → byte-equal
  string, `<agent>@<team>` order, no timestamp.

Runtime — `crates/atm-http-runtime/tests/herdr_nudge_invariant.rs` (new;
fixture daemons loopback only; roster shapes: lead+1, lead+3, two leads,
no lead; backends: Herdr steer, tmux, bare-CLI FIFO):

- `idle_member_with_queued_task_is_nudged_once_per_interval` — 3 ticks at
  t, t+30s, t+61s → exactly 2 prompts.
- `active_member_is_never_prompted` — 200 ticks, head assigned, state Active → 0 prompts, 0 mail.
- `non_herdr_active_member_is_never_prompted` — tmux-backed member, state
  Active via heartbeat, head assigned → 0 prompts, 0 mail (FNX-BA-CRIT-007).
- `non_herdr_idle_member_with_task_is_nudged_through_its_backend` — tmux
  member Idle via heartbeat → one dispatch built for the tmux backend and
  emitted; bare-CLI FIFO member → one FIFO append.
- `member_without_dispatchable_backend_holds_and_logs_once` —
  `Hold(NoDeliveryChannel)`, one `warn` per tick, no panic, no mail.
- `heartbeat_observed_offline_member_gets_one_episode_message` — a member
  with no Herdr backend goes Offline through the heartbeat ingress → 1
  mail, 0 prompts.
- `blocked_member_gets_one_message_zero_nudges_per_episode` — 50 ticks
  Blocked → 1 mail to lead + recipients, 0 prompts; then Idle → Blocked
  again → 2nd mail.
- `offline_member_gets_one_message_zero_nudges_per_episode` — same shape.
- `blocked_with_no_open_task_still_escalates`.
- `daemon_restart_does_not_reescalate_ongoing_episode` — R4: Blocked
  (roster `state_changed_at = t0`), 1 mail; restart the runtime (fresh
  `EscalationState`); 50 ticks still Blocked with the same `state_changed_at`
  → **0** further mail. Also asserts exactly one mailbox read per target per
  episode start, none per tick.
- `recovered_then_reblocked_episode_is_reported_again` — Idle then Blocked
  with `state_changed_at = t1 > t0` → the old report (timestamp < t1) does
  not suppress; 1 new mail (FNX-BA-CRIT-028).
- `recipients_only_episode_is_not_duplicated_after_restart` — team with two
  leads (no unique lead) and one configured recipient: 1 mail to the
  recipient; restart; 50 ticks → 0 further mail (the recipient's own mailbox
  is the record).
- `partial_target_failure_completes_missing_target_next_tick` — inject a
  failing recipient write on the first tick: lead written, recipient failed,
  `notified = false`; next tick → recipient written, lead **not** re-written;
  then `notified = true`.
- `poll_failure_produces_no_episode` — Herdr list error for 20 ticks →
  roster states unchanged, 0 mail, 0 prompts (FNX-BA-CRIT-025).
- `failed_start_write_then_active_member_still_starts_once` — handoff
  succeeds, the `Start` write is made to fail once, the member turns Active;
  next tick → `Start` applied through the owed-head path with no prompt,
  exactly one `task_started` receipt, row `active` (FNX-BA-CRIT-029).
- `episode_message_summary_and_body` — `summary == "escalation:blocked_escalated:<agent>@<team>"` (`MemberKey::Display`)
  (exact `MemberKey` `Display`), body contains `since`.
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
  emit → 0 prompts.
- `two_leads_escalation_goes_to_recipients_only` and
  `no_lead_no_recipients_escalation_is_logged_not_sent` (existing
  `EscalationTargets` behaviour, re-asserted under the new path).
- `one_team_read_per_tick` — count `open_tasks_for_team` calls with 5
  idle members → 1 per tick.
- `escalation_mail_does_not_consume_prompt_budget` — 16 idle members with
  due tasks + 1 blocked → 16 prompts and 1 mail in one tick.
- `third_consecutive_refusal_escalates_once` — through the real
  `commit_write`: 3 refused closes by A → 1 mail to lead + recipients with
  `summary == "escalation:refusals_escalated:A@<team>"`, kind `refusals_escalated`; a 4th
  refused close → no second mail; a `completed` then 3 more refused → one
  more mail. The closes themselves succeed whether or not the mail write
  succeeds (inject a failing lead write → close still committed, `warn`).
- `refusal_escalation_is_mail_only` — no Herdr notification is sent for
  `RefusalsEscalated` (design §6.1).
- `refusal_run_is_keyed_to_assignee_not_closer` — three refused closes of
  A's tasks submitted by the assigner, then by the unique lead → one mail
  naming A (FNX-BA-CRIT-027).
- `failed_refusal_mail_is_written_on_the_next_refusal` — inject a failing
  lead write on the 3rd refusal → 0 mail; 4th refusal → 1 mail; 5th → still
  1 (RSH-001).
- `stalled_escalation_partial_failure_completes_next_tick_then_records` —
  recipient write fails on the stalled tick: lead written,
  `lead_notified_count` stays 0; next tick writes the recipient only, then
  `lead_notified_count = 1`; 50 more ticks → silence.
- `every_escalation_summary_comes_from_one_constructor` — architecture
  test: `grep -c 'format!("escalation:' crates/` = 1 (RBP-F002).
- `escalation_ownership_architecture_test` — `crates/atm-architecture/tests/escalation_ownership.rs`
  (new): no `escalate`/`escalate_mail`/`escalation_summary` identifier under
  `crates/atm-core/src`; exactly one `match` over `RuntimeMemberState` in
  `crates/atm-http-runtime/src/herdr_*` and it is inside `dispose` (the
  existing `boundary_enforcement.rs` syn-visitor pattern; no boundary TOML
  edit, so no §10-style ruling is needed — RBQA-F001).
- `breaker_open_produces_no_escalation_mail` — trip the Herdr breaker → 0
  mail with kind `breaker_opened` (the kind no longer exists — compile-time
  proof is the enum, this test pins the runtime behaviour).
- `picker_member_status_is_not_referenced` — `grep -c PickerMemberStatus
  crates/atm-http-runtime/src` = 0 (architecture test in `atm-architecture`).

## Acceptance criteria

1. `dispose` is the only place that decides nudge/escalate/hold; grep for
   `RuntimeMemberState::` in `herdr_queue_wake*.rs` shows only the
   observation mapping and the pre-emit re-check.
2. All tests above pass; the 72-row table is present.
3. `grep -rn "BLOCKED_RENOTIFY_MS\|select_open_task\|breaker_escalation_gates\|breaker_cycle_opened_at\|breaker_failure_counts\|HerdrBreakerEscalationGate\|escalate_breaker_cycle\|BreakerOpened\|herdr_breaker_escalation" crates/` returns nothing (PLAN-SCOPE-002 grep gate).
4. `grep -n "DeliveryChannel::HerdrSteer" crates/atm-http-runtime/src/herdr_queue_wake.rs` returns nothing — the candidate sweep is backend-neutral.
5. `doctor` and `atm task events` show `started` events with actor
   `atm-daemon` on every handed-off task in the fixture run.
6. `grep -rn "escalate\b\|escalate_mail" crates/atm-core/src` returns nothing
   — escalation is emitted only from `atm-http-runtime`.

## Required validation

`just lint`, `just test`, RULE-003 (`herdr_queue_wake.rs` must not grow;
target ≤ 3,700 lines after deletions), `just lint-boundaries`.

## Out of scope

Mail-before-task ordering and the queue-item reminder (BA.5); CLI (BA.4);
any new nudge template kind (ADR-054 inventory unchanged).
