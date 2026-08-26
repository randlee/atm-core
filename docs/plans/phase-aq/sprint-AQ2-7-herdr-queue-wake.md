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
Tmux and graft remain AQ3's sole claim paths; AQ3 explicitly skips Herdr, so
two workers never claim a Herdr message.

The best available Herdr composition is a gate, not an atomic primitive:

```text
herdr agent wait <AgentName> --until idle --until done --until blocked --timeout <bounded>
# only idle/done continues
herdr agent prompt <AgentName> "You have unread ATM messages. Run: atm read"
```

There is a race between `wait` returning and `prompt` reaching the terminal:
the agent can begin a new turn in that interval. The plan must preserve that
truth in events, docs, and tests. `agent prompt`'s own `agent_blocked`
rejection is the final guard against injecting into a newly blocked dialog;
the design promises lifecycle-aware refusal, not delivery exactly at idle or
turn-correlated queueing.

## Deliverables

1. **Detached Tokio pump, no hidden queue.** Compose `HerdrQueueWakePump` in
   the replacement Tokio/Axum runtime beside AQ3's joined maintenance work.
   The sender detaches after durable queue admission; its request deadline is
   never inherited by this pump. The idle gate may wait for a long configured
   period (including 45 minutes) without blocking the send path. It considers
   only `DeliveryChannel::HerdrSteer` and only queue-kind pending markers. It
   owns no mailbox rows, FIFO, retry count, or per-message detached task. At
   each pass it handles at most one oldest pending message per member;
   shutdown cancels/reaps an active wait child and joins the pump under the
   daemon deadline. The legacy synchronous daemon is out of scope and must
   not be touched.

2. **Lifecycle gate and claim order.** For a member with pending work, derive
   the Herdr target directly from the member's `AgentName` (AQ2.6's launch
   convention); do not load, resolve, or retry a persisted Herdr target.
   Invoke AQ2.6's Herdr process adapter for `agent wait` with that AgentName
   and the detached pump's
   long configured timeout and accept only returned `idle` or `done`. A
   returned `blocked`, timeout (including an agent that remains
   unclassifiable/`unknown`, because this gate does not accept `--until
   unknown`), unavailable/not-found live agent, or malformed output produces a structured
   held/deferred observation and performs **no claim and no prompt**. On
   `idle`/`done`, atomically call AQ1's `claim_next_pending`, then dispatch
   that exact claim through the normal message-received selector to
   `HerdrReceivedHook`. The sender's original body is never passed to Herdr;
   the fixed mailbox-read prompt is the only terminal input.

3. **Blocked-race release without retry debt.** If the post-claim prompt is
   rejected with `agent_blocked`, call AQ1's new
   `PendingNudgeStore::release_pending(member, claim)`: restore exactly that
   marker without incrementing `nudge_attempts`. The rejection occurred
   before input injection, so `requeue_pending` would incorrectly consume
   retry budget. Other post-claim start/timeout/protocol failures use the
   existing `requeue_pending` path and bounded retry/stuck policy. A successful
   prompt completes the claim. Conditional claim identity prevents a later
   release from restoring a message that another worker has already handled.

4. **Honest lifecycle observability.** Emit backend-qualified events with
   `{member, msg_id when claimed, queue_kind, observed_state, outcome}` for
   `wait_idle`, `wait_done`, `held_blocked`, `held_unknown_or_timeout`,
   `prompted`, `blocked_before_input_released`, and `dispatch_failed_requeued`.
   Add health counters for held, released, and requeued work. No event calls
   a mailbox message delivered or read merely because Herdr returned `done`;
   `done` is only an admissible gate state. Publish the wait→prompt race and
   the absence of a native Herdr queue in ADR-054's addendum.

## Acceptance criteria

1. With a queued Herdr member that is working, the detached pump sends no
   prompt until its long configured idle gate observes `idle` or `done`; mail
   is nevertheless immediately obtainable with `atm read` throughout. A
   sender completes queue admission while that gate is held (including a
   45-minute fixture), proving the send path never awaits it.
2. A `blocked` lifecycle result produces no claim, no terminal input, and no
   retry-attempt change. Because the gate does not accept `--until unknown`,
   an unclassifiable/`unknown` agent reaches the held timeout path and is
   never accepted as completion or prompted.
   An unavailable/not-found Herdr `AgentName` takes the same held path; it is
   not retried through a stored target or an alternate backend.
3. A deterministic race fixture has the agent enter a blocked dialog after
   wait succeeds but before prompt. Herdr returns `agent_blocked`; the exact
   claim is released, its attempt count is unchanged, and the fixture records
   no input bytes.
4. A normal prompt failure after a claim uses `requeue_pending`, increments
   the attempt once, and eventually reaches AQ1's existing bounded stuck
   signal; no Herdr-specific retry counter exists.
5. Two pending queue messages for one Herdr member result in at most one
   prompt per pump pass, in AQ1 FIFO order. AQ3's sweep double, when presented
   the same `HerdrSteer` member, makes no claim or attempt mutation.
6. Cancellation while a wait child is live reaps the child and joins the pump
   under the daemon deadline. `just test`, daemon integration tests,
   boundary-manifest checks, and the ADR-054 addendum gate pass.

## Required validation

- Live macOS/Linux transcript: queue two messages for an initially working
  Herdr agent; observe one lifecycle-gated wake, then read both durable
  mailbox messages. The transcript labels the observed wait→prompt race as an
  accepted limitation rather than an idle-delivery guarantee.
- Blocked-dialog transcript or deterministic Herdr fixture: prove rejected
  prompt, zero injected bytes, and marker release without retry debt.
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

## Dependencies

- must_follow: AQ1 (`PendingNudgeStore`, including this sprint's
  `release_pending` lifecycle-release contract).
- must_follow: AQ2.5 (delivery classifier and queue taxonomy).
- must_follow: AQ2.6 (explicit `HerdrSteer` selection and the shared Herdr
  process/emitter adapter). Merge-forward trigger: AQ2.6 dev push.
- must_follow: AQ3 (AQ3 owns the shared recovery-sweep pre-check and lands
  the "skip Herdr" protection before this pump is enabled). Merge-forward
  trigger: AQ3 dev push.
- parallel_safe: none claimed; the new pump deliberately starts only after
  the existing scheduler's ownership boundary is in place.
