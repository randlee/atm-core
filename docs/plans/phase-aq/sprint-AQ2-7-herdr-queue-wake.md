# Sprint AQ2.7 — Queue: Herdr Poll-Gated Mailbox Wake-Up

Status: draft · Branch: `feature/aq-2-7-herdr-queue-wake` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Implement deferred queue wake-ups for AQ2.6's `HerdrSteer` members without
pretending that Herdr supplies a queue. The durable ATM mailbox is the queue:
mail remains immediately readable through `atm read`, while AQ1's pending
marker says that a wake-up is deferred. This sprint adds one host-wide,
Herdr-only Tokio **tick task** — no long-lived `herdr agent wait` children —
that polls `herdr agent list` on a fixed cadence and asks the existing Herdr
emitter to send the same mailbox-read prompt by the member `AgentName` to
every pending member it observes idle.

**Decision (Rand, 2026-08-26): the pump is a polling drain, not a
lifecycle-blocking gate.** The prior draft's per-member `agent wait` child
(bounded by a semaphore, up to a 45-minute timeout each) is deleted in full,
along with `HerdrQueueWakePumpConfig`. One Tokio task, ticking every
`HERDR_POLL_INTERVAL_MS = 5_000`, replaces it. `HerdrProcessAdapter::wait`
(ADR-058 D2) stays defined on the trait — AQ2.6 still owns it — but this
sprint never calls it; it is dead weight this sprint deliberately does not
remove from the trait, only from its own call graph.

Tmux and graft remain AQ3's claim paths. **This pump owns its own
Herdr-only guard** (claims only members AQ1's classifier reports as
`HerdrSteer`), and AQ3's idle drain and recovery sweep already carry the
mirror-image skip-Herdr pre-check (`sprint-AQ3-queue-tmux.md` deliverables 2
and 3, critical review B8: the drain was previously unguarded). Two workers
never claim a Herdr message only when both guards exist; AQ3 lands after
this sprint but already ships with its guard, so the risk window is nil.

This sprint's Herdr behaviour claims are governed by
[ADR-058](docs/adr/ADR-058-herdr-local-steer-backend-contract.md) (`herdr` 0.8.2, derived from source at
`d79fd746`). Where this doc and ADR-058 disagree, ADR-058 is authoritative;
this doc cites it by decision id (`D1`–`D10.1`). The Herdr process adapter's
module layout (crate `atm-herdr`) is normatively documented in
`docs/atm-herdr/architecture.md`; this sprint imports that crate, it does
not define its layout.

The mechanism is a poll-then-prompt drain, not an atomic primitive:

```text
herdr agent list                                    # one call per distinct session, every tick
# for each pending member whose returned agent_status is idle or done:
herdr agent prompt <AgentName> "You have unread ATM messages. Run: atm read"
```

There is a race between the `list` observation and `prompt` reaching the
terminal: the agent can begin a new turn in that interval. The plan must
preserve that truth in events, docs, and tests (ADR-058 D7 — retitled
"Acknowledged list→prompt race" for this sprint, but the acknowledgement
itself is kept verbatim in spirit from the prior `wait→prompt` draft).
`agent prompt`'s own `agent_blocked` rejection is the final guard against
injecting into a newly blocked dialog; the design promises lifecycle-aware
refusal, not delivery exactly at idle or turn-correlated queueing.

**State ownership.** This pump is not fire-and-forget. Every prompt it
emits is issued only against a message already **claimed** from AQ1's
`PendingNudgeStore` (`claim_next_pending`, oldest-first — the FIFO), and
every outcome is written back to that same store before the tick moves on:
a successful prompt needs no further write (the claim already cleared the
marker at claim time); `agent_blocked` or an absent/not-present target calls
`release_pending` (restores the marker, spends no retry budget); an
infrastructure failure (D10/D10.1) also calls `release_pending` and opens
the breaker. Herdr's own `agent_blocked` pre-write check (ADR-058 D4 row 4)
is defense in depth on top of this — it is never a substitute for the
pump's own claim/release bookkeeping, and no code path prompts a member
without a claim in hand.

**Convergence with AQ3.** Tmux's idle drain (AQ3) and this pump use the
**same** `PendingNudgeStore`, the **same** per-member FIFO rule (one claim
per idle observation, oldest message first), and the **same**
`requeue_pending`/`release_pending` semantics; only the idle-signal source
differs — AQ3 learns idle from harness heartbeat push hooks
(`TeamMemberHeartbeatRequest`, `RuntimeHealth.record_heartbeat`), this pump
learns it by polling `herdr agent list`. AQ2.7 ships its own minimal
tick-drain (deliverable 2) only because it lands before AQ3 and AQ3's
idle-transition hook does not yet exist. Deliverable 4 below feeds this
pump's poll observations into the same `RuntimeHealth` map harness
heartbeats populate, so that once AQ3 (or a follow-up) makes AQ3's
idle-drain channel pre-check kind-agnostic instead of Herdr-skipping, the
existing harness-side `MemberStateTransitionSink` hook starts firing for
Herdr members too — at that point this sprint's tick-drain (deliverable 2's
claim/dispatch loop) becomes **deletable**, not rewritable, and only the
poll-to-heartbeat producer (deliverable 4) survives. The tick-drain is kept
deliberately small and structured (one function, no state beyond the
constants below) specifically so that reduction is a deletion. This is
recorded again in Non-closure below; making AQ3's drain kind-agnostic is
explicitly **not** this sprint's or AQ3's committed scope — it is a pointer
for whichever sprint picks up the shared-drain follow-up.

## Deliverables

1. **Single Tokio tick task, no per-member children.** Compose
   `HerdrQueueWakePump` as a type in `atm-http-runtime` (beside
   `StorageAndNudgeRouter`/`HttpRuntime`, `crates/atm-http-runtime/src/lib.rs`);
   `atm-daemon-bootstrap` constructs and spawns it at daemon startup from
   `build_replacement_handler` (`atm-daemon-bootstrap/src/lib.rs:179-202`),
   the same function that already builds `active_received_hook_selector` and
   `StorageAndNudgeRouter`. `atm-http-runtime` gains a dependency on the new
   `atm-herdr` crate (structural change below) for the adapter types; it
   already depends on `tokio`.

   **Structural change (Rand, 2026-08-26): the Herdr process adapter moves
   to its own crate, `crates/atm-herdr`** (precedent: `crates/atm-graft`),
   authored by AQ2.6 (`sprint-AQ2-6-herdr-steer-backend.md` deliverable 3).
   This sprint only **imports** `atm_herdr::{HerdrProcessAdapter,
   HerdrAgentListEntry, HerdrAgentListOutcome, HerdrAgentStatus}` — it does
   not define the crate, its `Cargo.toml`, or its boundary manifest. The
   module layout is normatively documented in `docs/atm-herdr/architecture.md`
   (authored in planning alongside this sprint by a separate doc pass; this
   sprint updates it only if its own implementation deviates from what is
   already recorded there).

   **One tick, host-wide, no config struct.** A single `tokio::spawn`ed
   loop owns two named constants (no `HerdrQueueWakePumpConfig`, no
   CLI-facing override — deliberately out of scope, matching AQ2.6's Herdr
   backoff constants' precedent, ADR-058 D10.1):

   ```rust
   /// Tick cadence for the Herdr queue-wake poll. Not configurable in
   /// Phase AQ — a fixed operational default, same idiom as
   /// HERDR_BACKOFF_BASE_MS/HERDR_BACKOFF_CAP_MS (atm-herdr, ADR-058 D10.1).
   pub const HERDR_POLL_INTERVAL_MS: u64 = 5_000;
   /// Host-wide cap on prompts issued in one tick, across every Herdr
   /// session. Protects against a large simultaneous idle-transition burst
   /// spawning many `agent prompt` children in one instant.
   pub const HERDR_MAX_PROMPTS_PER_TICK: usize = 16;
   ```

   Each iteration: `tokio::select!` between `tokio::time::sleep(Duration::
   from_millis(HERDR_POLL_INTERVAL_MS))` and the runtime's existing shutdown
   watch (`HttpRuntime`'s `watch::Receiver<()>`, `lib.rs:380-387`, the same
   primitive `HttpRuntime::begin_shutdown` (`lib.rs:822`) sends on). On
   shutdown: stop ticking: any in-flight `herdr` child spawned during the
   current tick is already bounded by D10's 5 s steer/list bound (there is
   no longer a 45-minute wait bound to await — D10's wait bound and
   `HERDR_WAIT_GRACE_MS` are removed by this sprint, ADR-058 D10), so the
   current tick's own completion is the only thing the shutdown path awaits,
   wrapped in `tokio::time::timeout(<daemon shutdown deadline>, ...)`,
   mirroring `HttpRuntime::finish_shutdown`'s `tokio::time::timeout(self.
   config.timeouts.shutdown, &mut server_task)` (`lib.rs:853`) — this pump
   does not invent a second shutdown-deadline source. The legacy synchronous
   daemon is out of scope and must not be touched.

   It considers only `DeliveryChannel::HerdrSteer` members and only
   queue-kind pending markers. It owns no mailbox rows, FIFO, or retry
   count — retry/FIFO stay AQ1's store; this pump owns only the poll,
   the claim/dispatch call, and (deliverable 4) the poll-to-heartbeat feed.

2. **Tick body: session grouping, one `agent list` per session, FIFO claim
   with a host-wide burst cap.**

   a. **Roster-wide Herdr population.** Enumerate every `HerdrSteer` member
      (not just pending ones — deliverable 4 needs the full population) via
      `atm_core::boundary::store::RosterStore::list_teams()` +
      `load_roster(&team)` (`boundary/store.rs:176-202`), filtered through
      AQ1's `local_message_received_backend`/`classify_delivery_channel`
      exactly as AQ3's shared channel pre-check does, to
      `DeliveryChannel::HerdrSteer`. Group the resulting `(TeamName,
      AgentName, Option<HerdrSession>)` rows by their `Option<HerdrSession>`
      (`None` = Herdr's default server, one bucket).

   b. **Pending intersection.** Call `PendingNudgeStore::list_pending_members()`
      (bridged off the tick's async context the same way `StorageAndNudgeRouter`
      already bridges synchronous store calls, `BlockingCoreBridge`,
      `storage_and_nudge_router.rs:38-74`) and intersect with (a)'s
      `HerdrSteer` population to get the set of pending Herdr members this
      tick considers for a claim. Members outside this intersection (no
      pending mail, or classified `TmuxSteer`/`Graft`/`BareCli`) are never
      claimed against, but — per deliverable 4 — every member from (a) still
      gets a heartbeat observation if it appears in this tick's `list`
      result.

   c. **One `agent list` call per distinct session (critical review
      analogue to I17's original concurrency finding, now trivially
      satisfied since there is exactly one call per session, not one child
      per member).** For each session bucket from (a) with at least one
      member of interest (pending or heartbeat-eligible), invoke
      `HerdrProcessAdapter::list(session.as_deref(),
      RequestDeadline::after(<5 s list bound>))` — one call, `HERDR_SESSION`
      set on that child only when `session` is `Some` (ADR-058 D9.1). A
      distinct `RequestDeadline` is synthesized per call the same way other
      internal, non-inbound-request work in this crate already does
      (`storage_and_nudge_router.rs:1081`, `RequestDeadline::after(...)`
      outside any HTTP request context).

   d. **Idle gate + claim, FIFO by oldest pending ULID, burst-cap via
      claim-then-release (not pre-sort).** `PendingNudgeStore` is frozen by
      AQ1 ("no later sprint may define or widen them") and exposes no
      cross-member ordering key, only per-member oldest-first
      `claim_next_pending`. So: for every pending Herdr member (from b)
      whose `list` entry (from c) has `agent_status ∈ {idle, done}`, claim
      immediately — `claim_next_pending(member)` — collecting every
      successful `NudgeClaim` across every session polled this tick. Once
      every eligible member for the tick has been claimed, sort the
      collected claims by `claim.msg` (an `AtmMessageId`, ULID-backed — AQ1:
      "FIFO derived ... ULID order", lexicographically sortable by creation
      time) ascending, and dispatch the first `HERDR_MAX_PROMPTS_PER_TICK`
      of them (deliverable 3). Every claim beyond the cap is immediately
      `release_pending`'d — no prompt was attempted, so this spends no
      retry budget — and waits for the next tick's fresh idle observation.
      **Exactly one claim per member per tick**, and only when idle:
      `agent_status ∈ {working, blocked, unknown}`, or the member absent
      from that session's `list` result, produces no claim at all for that
      member this tick — five queued messages to one member are therefore
      at most five prompts total, one per satisfied idle tick, never five
      in one tick.

3. **Outcome handling — claim-then-write-back, no unclaimed prompt ever
   fires.** For each dispatched claim (deliverable 2d, in ULID order, capped
   at `HERDR_MAX_PROMPTS_PER_TICK`): rebuild the dispatch via AQ1's
   `rebuild_received_hook_dispatch(runtime, member, claim.msg,
   NudgeKind::Queue)` (`nudge_dispatch.rs`) and send it through the injected
   `Arc<dyn MessageReceivedHookSelector>` (the same selector instance
   `active_received_hook_selector` builds and `StorageAndNudgeRouter`
   already holds, `storage_and_nudge_router.rs:95`) — AQ2.6's extended
   `select_emitter` routes `(Queue, LocalSteer(Herdr(_)))` to
   `HerdrReceivedHook`, so this sprint calls the selector, never a private
   reference to the emitter or to `atm_herdr::HerdrProcessAdapter::prompt`
   directly.

   - **Success** → no further store write; `claim_next_pending` already
     cleared the marker at claim time. Emitted as `prompted`.
   - **`agent_blocked`** (the agent started a turn between the tick's `list`
     observation and this prompt — the acknowledged race, ADR-058 D7) →
     `release_pending(member, claim)`: restores exactly that marker without
     incrementing `nudge_attempts`. Emitted as `blocked_before_input_released`.
   - **`agent_not_found` / `agent_target_ambiguous` / `agent_not_ready`**
     (the member vanished, was renamed, or lost foreground between the
     tick's `list` snapshot and the prompt spawn) → `release_pending(member,
     claim)` — an absent/renamed target is not a delivery failure, so no
     retry budget is spent — plus the doctor-visible `held_target_not_present`
     counter (deliverable 5). Emitted as `held_target_not_present`.
   - **Timeout / spawn / protocol errors on the prompt itself, or on the
     tick's own `agent list` call (deliverable 2c)** → the shared,
     per-host `HerdrSpawnBreaker` (ADR-058 D10.1, now living beside the
     adapter in `atm-herdr`) opens; any claim already taken for that
     session this tick is `release_pending`'d (no retry budget spent —
     nothing was ever injected). While the breaker is open, subsequent
     ticks skip `list`/`prompt` calls for the affected session entirely
     (no spawn, `HerdrUnavailable` returned synchronously by the adapter)
     until `retry_after` elapses. Emitted as `dispatch_failed_requeued` is
     **not** used here — an infra failure never reaches `requeue_pending`,
     only `release_pending`, because Steer/Queue-kind Herdr dispatch has no
     independent retry counter of its own (AQ1 §1.4 applies the same way it
     does to AQ2.6's immediate steer path).

   A member absent from its session's `list` result entirely (deliverable
   2d) is never claimed in the first place — there is nothing to release;
   it is recorded once as `held_target_not_present` at the pre-claim
   filtering step, not duplicated at outcome time.

4. **Poll-to-heartbeat feed — every listed member, not only pending
   ones.** Each tick, for every `(team, member)` in deliverable 2a's
   roster-wide `HerdrSteer` population that has a matching entry (by
   `name`) in that session's `agent list` result (deliverable 2c), record
   the observed state into the **same** `RuntimeHealth` map harness
   heartbeats populate (`crates/atm-http-runtime/src/runtime_health.rs`),
   using the **same** `record_heartbeat` entry point
   `storage_and_nudge_router.rs`'s heartbeat handler already calls
   (`~487-510`, per `sprint-AQ3-queue-tmux.md`'s citation) — this is the
   mechanism the Non-closure/Convergence note above depends on for a
   future kind-agnostic AQ3 drain to pick Herdr members up for free.

   - **Mapping**: `agent_status` `idle`/`done` → `HeartbeatActivity::Idle`;
     `working`/`blocked` → `HeartbeatActivity::ActiveToolUse` (`RuntimeMemberState`
     has no distinct blocked/working variant — `protocol.rs:366-372` — so
     both map to the existing `Active` state); `unknown`, or the member
     absent from the `list` result, produces **no call** — an existing
     `RuntimeHealth` observation is never downgraded merely because one
     poll missed or could not classify the member (avoids flapping on a
     transient Herdr-side gap).
   - **Provenance tagging (so doctor can tell poll-derived state from a
     harness heartbeat push).** `RuntimeObservationSource`
     (`protocol.rs:322-326`, currently `{Heartbeat, LocalCommand}`) gains a
     third variant, `HerdrPoll` (serializes `herdr_poll`, `#[serde(rename_all
     = "snake_case")]` already on the enum). `TeamMemberHeartbeatRequest`
     (`protocol.rs:329-341`) gains `#[serde(default)] pub source:
     RuntimeObservationSource`, defaulting to `Heartbeat` (`#[derive(Default)]`
     + `#[default]` on the `Heartbeat` variant) so every existing native
     heartbeat caller (`storage_and_nudge_router.rs`'s handler, which never
     sets this field) is unaffected. `RuntimeHealth`'s private `MemberRecord`
     (`runtime_health.rs:49-56`) gains a `state_source: RuntimeObservationSource`
     field (defaulted `Heartbeat`); `record_heartbeat`
     (`runtime_health.rs:101-157`) stores `request.source` into it instead of
     leaving it implicit; `snapshot` (`runtime_health.rs:161-180`) reads
     `record.state_source` instead of the currently hardcoded
     `Some(RuntimeObservationSource::Heartbeat)` at line 173. This is a
     backward-compatible widening of an existing type this sprint owns the
     diff for — AQ3 (landing later) does not need to touch it.
   - **Synthesized request.** `TeamMemberHeartbeatRequest { team, member,
     pid: 0, observed_at: <tick wall-clock IsoTimestamp>, activity,
     source: RuntimeObservationSource::HerdrPoll, session_id: None }`.
     `pid: 0` is an explicit non-process sentinel — Herdr-poll observations
     are not tied to a harness OS process the way a native heartbeat is;
     `pid_changed` on the resulting `TeamMemberHeartbeatResponse` is not
     meaningful here and this sprint discards the response rather than
     forwarding it anywhere doctor-visible as an anomaly.
   - **The tick's own drain decision (deliverable 2d) uses the fresh `list`
     result directly, never a stale `RuntimeHealth` read-back** — heartbeat
     ingestion is a side effect of the poll for observability/convergence,
     not an input to this sprint's own claim logic.

5. **Observability.** Emit one per-tick structured event (same
   `emit_daemon_event` precedent AQ3 uses, `atm-daemon/bin_support/
   daemon_observability.rs`) with counts `{members_pending, idle, prompted,
   released, breaker_open}`, plus backend-qualified per-outcome events
   `{member, msg_id when claimed, queue_kind, outcome}` for `prompted`,
   `blocked_before_input_released`, `held_target_not_present`. Health
   counters for released and target-not-present work. Doctor gains
   `herdr_queue_pump: { last_tick_at, breaker: closed | open {retry_after_ms,
   consecutive_failures} }` (the breaker half reuses ADR-058 D10.1's
   existing `herdr_breaker` doctor shape verbatim — one breaker, one doctor
   field, shared with AQ2.6's immediate steer path since the breaker is
   per-host, not per-caller). No event calls a mailbox message delivered or
   read merely because a prompt was accepted; only `atm read` clearing the
   marker means read. Publish the list→prompt race and the absence of a
   native Herdr queue in ADR-054's addendum.

## Acceptance criteria

1. **FIFO per member.** Three queued messages to one Herdr member observed
   `idle` on tick 1 produce exactly one prompt on tick 1 (the oldest,
   `claim_next_pending`); the second is claimed and prompted only on a
   later tick where that member is freshly observed `idle` again (its
   working/blocked ticks in between produce no claim). `agent read`
   clearing the marker mid-backlog is unaffected — the pump only ever acts
   on what `list_pending_members` currently reports.
2. **Burst cap.** With more than `HERDR_MAX_PROMPTS_PER_TICK` (16) distinct
   Herdr members simultaneously idle and pending in one tick, exactly 16
   are claimed-and-prompted; the rest are claimed-then-`release_pending`'d
   with no attempt-count change and are prompted (subject to a fresh idle
   re-check) on the next tick. A fixture proves the cap is enforced
   host-wide, across sessions, not per-session.
3. **Session grouping.** Two eligible Herdr members configured with
   different `HerdrSession`s produce exactly two `agent list` child
   invocations in one tick, each with a distinct `HERDR_SESSION` value on
   its child environment (or unset, for the `None`/default-server bucket);
   a fixture with a fake `HerdrProcessAdapter::list` records both argvs and
   environments and asserts no third call and no cross-session leakage.
4. **Shutdown.** With a `list`/`prompt` child in flight (bounded by D10's
   5 s bound), triggering the runtime's shutdown watch stops further ticks
   from starting and the pump's own join completes within the daemon
   shutdown deadline via the same `tokio::time::timeout` mechanism
   `HttpRuntime` uses (`lib.rs:853`); no orphaned child or task survives.
5. **Breaker interaction.** A fake `HerdrProcessAdapter::list` returning
   `server_not_running` opens the shared breaker (ADR-058 D10.1); the
   fixture proves: no claim is taken for the affected session while open
   (nothing to release), subsequent ticks skip the `list` spawn entirely
   until `retry_after`, and the first successful `list` after `retry_after`
   closes the breaker — reusing AQ2.6 AC 11's extension fixture shape, not
   a second breaker implementation.
6. A `blocked`-race fixture: the tick's `list` observes a member `idle`,
   the member enters a blocked dialog before the subsequent `prompt`
   lands; `agent prompt` returns `agent_blocked`; the exact claim is
   released via `release_pending`, its attempt count is unchanged, and the
   fixture records zero injected bytes. A second fixture distinguishes this
   from `agent_not_found`/`agent_target_ambiguous`/`agent_not_ready` on the
   same post-claim prompt call, which also use `release_pending` (not
   `requeue_pending` — Steer/Queue-kind Herdr dispatch has no independent
   retry counter, AQ1 §1.4).
7. A not-present fixture: a pending Herdr member absent from its session's
   `agent list` result produces no claim, emits `held_target_not_present`,
   retains the marker, and is re-evaluated (no special backoff state
   beyond the normal `HERDR_POLL_INTERVAL_MS` cadence — there is no
   `target_recheck_interval` in this design) on the next tick.
8. Dispatch for a claimed prompt goes through `rebuild_received_hook_dispatch(
   ..., NudgeKind::Queue)` and the injected `MessageReceivedHookSelector`,
   never a private/duplicated reference to `HerdrReceivedHook` or to
   `atm_herdr::HerdrProcessAdapter::prompt` directly — a fixture over a fake
   selector proves the pump asks the selector, and that AQ2.6's `(Queue,
   LocalSteer(Herdr(_)))` arm is what resolves it.
9. A fake implementation of `atm_herdr::HerdrProcessAdapter` (behind
   `atm-herdr`'s `test-utils` feature, not the real `HerdrProcessInvoker`,
   consistent with AQ2.6's `forbidden_test_bypasses` rule) is the sole test
   double exercising the pump's `list` calls below the adapter boundary.
   `HerdrProcessAdapter::wait` is never called by any fixture in this
   sprint — a fixture asserts the fake's `wait` implementation is never
   invoked, proving the "defined but unused" claim mechanically rather than
   aspirationally.
10. **Poll-to-heartbeat feed.** After one tick observing a Herdr member as
    `idle`, `atm doctor --json` shows that member's runtime state as `Idle`
    with `state_changed_by: "herdr_poll"`, sourced through the same
    `RuntimeHealth` snapshot harness heartbeats populate; a fixture proves
    the `idle|done → Idle`, `working|blocked → Active`, and
    `unknown`/absent-from-list → no-write mappings, and that an existing
    observation is left untouched when a later poll cannot classify the
    member. This sprint's own suite does **not** assert that
    `MemberStateTransitionSink::on_transition` fires for a Herdr member —
    that trait does not exist until AQ3 lands; the convergence claim
    (Non-closure) is validated once AQ3's kind-agnostic drain follow-up
    lands, not by this sprint.

## Required validation

- Live macOS/Linux transcript: queue two messages for an initially working
  Herdr agent; observe the poll holding (no prompt while `working`), then
  one prompt once `list` observes `idle`, then read both durable mailbox
  messages. The transcript labels the observed list→prompt race as an
  accepted limitation rather than an idle-delivery guarantee.
- Blocked-dialog transcript or deterministic Herdr fixture: prove rejected
  prompt, zero injected bytes, and marker release without retry debt.
- Missing-agent fixture: prove no queue dispatch occurs when a pending
  member is absent from `agent list`, the marker remains, and
  `held_target_not_present` is observable.
- Session-grouping fixture: two Herdr members in two different sessions
  produce two `agent list` children with distinct `HERDR_SESSION` values in
  one tick.
- Burst-cap fixture: more than 16 simultaneously idle+pending Herdr members
  in one tick produce exactly 16 prompts and 
  `count - 16` claim-then-release cycles with no attempt-count change.
- Heartbeat-feed fixture: `atm doctor --json` reflects a polled Herdr
  member's state after one tick, tagged `herdr_poll`.
- Regression test: AQ3 continues to drain tmux/graft only (its channel
  pre-check still skips `HerdrSteer`), while Herdr queue work is owned only
  by this pump.

## Non-closure / out of scope

- A Herdr-native queue, atomic idle-and-send operation, per-turn tracking,
  or priority/re-nudge policy. `agent prompt` is fire-and-forget; only this
  pump gates it on a poll observation.
- Changing immediate steer behavior from AQ2.6: immediate Herdr steer may
  prompt a working agent; queue is the poll-gated policy.
- Any tmux removal, legacy-daemon work, mailbox persistence redesign, or
  bare-CLI FIFO change.
- A CLI-facing override for `HERDR_POLL_INTERVAL_MS`/
  `HERDR_MAX_PROMPTS_PER_TICK` (code-level production defaults in this
  sprint; an operator knob is a follow-up if the defaults prove wrong in
  practice).
- **Making AQ3's idle-drain channel pre-check kind-agnostic** (i.e.,
  admitting `HerdrSteer` instead of skipping it) so that deliverable 4's
  poll-to-heartbeat feed can drive AQ3's existing `MemberStateTransitionSink`
  hook for Herdr members and this sprint's own deliverable 2/3 tick-drain
  can be deleted. This is explicitly **not** committed scope for this
  sprint or for AQ3 as currently planned — it is recorded here as a
  pointer for whichever future sprint picks up the shared-drain
  consolidation, and as the reason deliverable 2's tick-drain is kept small
  and structured (so that consolidation is a deletion, not a rewrite).
- `HerdrQueueWakePumpConfig`, the semaphore-bounded per-member task model,
  and the 45-minute `agent wait` timeout — all deleted by this rewrite, not
  carried forward in any form.

## Dependencies

- must_follow: AQ1 (trait foundation: `PendingNudgeStore` incl.
  `release_pending`, `list_pending_members`, dispatch-from-message-id;
  `DeliveryChannel` classifier; `RosterStore::list_teams`/`load_roster`).
- must_follow: AQ2.6 (Herdr emitter, the extended `select_emitter` Queue+Herdr
  arm, and the `atm-herdr` crate — `HerdrProcessAdapter`, `HerdrProcessInvoker`,
  `HerdrSpawnBreaker` — this sprint imports directly for `agent list` and
  indirectly, via the selector, for `agent prompt`; `agent wait` stays
  defined on the trait per ADR-058 D2 but this sprint never calls it).
  Merge-forward trigger: AQ2.6 dev push. See also
  `docs/atm-herdr/{requirements.md,architecture.md,boundaries.md}`
  (authored alongside AQ2.6; this sprint's `atm-http-runtime -> atm-herdr`
  dependency edge must match `boundaries/atm-herdr/herdr-process-adapter.toml`'s
  `allowed_dependents`).
- Removed 2026-08-26 (reorder per Rand): `must_follow AQ2.5` (classifier now
  AQ1's) and `must_follow AQ3` (this pump carries its own Herdr-only guard;
  AQ3 lands after and already carries its own skip-Herdr pre-check — see
  above).
- parallel_safe: AQ1.5–AQ1.9 (disjoint files).
- Resolved for the poll-based rewrite round (Rand, 2026-08-26): pump
  concurrency model / head-of-line blocking behind a 45-min `agent wait`
  (prior I17) — eliminated by construction (no `wait`, one tick, one `list`
  per session, deliverable 2); adapter crate placement (prior I18) —
  resolved to `atm-herdr` (structural change, deliverable 1; not
  `atm-http-runtime`, and not `atm-core`), see AQ2.6 deliverable 3.
