# Sprint AQ2.7 — Queue: Herdr Lifecycle-Gated Mailbox Wake-Up

Status: draft · Branch: `feature/aq-2-7-herdr-queue-wake` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Implement deferred queue wake-ups for AQ2.6's `HerdrSteer` members without
pretending that Herdr supplies a queue. The durable ATM mailbox is the queue:
mail remains immediately readable through `atm read`, while AQ1's pending
marker says that a wake-up is deferred. This sprint adds one detached,
Herdr-only Tokio pump that waits for an acceptable lifecycle observation and
then asks the existing Herdr emitter to send the same mailbox-read prompt by
the member `AgentName`.

Tmux and graft remain AQ3's claim paths. **This pump owns its own
Herdr-only guard** (claims only members AQ1's classifier reports as
`HerdrSteer`, discovered via `list_pending_members`), and AQ3 — which lands
later in the 2026-08-26 order — adds the mirror-image skip-Herdr pre-check on
**both** its idle drain and its recovery sweep (critical review B8: the drain
was previously unguarded). Two workers never claim a Herdr message only when
both guards exist; until AQ3 lands, this pump is the only claimant for any
member and the risk is nil.

This sprint's Herdr behaviour claims are governed by
[ADR-058](./ADR-058-draft.md) (`herdr` 0.8.2, derived from source at
`d79fd746`). Where this doc and ADR-058 disagree, ADR-058 is authoritative;
this doc cites it by decision id (`D1`–`D8`).

The best available Herdr composition is a gate, not an atomic primitive:

```text
herdr agent wait <AgentName> --until idle --until done --until blocked --timeout <bounded>
# only idle/done continues
herdr agent prompt <AgentName> "You have unread ATM messages. Run: atm read"
```

There is a race between `wait` returning and `prompt` reaching the terminal:
the agent can begin a new turn in that interval. The plan must preserve that
truth in events, docs, and tests (ADR-058 D7 — this text is acknowledged and
kept verbatim from the prior draft). `agent prompt`'s own `agent_blocked`
rejection is the final guard against injecting into a newly blocked dialog;
the design promises lifecycle-aware refusal, not delivery exactly at idle or
turn-correlated queueing.

## Deliverables

1. **Detached Tokio pump, no hidden queue; bounded per-member concurrency
   (critical review I17).** Compose `HerdrQueueWakePump` as a type in
   `atm-http-runtime` (beside `StorageAndNudgeRouter`/`HttpRuntime`,
   `crates/atm-http-runtime/src/lib.rs`), the same crate AQ3's joined
   maintenance work lands in; `atm-daemon-bootstrap` constructs and spawns it
   at daemon startup from `build_replacement_handler`
   (`atm-daemon-bootstrap/src/lib.rs:178-201`), the same function that already
   builds `active_received_hook_selector` and `StorageAndNudgeRouter`. The
   sender detaches after durable queue admission; its request deadline is
   never inherited by this pump.

   **Concurrency model (decided): one Tokio task per pending Herdr member,
   gated by a bounded `tokio::sync::Semaphore`.** A low-frequency outer tick
   (a `tokio::spawn`ed loop, cadence matching AQ3's "maintenance-cadence
   precedent") calls `PendingNudgeStore::list_pending_members()`, filters to
   members `classify_delivery_channel` (via AQ1's classifier over the
   member's current roster row) resolves to `DeliveryChannel::HerdrSteer`,
   and spawns one task per member not already tracked in an in-memory
   `HashMap<MemberKey, JoinHandle<()>>` (own state — not durable, rebuilt from
   `list_pending_members` on daemon restart, matching AQ1/AQ3's "recovery
   sweep is the correctness backstop" posture). Each task first acquires a
   permit from a shared `Arc<Semaphore>` before calling `agent wait`, so at
   most **N** `agent wait` calls (each up to the configured wait timeout,
   including 45 minutes) run concurrently; a member beyond the bound queues
   for a permit instead of blocking any other member's task, and — critically
   — never blocks the send path, which is on a different task entirely and
   never touches this semaphore.

   This is a **new** typed config struct, `HerdrQueueWakePumpConfig`
   (mirroring the existing `RuntimeLimits`/`RuntimeTimeouts`/`NonZeroDuration`
   pattern at `crates/atm-http-runtime/src/lib.rs:277-327`, not a raw env
   var — no other daemon runtime knob in this codebase is env-var-configured,
   `RuntimeLimits`/`RuntimeTimeouts` are the established idiom):

   ```rust
   pub struct HerdrQueueWakePumpConfig {
       max_concurrent_waits: usize,       // semaphore bound; production default 8
       wait_timeout: Duration,            // --timeout passed to `agent wait`; production default 45 min
       target_recheck_interval: Duration, // recheck cadence after held_target_not_present; production default 10 min
   }
   ```

   **Flag to Rand for confirmation:** the bound of **8** concurrent waits and
   the **10-minute** target-not-present recheck interval are this sprint's
   proposed production defaults, not derived from any existing repo
   precedent — there is no prior Herdr fleet-size or wait-concurrency data to
   size them from. `HerdrQueueWakePumpConfig` has no CLI-facing override in
   this sprint (deliberately out of scope, tunable later); only a
   `production_default()` constructor is authored, called from
   `build_replacement_handler`.

   It considers only `DeliveryChannel::HerdrSteer` and only queue-kind
   pending markers. It owns no mailbox rows, FIFO, retry count, or per-message
   detached task — retry/FIFO stay AQ1's store; this pump owns only the
   lifecycle gate and the claim/dispatch call. At each pass through a
   member's task, it handles at most one oldest pending message for that
   member before looping back to re-check `list_pending_members` membership
   (so a member with one message does one wait+prompt and its task then
   exits; a backlog keeps the task alive, one message per satisfied gate).
   After one observed live-agent-not-found result (`held_target_not_present`,
   deliverable 2), retain the marker but move that member's task to the
   longer `target_recheck_interval` sleep instead of repeatedly running the
   normal blocked/timeout `agent wait` loop; surface the state on health as
   target-not-present, distinct from held/blocked/timeout.

   **Shutdown.** The pump holds a clone of `HttpRuntime`'s existing
   `watch::Receiver<()>` shutdown signal (`lib.rs:380-387`, the same
   primitive `HttpRuntime::begin_shutdown`/`:822-823` sends on). On receipt:
   stop spawning new member tasks; for each live per-member `JoinHandle`,
   `abort()` it — ADR-058 D5's "Cancellation" paragraph confirms killing the
   `herdr` child mid-`wait` is a clean cancel with no side effects and no
   input ever written, so an abort mid-wait is safe by the same reasoning
   `AsyncMessageReceivedHookEmitter`'s doc comment already relies on
   ("Dropping the future is the cancellation signal",
   `boundary/message_received_hook_emitter.rs:29-34`). The pump then awaits
   all aborted/finishing handles under `tokio::time::timeout(<daemon shutdown
   deadline>, ...)`, mirroring `HttpRuntime::finish_shutdown`'s
   `tokio::time::timeout(self.config.timeouts.shutdown, &mut server_task)`
   (`lib.rs:853`) — this pump does not invent a second shutdown-deadline
   source. The legacy synchronous daemon is out of scope and must not be
   touched.

2. **Lifecycle gate and claim order.** For a member with pending work, derive
   the Herdr target directly from the member's `AgentName` and its stored
   `session` (AQ2.6's `LocalMessageReceivedBackend::Herdr { session }`); do
   not load, resolve, or retry a persisted Herdr *target* — the session is
   the only stored routing datum, per AQ2.6. Invoke AQ2.6's named
   `HerdrProcessAdapter::wait` (trait in
   `crates/atm-http-runtime/src/herdr_process.rs`, same crate this pump
   lives in — no cross-crate reach needed) via the shared
   `HerdrProcessInvoker` instance injected at construction, with
   that `AgentName`, that `session`, and the pump's `wait_timeout` from
   `HerdrQueueWakePumpConfig` — **`--timeout` is always passed explicitly**
   (ADR-058 D5: "Without `--timeout` the wait is indefinite... atm-core
   always passes `--timeout`").

   **Exit-0-with-`blocked` is success, not an error (ADR-058 D5, explicit
   correction from the prior draft).** `agent wait`'s `--until` set
   (`idle`, `done`, `blocked`) means Herdr returns **exit 0** with
   `result.agent.agent_status` set to whichever of those three states was
   observed — `blocked` is a normal successful wait outcome, not a
   `timeout`/error exit. The pump must parse `agent_status` from the exit-0
   JSON body, not branch on exit code alone:

   - `agent_status == "idle" | "done"` → atomically call AQ1's
     `claim_next_pending`, then dispatch that exact claim through
     `rebuild_received_hook_dispatch(runtime, member, claim.msg,
     NudgeKind::Queue)` (AQ1's `nudge_dispatch.rs`) and the injected
     `Arc<dyn MessageReceivedHookSelector>` (the same selector instance
     `active_received_hook_selector` builds and `StorageAndNudgeRouter`
     already holds, `storage_and_nudge_router.rs:95/109`) — AQ2.6's
     extended `select_emitter` routes `(Queue, LocalSteer(Herdr(_)))` to
     `HerdrReceivedHook`, so this sprint calls the selector, never a
     private reference to the emitter. No claim and no prompt happen before
     this branch.
   - `agent_status == "blocked"` (exit 0) → structured `held_blocked`
     observation; **no claim, no prompt**. Because `--until unknown` is
     never passed, an agent that stays `unknown` for the whole wait reaches
     the `timeout` error exit below, not a success exit — `unknown` is never
     accepted as completion or prompted.
   - `error.code == "timeout"` (exit 1) → `held_unknown_or_timeout`; no
     claim, no prompt.
   - `error.code == "agent_not_found"` (initial probe) **or**
     `"agent_not_running"` (mid-wait, ADR-058 D5) → **both** map to
     `held_target_not_present`: no claim was ever taken at this point (the
     wait step runs strictly before `claim_next_pending`), so there is
     nothing to release — this is distinct from, and pre-claim relative to,
     deliverable 3's post-claim `release_pending` path (reserved exclusively
     for `agent_blocked` on the *prompt* call after a claim). Retain the
     marker, perform no claim/prompt, and enter deliverable 1's longer
     `target_recheck_interval`.
   - `error.code == "agent_target_ambiguous"` (wait) → advisory failure, no
     retry (ADR-058 D8): logged, no claim, member's task continues on the
     normal cadence (an operator name collision, not a transient condition
     the recheck interval helps with).
   - `error.code` ∈ `{server_not_running, protocol_mismatch}` → advisory
     failure, health counter `herdr_unavailable`; no claim.

   The sender's original body is never passed to Herdr; the fixed
   mailbox-read prompt is the only terminal input.

3. **Blocked-race release without retry debt.** If the post-claim `prompt`
   call (via `HerdrReceivedHook`, invoked as above) is rejected with
   `agent_blocked`, call AQ1's `PendingNudgeStore::release_pending(member,
   claim)`: restore exactly that marker without incrementing
   `nudge_attempts`. The rejection occurred before input injection, so
   `requeue_pending` would incorrectly consume retry budget. **Every other**
   post-claim `prompt` failure (`agent_not_found`, `agent_target_ambiguous`,
   `agent_not_ready`, `agent_prompt_failed`, `empty_agent_prompt`,
   `internal_error`, `server_unavailable`, `server_not_running`,
   `protocol_mismatch` — ADR-058 D8's prompt column, mirroring AQ2.6
   deliverable 3's table but now via `requeue_pending` because a claim was
   already taken) uses the existing `requeue_pending` path and AQ1's bounded
   retry/stuck policy (`MAX_NUDGE_ATTEMPTS`). A successful prompt completes
   the claim. Conditional claim identity (AQ1's `NudgeClaim` equality)
   prevents a later release from restoring a message that another worker has
   already handled.

4. **Honest lifecycle observability.** Emit backend-qualified events with
   `{member, msg_id when claimed, queue_kind, observed_state, outcome}` for
   `wait_idle`, `wait_done`, `held_blocked`, `held_unknown_or_timeout`,
   `held_target_not_present`, `held_ambiguous`, `prompted`,
   `blocked_before_input_released`, and `dispatch_failed_requeued`. Add
   health counters for held, released, requeued, and target-not-present
   work, with target-not-present distinct from blocked/timeout rechecks, and
   a gauge for in-flight `agent wait` calls against
   `HerdrQueueWakePumpConfig.max_concurrent_waits` (so semaphore contention
   is observable, not just inferred). No event calls a mailbox message
   delivered or read merely because Herdr returned `done`; `done` is only an
   admissible gate state. Publish the wait→prompt race and the absence of a
   native Herdr queue in ADR-054's addendum.

## Acceptance criteria

1. With a queued Herdr member that is working, the detached pump's
   per-member task sends no prompt until its `agent wait` call — bounded by
   `HerdrQueueWakePumpConfig.wait_timeout` — observes `idle` or `done`
   (exit 0, parsed from `agent_status`, not inferred from exit code alone);
   mail is nevertheless immediately obtainable with `atm read` throughout. A
   sender completes queue admission while that gate is held (including a
   45-minute fixture), proving the send path never awaits it and never
   touches the pump's semaphore.
2. A `blocked` `agent_status` on a **successful** (exit 0) `agent wait`
   produces no claim, no terminal input, and no retry-attempt change — a
   fixture asserts the pump distinguishes this from the `timeout` error exit.
   Because the gate does not accept `--until unknown`, an
   unclassifiable/`unknown` agent reaches the held timeout path and is never
   accepted as completion or prompted. `agent_not_found` (initial probe) and
   `agent_not_running` (mid-wait) both emit `held_target_not_present`
   pre-claim, retain the marker without claim/prompt, and move the member's
   task to `target_recheck_interval` rather than the blocked/timeout loop, a
   stored target, or an alternate backend.
3. A deterministic race fixture has the agent enter a blocked dialog after
   wait succeeds but before prompt. Herdr's `agent prompt` call returns
   `agent_blocked`; the exact claim is released via `release_pending`, its
   attempt count is unchanged, and the fixture records no input bytes. This
   is distinguished by a second fixture from `agent_not_found`/
   `agent_target_ambiguous` on the post-claim prompt call, which instead uses
   `requeue_pending`.
4. A normal prompt failure after a claim uses `requeue_pending`, increments
   the attempt once, and eventually reaches AQ1's existing bounded stuck
   signal; no Herdr-specific retry counter exists.
5. Two pending queue messages for one Herdr member result in at most one
   prompt per satisfied gate, in AQ1 FIFO order, with the member's task
   looping back for the second message only after the first completes.
   `max_concurrent_waits` bounds total concurrent `agent wait` calls across
   members (fixture: `max_concurrent_waits = 1` with two eligible Herdr
   members proves the second member's task queues for the semaphore
   permit rather than running its `agent wait` concurrently). AQ3's sweep
   double, when presented the same `HerdrSteer` member, makes no claim or
   attempt mutation.
6. Cancellation while a wait child is live aborts the member's task, reaps
   the `herdr` child (ADR-058 D5's clean-cancel guarantee) with no prompt
   emitted, and the pump's own shutdown join completes within the daemon
   shutdown deadline via the same `tokio::time::timeout` mechanism
   `HttpRuntime` uses (`lib.rs:853`). `just test`, daemon integration tests,
   boundary-manifest checks, and the ADR-054 addendum gate pass.
7. A not-found fixture proves the pump does one live `AgentName` lookup
   (the initial `agent.get` inside `agent wait`), emits
   `held_target_not_present` (not `held_blocked`), retains the marker
   without claim/prompt, and schedules `target_recheck_interval` rather than
   indefinitely retrying the normal loop.
8. Dispatch for the post-claim prompt goes through
   `rebuild_received_hook_dispatch(..., NudgeKind::Queue)` and the injected
   `MessageReceivedHookSelector`, never a private/duplicated reference to
   `HerdrReceivedHook` — a fixture over a fake selector proves the pump asks
   the selector, not a hard-coded emitter handle, and that AQ2.6's
   `(Queue, LocalSteer(Herdr(_)))` arm is what resolves it.
9. A fake implementation of `HerdrProcessAdapter` (not the real
   `HerdrProcessInvoker`, consistent with AQ2.6's `forbidden_test_bypasses`
   rule) is the sole test double exercising the pump's `wait` calls below
   the adapter boundary.

## Required validation

- Live macOS/Linux transcript: queue two messages for an initially working
  Herdr agent; observe one lifecycle-gated wake, then read both durable
  mailbox messages. The transcript labels the observed wait→prompt race as an
  accepted limitation rather than an idle-delivery guarantee.
- Blocked-dialog transcript or deterministic Herdr fixture: prove rejected
  prompt, zero injected bytes, and marker release without retry debt.
- Missing-agent fixture: prove no queue dispatch occurs, the marker remains,
  `held_target_not_present` is observable, and the next lookup uses
  `target_recheck_interval`.
- Concurrency fixture: with `max_concurrent_waits` set low (e.g. 1) and
  multiple eligible Herdr members pending, observe queued permits rather
  than concurrent `agent wait` processes, and confirm the send path's
  latency is unaffected regardless of pump saturation.
- Regression test: AQ3 continues to drain tmux/graft only, while Herdr queue
  work is owned only by this pump.

## Non-closure / out of scope

- A Herdr-native queue, atomic idle-and-send operation, per-turn tracking, or
  priority/re-nudge policy. `agent prompt` is fire-and-forget; only this
  detached pump waits for lifecycle observation.
- Changing immediate steer behavior from AQ2.6: immediate Herdr steer may
  prompt a working agent; queue is the lifecycle-gated policy.
- Any tmux removal, legacy-daemon work, mailbox persistence redesign, or
  bare-CLI FIFO change.
- A CLI-facing override for `HerdrQueueWakePumpConfig` (bound/timeouts are
  code-level production defaults in this sprint; an operator knob is a
  follow-up if the defaults prove wrong in practice).

## Dependencies

- must_follow: AQ1 (trait foundation: `PendingNudgeStore` incl.
  `release_pending`, `list_pending_members`, dispatch-from-message-id;
  `DeliveryChannel` classifier).
- must_follow: AQ2.6 (Herdr emitter, the extended `select_emitter` Queue+Herdr
  arm, and the named `HerdrProcessAdapter`/`HerdrProcessInvoker` in
  `atm-http-runtime` this pump invokes directly for `agent wait` and
  indirectly, via the selector, for `agent prompt`). Merge-forward trigger:
  AQ2.6 dev push.
- Removed 2026-08-26 (reorder per Rand): `must_follow AQ2.5` (classifier now
  AQ1's) and `must_follow AQ3` (this pump carries its own Herdr-only guard;
  AQ3 lands after and adds its own skip-Herdr pre-check — see above).
- parallel_safe: AQ1.5–AQ1.9 (disjoint files).
- Resolved for the rewrite round (critical review): pump concurrency model /
  head-of-line blocking behind a 45-min `agent wait` (I17) — one task per
  member, bounded `Semaphore`, see deliverable 1; adapter crate placement
  (I18) — `atm-http-runtime` (not `atm-core`: `atm-daemon-bootstrap` already
  depends on `atm-http-runtime`, so no new crate edge is needed), see AQ2.6
  deliverable 3.
