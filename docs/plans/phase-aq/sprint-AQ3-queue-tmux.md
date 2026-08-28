# Sprint AQ3 — Queue: Tmux Idle-Drain

Status: implementation complete · Branch: `feature/aq-3-queue-tmux` off
`integrate/phase-aq` · Worktree: `atm-core-worktrees/feature/aq-3-queue-tmux` ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

The tmux received-hook side of queue: maintain the per-recipient pending
FIFO (derived — AQ1) and, when the harness transitions to idle, nudge the
NEXT unread queued message. One nudge per idle transition; a backlog drains
at the harness's own pace.

Verified baseline: harness state arrives as `TeamMemberHeartbeatRequest`
(`HeartbeatActivity: ActiveToolUse | Idle | SessionEnded`,
protocol.rs:330-342) recorded by `RuntimeHealth.record_heartbeat()`
(runtime_health.rs:101-158) into an in-memory map — explicitly
"observability, not durable state", polling-only, no transition events
today. Activity mapping: ActiveToolUse→Active, Idle→Idle,
SessionEnded→Offline.

## Deliverables

1. **Idle-transition hook**: `RuntimeHealth.record_heartbeat()` gains a
   transition notification firing on transitions to `Idle`.
   **Trait + boundary record**: `MemberStateTransitionSink` is defined in
   `crates/atm-http-runtime/src/runtime_health.rs` beside `RuntimeHealth`,
   re-exported from `crates/atm-http-runtime/src/lib.rs`, sealed via
   `atm_core::boundary::sealed::Sealed` — the same supertrait
   `CanonicalWriteHandler` already uses in this crate
   (`message_handler.rs:64`, ADR-001 pattern), not a new local seal. It
   gets a new manifest,
   `boundaries/atm-http-runtime/member-state-transition-sink.toml`
   (fixed/internal).

```rust
pub trait MemberStateTransitionSink: atm_core::boundary::sealed::Sealed + Send + Sync {
    fn on_transition(&self, member: &atm_core::boundary::MemberKey,
                     from: RuntimeMemberState, to: RuntimeMemberState);
}
```

   **Key conversion point (no `From` impl needed)**: the sink's `member`
   parameter is AQ1's public `atm_core::boundary::MemberKey`, never the
   private `runtime_health::MemberKey` (`runtime_health.rs:43`) — the two
   types are never in the same scope, so no conversion function exists or
   is needed. `record_heartbeat` captures the record's state *before*
   overwriting it and returns whether this call landed a genuine
   transition into `Idle` (previous state != `Idle`, next state ==
   `Idle`) alongside its existing `TeamMemberHeartbeatResponse`. Its
   caller — the `heartbeat()` handler in
   `crates/atm-http-runtime/src/storage_and_nudge_router.rs` (~487-510)
   — already holds `request.team`/`request.member` (the exact
   `TeamName`/`AgentName` pair `record_heartbeat` uses internally to
   build the private key), so on a landed transition it builds
   `atm_core::boundary::MemberKey::new(request.team.clone(),
   request.member.clone())` from those same fields and calls
   `self.member_state_transition_sink.on_transition(&member_key, from,
   RuntimeMemberState::Idle)`. Execution contract: this call happens
   **strictly after `record_heartbeat` has returned** — its internal
   `MutexGuard` is already dropped, so `on_transition` never runs inside
   the critical section its module doc scopes to in-memory fields — and
   the storage claim it triggers runs under `spawn_blocking`/a bounded
   blocking pool, never inline on the heartbeat-handling task.
   `RuntimeHealth`'s module doc is updated (best-effort sink,
   non-authoritative; recovery sweep is the correctness backstop).
   **Wiring**: `StorageAndNudgeRouter` gains a
   `with_member_state_transition_sink(...)` builder step mirroring
   `with_runtime_health` (`storage_and_nudge_router.rs:145-148`), set at
   composition time.
   **Impl owner**: the concrete sink needs `PendingNudgeStore` (via
   `service_runtime.pending_nudge_store()`), the
   `MessageReceivedHookSelector`, and `rebuild_received_hook_dispatch` —
   all daemon-composition concerns — so it is implemented and constructed
   in `crates/atm-daemon-bootstrap/src/lib.rs`'s
   `run_replacement_daemon_with_selector` (defined at line 640; today it
   constructs `RuntimeHealth::with_owner` at line 653 and calls
   `build_replacement_handler`, whose `selector_factory(...)` call sits at
   line ~504 — the AQ2.5 doc's "~lib.rs:217" anchor for this same
   composition root is stale against the verified baseline and should
   read these line numbers instead), beside where AQ2.5 composes
   `BareCliFifo`. Name it `struct DrainOnTransitionSink { runtime:
   atm_core::LocalServiceRuntime, selector: Arc<dyn
   MessageReceivedHookSelector> }`, `impl MemberStateTransitionSink for
   DrainOnTransitionSink`.

2. **Drain step**: on an idle transition for member M, first apply the
   channel pre-check (defined once below, shared verbatim with deliverable
   3), then call AQ1's `PendingNudgeStore::claim_next_pending(&member)` —
   the backend-side atomic select-and-claim is THE at-most-once mechanism,
   shared verbatim with the recovery sweep. On `Some(claim)`, rebuild the
   dispatch for exactly that message-id via AQ1's
   `rebuild_received_hook_dispatch(&runtime, &member, claim.msg,
   NudgeKind::Queue)` — always `Queue`, never derived, because
   `mark_pending` only ever runs for a `Deferred` write (AQ1 D4); nothing
   claimable through this store is ever Steer-kind — and route
   it through the existing `MessageReceivedHookSelector` (steer for tmux
   members); a failed dispatch is requeued via
   `requeue_pending(&member, &claim)`. **One message per transition.**
   Read rows never appear (markers cleared by AQ1's read hook).
   **Channel pre-check (shared helper, defined here, called from both
   deliverable 2 and deliverable 3 — critical review B8)**: before
   calling `claim_next_pending`, the pre-check calls AQ1's
   `DeliveryChannel` classifier and admits the claim **only** when M is
   classified `TmuxSteer` or `Graft`. Heartbeat hooks are roster-blind,
   so without this the drain-side call site would claim for `HerdrSteer`
   or bare-CLI members too, racing AQ2.7's pump and burning attempt
   budget on members that never route through this sweep's channels. For
   `HerdrSteer` members the drain performs no claim (Herdr members belong
   solely to AQ2.7's lifecycle-gated pump); for bare-CLI members the drain
   performs no claim either (AQ2.5's emitter already cleared their marker
   on FIFO append — handoff semantics; the pre-check is the belt to that
   suspender). Implemented as one function called from both call sites so
   the two pre-checks cannot drift apart; this sprint authors the
   pre-check helper, AQ1 authors the classifier it calls — exactly one
   owner per file.
3. **Recovery sweep** — **kind-agnostic**: a low-frequency periodic pass
   (maintenance-cadence precedent) that enumerates candidate members via
   AQ1's `PendingNudgeStore::list_pending_members()`, applies deliverable
   2's shared channel pre-check to each (admits only `TmuxSteer` or
   `Graft`; skips `HerdrSteer` and bare-CLI members with no claim, no
   dispatch failure, no attempt increment), then for each admitted member
   calls `claim_next_pending(&member)` and, on `Some(claim)`, rebuilds the
   dispatch via `rebuild_received_hook_dispatch(&runtime, &member,
   claim.msg, NudgeKind::Queue)` (see deliverable 2 — always `Queue`, by
   construction of what `PendingNudgeStore` ever holds) and re-dispatches
   through the ordinary
   `MessageReceivedHookSelector`, which routes by recipient kind (graft →
   AQ2's queue channel, tmux → steer). The sweep contains **no
   graft-specific logic in its own diff** (it dispatches through the
   selector, whose graft arm is AQ2 code — critical review I2; this sprint
   lands after AQ2, so that is fine) and a failed dispatch is requeued via
   `requeue_pending(&member, &claim)`, so retry bounds and the stuck flag
   are enforced by AQ1's store, not here. This covers restarts (in-memory
   `RuntimeHealth` resets), missed transitions, and failed graft handoffs
   by the same mechanism. One per member per pass. Daemon shutdown
   cancels and joins the task within the daemon deadline.
4. **Observability**: per-drain structured event (`subsystem`/`action`/
   `outcome` + `{member, msg_id}`) and a cumulative drained counter on the
   health report. Events are emitted with `tracing::info!`/`tracing::warn!`
   carrying structured `subsystem`/`action`/`outcome` fields (see
   `crates/atm-daemon-bootstrap/src/queue_drain.rs`), matching the
   maintenance-worker precedent in `atm_temp_sweeper_runtime.rs`; the legacy
   daemon's `emit_daemon_event` is not used (it is unreachable from
   `atm-daemon-bootstrap`). No license to refactor the emission path.

## Acceptance criteria

1. Active→Idle with two pending unread → exactly the oldest drains (test
   double records it), marker clears; the second drains on the next
   transition.
2. Read-before-drain → skipped entirely; next pending drains instead.
3. Restart with pending rows → recovery sweep drains for an Idle member
   with no transition.
4. Concurrency: transition drain and sweep pass fired concurrently for one
   member with one pending message → exactly one nudge and one clear
   (atomic-claim test).
5. No transition, no tick → no nudge. Shutdown mid-pass cancels and joins
   within the deadline.
6. Sweep channel pre-check: the sweep claims only for members the
   AQ1 classifier resolves to `TmuxSteer` or `Graft`; for a
   `HerdrSteer` or bare-CLI member the sweep performs no claim and no attempt
   increment (test double over the classifier seam), and a pending marker for
   either member — should one exist — is never driven to a stuck flag by this
   sweep. AQ2.7 is the sole Herdr claimant. Sweep enumeration is exercised
   over `list_pending_members()` (test double returning a mixed-channel
   member set); re-dispatch is exercised over `rebuild_received_hook_dispatch`
   (rebuilt dispatch matches the original write-time shape for the same
   message-id).
7. Drain-side channel pre-check (critical review B8): the same pre-check
   as AC 6, applied at the idle-transition call site (deliverable 2), not
   only the sweep — an idle transition landing for a `HerdrSteer` or
   bare-CLI member performs no claim and no attempt increment (test
   double over the classifier seam). Both call sites are proven to share
   one pre-check function (a single test-double injection point covers
   both, or an assertion that both call sites invoke the same helper).
8. `just test` + daemon integration suite, all three lanes.

## Paths to delete

None.

## Required validation

- `just test` + daemon integration suite, ubuntu/macOS/Windows.
- Live evidence: a real tmux-harness member queued 2 messages while busy,
  observed draining one-per-idle-transition; transcript committed.
  **Requires AQ2.5's heartbeat producer** — no in-tree client sends
  `TeamMemberHeartbeatRequest` today, so a live idle transition cannot
  occur without it. The sink/drain tests are exercised by injecting
  heartbeats directly; the sweep pre-check consumes AQ2.5's classifier
  seam (see Dependencies).

### AC8 multi-OS CI citation (2026-08-28, ATM-QA-102)

CI run [`33128710126`](https://github.com/randlee/atm-core/actions/runs/33128710126)
at head `8b8071793055fc32ed87988012e781d16032424a` (PR #1054; verify with
`gh run view 33128710126`):

| Lane | ubuntu-latest | macos-latest | windows-latest |
| --- | --- | --- | --- |
| Test | pass | pass | pass |
| Just lint | pass | pass | pass |
| Clippy | pass | — | — |
| Format check | pass | — | — |

All three `Test` lanes are green at this head; the `windows-latest` failure
below (`scripts/hooks/test_queue_hooks.py`) is fixed on this branch and no
longer reproduces. **ATM-QA-102 is satisfied.**

Prior citation, superseded by the above (2026-08-27, ATM-QA-102):

CI run [`33123121121`](https://github.com/randlee/atm-core/actions/runs/33123121121)
at head `9af3817b2704594bb6ef7f44d2670f552ca9eb10` (PR #1054):

| Lane | ubuntu-latest | macos-latest | windows-latest |
| --- | --- | --- | --- |
| Test | pass | pass | **fail** |
| Just lint | pass | pass | pass |
| Clippy | pass | — | — |
| Format check | pass | — | — |

### Live tmux idle-drain evidence (2026-08-27, ATM-QA-101)

CI run [`33126990425`](https://github.com/randlee/atm-core/actions/runs/33126990425)
at head `35a28588ab4aa319eabbb972ca68931d9a61f05e`:

| Runner | Result | Evidence files |
| --- | --- | --- |
| clean-runner-linux | PASS | `AQ3/tmux-idle-drain-clean-runner-linux.json`, `.md` |
| clean-runner-macos | PASS | `AQ3/tmux-idle-drain-clean-runner-macos.json`, `.md` |
| clean-runner-windows | skipped_no_tmux | `AQ3/tmux-idle-drain-clean-runner-windows.json`, `.md` |

Both Linux and macOS evidence transcripts confirm all live-evidence assertions:
`fifo_order_confirmed=true`, `single_drain_per_transition_confirmed=true`,
`status=pass`. Windows test self-records `skipped_no_tmux` — tmux is not available
on Windows, and this sprint's drains are tmux-only by design. **ATM-QA-101 is satisfied.**

The only red cell, `Test (windows-latest)`, was **not** an AQ3/Rust failure:
`scripts/hooks/test_queue_hooks.py` (AQ2.5-owned Python Stop-hook test) failed
4/7 cases on Windows only, unrelated to this sprint's daemon-side drain code.
That gap was closed by a merge-forward fix,
`fix(aq2.5): run Python hook test shims on Windows` (`0129e2d42`), landed on
this branch at head `803168c59521e04ef1b70c60ee162248b75312c4`. CI run
[`33124563956`](https://github.com/randlee/atm-core/actions/runs/33124563956)
at that head confirms `Test (ubuntu-latest)` and `Test (macos-latest)` green
(unchanged from before); `Test (windows-latest)` was still in progress at the
time this citation was written — see that run's Actions page for its final
conclusion. The fix predates and is independent of AQ3's own changes below.

The companion `Phase AQ Evidence` workflow (a separate, non-gating job that
runs the live-evidence harnesses under `scripts/phase-aq/`) is red on both
`ubuntu-latest` and `macos-latest` at both of the above heads
([`33123121243`](https://github.com/randlee/atm-core/actions/runs/33123121243),
[`33124563961`](https://github.com/randlee/atm-core/actions/runs/33124563961)),
but precisely and only because of the pre-existing AQ1.9
`run_hermes_atm_restart_matrix.py` harness failure (unrelated to AQ3 —
tracked separately); the AQ2.5 harness passes clean in the same run. This
sprint's own live-evidence harness, `run_aq3_tmux_idle_drain_evidence.py`
(ATM-QA-101), was not yet registered in `EVIDENCE_DIR_BY_SCRIPT` at either of
those heads — it is registered as part of this same change and will be
exercised, along with a Windows leg added to that workflow's matrix (the
harness self-records a `skipped_no_tmux` status there), on the next dispatch.

### Recovery sweep (deliverable 3) evidence and orphan harness removal (2026-08-28, ATM-QA-104)

`scripts/phase-aq/run_aq3_idle_drain_sweep_evidence.py` was an orphaned local
script: it had no `EVIDENCE_DIR_BY_SCRIPT` entry in the `Phase AQ Evidence`
workflow (so CI never ran it), and no automated test covered it. It has been
deleted, along with its committed "local" transcripts
(`docs/plans/phase-aq/evidence/AQ3/idle-drain-sweep-local.json`, `.md`).

Deliverable 3 (the recovery sweep) does not need a separate live harness: it
is a low-frequency, in-process periodic pass with no tmux/pane dependency, so
its correctness is fully exercised by `crates/atm-daemon-bootstrap/src/queue_drain.rs`'s
unit and integration tests, run on every `just test` lane (AC8):

- `recovery_sweep_replays_pending_after_restart` — AC3 (restart with pending
  rows drains for an Idle member with no transition).
- `recovery_sweep_isolates_one_member_failure_and_continues` — one member's
  dispatch failure does not block the sweep pass for the rest of the roster.
- `concurrent_transition_and_sweep_claim_once` — AC4 (transition drain and
  sweep pass racing for one member claim exactly once).
- `shared_channel_precheck_skips_herdr_and_bare_cli_members` — AC6 (the
  sweep's shared channel pre-check admits only `TmuxSteer`/`Graft`).
- `shutdown_cancels_tracked_transition_and_releases_in_flight_claim` — AC5
  (daemon shutdown cancels and joins an in-flight sweep/drain task within the
  deadline).

`run_aq3_tmux_idle_drain_evidence.py` (ATM-QA-101, above) remains AQ3's sole
live/CI-exercised evidence harness; it proves deliverables 1 and 2 (the
idle-transition hook and drain step) end to end against a real daemon and
tmux server. **ATM-QA-104 is satisfied.**

## Non-closure / out of scope

- QA-1 remediation (2026-08-27): transition drains are tracked and joined
  within the replacement shutdown deadline; interrupted claims are released
  by a drop guard; the recovery sweep isolates per-member failures and
  continues; and handoff marker cleanup has one `atm-core` helper enforced by
  an architecture test. The real daemon/tmux loopback transcript remains
  pending because the shared local daemon owner lock is occupied and `m5` is
  not resolvable from this workstation.
- AQ2.5 dependency (2026-08-26): the bare-CLI delivery-trigger dependency
  for AQ3-CRIT-001 is satisfied by reconciled head `72d413134` on
  `feature/aq-2-5-queue-delivery-triggers`, including the shared
  `atm-core::nudge_dispatch::clear_queue_marker_after_handoff` ownership
  gate. The real daemon/tmux loopback transcript remains pending as noted
  above.
- Durable heartbeat history; subscription APIs beyond the internal sink;
  priority ordering; re-nudge/reminder policies.

## Dependencies

- must_follow: AQ1 (kinds, store, suppression) — merge-forward before every
  dev/fix round.
- must_follow: AQ2.5 (the sweep pre-check consumes AQ1's `DeliveryChannel`
  classifier via the bare-CLI "never sweep" rule AQ2.5 establishes; the
  live-evidence validation additionally requires AQ2.5's heartbeat producer). Merge-forward trigger: AQ2.5 dev push.
- must_follow: AQ2.6 (the classifier gains the retained-tmux/alternate-Herdr
  distinction; this sprint owns the corresponding "skip Herdr" pre-check
  diff). Merge-forward trigger: AQ2.6 dev push.
- parallel_safe: none — the former `parallel_safe: AQ2` was dead text
  (this sprint transitively follows AQ2 via AQ2.5; critical review I1,
  removed 2026-08-26).
- Resolved 2026-08-26 (critical review B8): the skip-Herdr / channel
  pre-check now lives as one shared function defined in deliverable 2 and
  called from both the **idle-transition drain (deliverable 2)** and the
  **sweep (deliverable 3)** — heartbeat hooks are roster-blind, so a Herdr
  member's Stop hook would otherwise trigger an unguarded drain claim
  racing AQ2.7's pump. Member discovery for the sweep uses AQ1's
  `list_pending_members`; the drain-side guard is now AC 7, distinct from
  the sweep's AC 6.
