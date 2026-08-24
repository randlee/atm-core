# Sprint AQ2.5 — Queue Delivery Triggers: Harness Idle Signal + Non-Tmux Pull

Status: draft · Branch: `feature/aq-2-5-queue-delivery-triggers` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Inserted per Rand 2026-08-24: the plan wires queue through the entire
CLI/daemon (AQ1 store, AQ2 graft channel, AQ3 idle-drain), but never
answers **"when do we send queue"** — AQ3's drain fires on
`RuntimeHealth` transitions to `Idle`, and **no in-tree client ever sends
a `TeamMemberHeartbeatRequest`** (verified: `HeartbeatActivity` /
`TeamMemberHeartbeatRequest` appear only in `protocol.rs`, `api.rs`,
`runtime_health.rs`, `message_handler.rs`,
`storage_and_nudge_router.rs` — all server-side). This sprint delivers
the idle-signal producer and the delivery-trigger policy for members
without a tmux pane.

**Verified baseline (2026-08-24, fenix)**: a production Codex hook
implementation already exists machine-globally (`~/.codex/hooks.json` →
`~/.codex/scripts/schook_codex_idle.py`) and proves the pattern this
sprint standardizes: `Stop` fires reliably on Codex (schook docs
corrected via randlee/schook#168), `Stop` writes a debounced pending
record and spawns a detached timer worker, `PreToolUse` cancels it, and
the debounce expiry is the idle event. An isolated end-to-end smoke test
of that cycle passed the same day. Claude Code supports the same `Stop`
/ `PreToolUse` / `SessionEnd` hook surface via `~/.claude/settings.json`,
so **one hook shape serves both harnesses, tmux and non-tmux alike** —
the hook produces the signal; the roster decides the channel.

## Delivery-trigger policy (normative; recorded in ADR-054 addendum)

For a queue-kind message pending for member M, the injection trigger and
channel are decided by M's roster row — the hook-side signal is identical
in every case:

| Roster shape | Trigger | Channel |
|---|---|---|
| tmux `pane_id` set (Claude or Codex — identical) | AQ3 idle-transition drain (fed by this sprint's heartbeats) | existing steer selector (tmux send-keys) |
| no `pane_id`, graft root published | AQ2 queue channel | graft queue-kind wire message |
| no `pane_id`, no graft, Claude | **pull at Stop** (deliverable 3) | Stop-hook block-with-reason injection |
| no `pane_id`, no graft, Codex | none — no injection channel exists | stays pending; AQ3's recovery sweep keeps it claimable until the member gains a pane or graft root. Explicitly disclosed, not silent. |

## Deliverables

1. **Heartbeat producer CLI surface**: `atm _internal-heartbeat
   --activity <active-tool-use|idle|session-ended>` (naming and plumbing
   mirroring `_internal-nudge`), wrapping the existing
   `RequestEnvelope::Heartbeat`. Identity/team from `ATM_IDENTITY` /
   `ATM_TEAM` or `--as` / `--team` per the standing caller-context rule;
   the daemon handler is the existing Heartbeat route (already gated
   `AuthenticatedIngress::Local`, member-validated) — **no daemon-side
   changes**. Exit fast and zero when the daemon is unreachable
   (bounded connect timeout): a heartbeat is advisory; a down daemon must
   never wedge or slow a harness hook (AQ1.5 lifecycle requirement
   applies).
2. **Reference hook scripts (Python MVP)**: shipped in-repo under
   `scripts/hooks/` with a README —
   - Claude (`~/.claude/settings.json` entries): `PreToolUse` →
     `--activity active-tool-use`; `Stop` → debounced idle (debounce
     lives hook-side, reusing the proven pending-record /
     cancel-on-PreToolUse / detached-timer pattern from the baseline
     implementation; the daemon stays dumb); `SessionEnd` →
     `--activity session-ended`.
   - Codex (`~/.codex/hooks.json` entries): same three mappings; the
     existing machine-global `schook_codex_idle.py` machinery is
     migrated to call deliverable 1 instead of (in addition to) its
     `atm send` idle notice, per the shared-hook policy in
     `~/.scripts/README.md` (dated backup + `HOOK_CHANGES.md` entry).
   - These scripts are the documented MVP contract for the later schook
     Rust plugin (out of scope here), which will link atm-core as a
     library instead of shelling the CLI.
3. **Non-tmux Claude pull path**: `atm _internal-claim-queue` exposing
   `PendingNudgeStore::claim_next_pending` for the **caller's own
   identity only** (same Local-ingress gating as deliverable 1; a member
   can never claim another member's queue). The Claude `Stop` hook — for
   members whose roster row has no `pane_id` — calls it; when a message
   is claimed, the hook blocks the stop with the message as the block
   reason (Claude's native continuation-injection). Guardrails:
   - **At-most-once is the store's**: the claim is the same atomic
     `claim_next_pending` AQ3's drain and sweep use; a concurrent AQ3
     sweep and a Stop-hook pull for one pending message yield exactly
     one winner (AC 4 mirrors AQ3 AC 4).
   - **Requeue on injection failure**: if the hook cannot emit the block
     output after claiming, it calls `requeue_pending` (`NudgeClaim`
     round-trip per AQ1 AC 4a); retry bounds and the stuck flag remain
     AQ1's.
   - **Loop guard**: the hook honors `stop_hook_active` and pulls at
     most one message per idle transition (a backlog drains one per
     stop, matching AQ3's one-per-transition rule).
   - **Fail-open**: daemon unreachable → exit 0 within the bounded
     timeout, stop proceeds normally.
4. **ADR-054 addendum**: the delivery-trigger policy table above, the
   heartbeat-producer decision (hook-side debounce, daemon stays dumb),
   and the disclosed Codex-non-tmux gap.

## Acceptance criteria

1. `atm _internal-heartbeat` drives `RuntimeHealth` transitions
   observable via the AQ3 sink (integration test over the existing
   Heartbeat route; deterministic clock per ADR-008).
2. Hook-script debounce cycle passes deterministically (env-overridable
   state root, debounce seconds, autostart — the baseline
   implementation's test seams): Stop schedules, PreToolUse cancels,
   expiry sends exactly one idle heartbeat.
3. Non-tmux Claude pull: with two pending queue messages, a Stop pulls
   exactly the oldest and blocks with its content; the next Stop pulls
   the second; `stop_hook_active` suppresses a re-pull loop (test
   double).
4. Concurrency: AQ3 sweep and Stop-pull racing for one pending message →
   exactly one nudge and one clear (shared atomic-claim test).
5. Identity gating: `_internal-claim-queue` for a different member than
   the authenticated caller is rejected.
6. Daemon down: both CLI surfaces exit 0 within the bounded timeout;
   hooks never block a harness (timed test).
7. `just test` all three lanes; hook scripts covered by Python tests
   runnable without a live daemon.

## Required validation

- `just test` + daemon integration suite, ubuntu/macOS/Windows (hook
  scripts themselves are exercised on macOS/ubuntu only — Codex/hermes
  are not used on Windows; the CLI surfaces are cross-platform).
- Live evidence: one real non-tmux Claude member with two queued
  messages observed pulling one-per-stop; one tmux member observed
  draining via AQ3 fed by this sprint's heartbeats; transcripts
  committed.

## Non-closure / out of scope

- schook Rust plugin (links atm-core as a library) — this sprint's
  scripts + README are its MVP spec.
- Codex non-tmux injection — waits for graft adoption (AQ1.5–AQ1.9 make
  the graft path robust; AQ2 gives it the queue channel).
- Any daemon-side scheduling/state beyond the existing Heartbeat route
  and AQ1's store (no new state machines — lifecycle rule from AQ1.5).
- Re-nudge/reminder policies (AQ3 non-closure carries).

## Dependencies

- must_follow: AQ1 (store claim surfaces + kinds). Merge-forward
  trigger: AQ1 dev push.
- parallel_safe: AQ2 (graft channel — disjoint emitters/files).
- Downstream: AQ3's **live-evidence validation** requires this sprint's
  heartbeat producer (AQ3's code deliverables remain parallel-safe; only
  its live-evidence step gains the dependency — recorded in AQ3).
