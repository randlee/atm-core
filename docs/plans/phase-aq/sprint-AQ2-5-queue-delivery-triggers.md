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
so **one hook shape serves both harnesses, tmux and non-tmux alike**.
That baseline is prior art only — every committed deliverable of this
sprint is in-repo (see Non-closure for the machine-global follow-up).

## Delivery-trigger policy (normative; recorded in ADR-054 addendum)

For a queue-kind message pending for member M, the injection trigger and
channel are decided by M's roster row. **Hooks are uniform and
roster-blind**: every harness runs the same scripts and never consults
roster state. The daemon enforces this table at its two decision points
— AQ3's drain/sweep dispatch (rows 1–2) and this sprint's claim route
(row 3), which grants a pull claim **only when pull is M's designated
channel** and denies it otherwise (deliverable 3). That server-side gate
is what makes a pull request from a tmux member harmless: it is denied,
never raced against AQ3.

| Roster shape | Trigger | Channel |
|---|---|---|
| tmux `pane_id` set (Claude or Codex — identical) | AQ3 idle-transition drain (fed by this sprint's heartbeats) | existing steer selector (tmux send-keys) |
| no `pane_id`, graft root published | AQ2 queue channel | graft queue-kind wire message |
| no `pane_id`, no graft, Claude | **pull at Stop** (deliverable 3) | Stop-hook block-with-reason injection |
| no `pane_id`, no graft, Codex | none — no injection channel exists | stays pending and claimable indefinitely — the AQ3 sweep's viable-channel pre-check (amended there this sprint) skips no-channel members entirely, so the message never burns attempt budget and is never marked stuck; it delivers when the member gains a pane or graft root. Explicitly disclosed, not silent. |

## Deliverables

1. **Heartbeat producer CLI surface** (`crates/atm/src/commands/`,
   mirroring `internal_nudge.rs` plumbing):

   ```text
   atm _internal-heartbeat --activity <active-tool-use|idle|session-ended>
       [--team <TEAM>] [--as <ACTOR>]
   ```

   Wraps the **existing** `RequestEnvelope::Heartbeat`
   (`TeamMemberHeartbeatRequest`; handler already gated
   `AuthenticatedIngress::Local` + `validate_heartbeat_member`, verified
   at `storage_and_nudge_router.rs:441-465`) — **no daemon-side
   changes**. Caller context per the standing rule (`ATM_IDENTITY` /
   `ATM_TEAM` env or `--as` / `--team`; no `.atm.toml` fallback). Output:
   nothing on stdout. Exit codes: `0` accepted **and** `0` on
   daemon-unreachable within a bounded connect timeout (a heartbeat is
   advisory; a down daemon must never wedge or slow a harness hook —
   AQ1.5 lifecycle requirement); nonzero only for caller-context or
   validation errors.

2. **Reference hook scripts (Python MVP)** — in-repo under
   `scripts/hooks/` with a README documenting installation
   (`~/.claude/settings.json` / `~/.codex/hooks.json` entries):
   - Claude: `PreToolUse` → `--activity active-tool-use`; `Stop` →
     debounced idle (debounce lives hook-side — pending-record /
     cancel-on-PreToolUse / detached-timer pattern from the verified
     baseline; the daemon stays dumb); `SessionEnd` →
     `--activity session-ended`.
   - Codex: same three mappings (`Stop` / `PreToolUse` /
     `SessionStart`-adjacent lifecycle per the Codex hook surface).
   - All state/debounce/timeout knobs env-overridable (the baseline's
     test seams) so the scripts unit-test without a live daemon.
   - The README states these scripts are the MVP contract for the later
     schook Rust plugin (links atm-core as a library; out of scope
     here).

3. **Non-tmux Claude pull path** — a new daemon-mediated claim/requeue
   capability plus the Claude Stop-hook consumer. **Wire contract
   (normative)**, patterned on AQ1.5's registration contract; the CLI
   never touches storage directly — everything goes through the daemon's
   existing Local-ingress path:

   ```rust
   // protocol.rs additions
   // RequestEnvelope::QueueClaimNext(QueueClaimNextRequest)
   // RequestEnvelope::QueueRequeue(QueueRequeueRequest)
   // + matching ResponseEnvelope variants; api.rs gains one
   //   HttpRouteKind + route spec per request, modeled on Heartbeat.

   pub struct QueueClaimNextRequest {
       pub team: TeamName,
       pub member: AgentName, // filled from caller context ONLY —
                              // the CLI surface has no target-member
                              // flag (see AC 5)
   }

   pub enum QueueClaimNextResponse {
       /// Pull is this member's designated channel and a pending
       /// queue message was atomically claimed
       /// (PendingNudgeStore::claim_next_pending — the same claim
       /// AQ3's drain and sweep use).
       Claimed { msg_id: MessageId, claim: NudgeClaim, body: String },
       /// Pull is the designated channel but nothing is pending.
       Empty,
       /// Pull is NOT this member's designated channel (roster row has
       /// a pane_id or a graft root). Hook treats identically to
       /// Empty; the daemon-side trigger table stays authoritative.
       NotDesignated,
   }

   pub struct QueueRequeueRequest {
       pub team: TeamName,
       pub member: AgentName, // caller context only, as above
       pub claim: NudgeClaim, // round-trips claim_next_pending's claim
   }
   ```

   Handler lives beside the Heartbeat handler in
   `storage_and_nudge_router.rs`: gated `AuthenticatedIngress::Local`,
   member validated against the roster (mirroring
   `validate_heartbeat_member`), then the **designated-channel gate**
   (trigger-table row lookup on the member's roster shape) before any
   store call. Requeue delegates to
   `PendingNudgeStore::requeue_pending` — attempt bounds and the stuck
   flag remain AQ1's.

   CLI surface:

   ```text
   atm _internal-claim-queue [--team <TEAM>] [--as <ACTOR>]
   atm _internal-requeue-claim --claim <CLAIM-JSON> [--team ..] [--as ..]
   ```

   `_internal-claim-queue` stdout: on `Claimed`, a single JSON object
   `{"msg_id": "...", "claim": {...}, "body": "..."}`; on
   `Empty`/`NotDesignated`, nothing (exit 0). Daemon unreachable: exit 0
   within the bounded timeout, nothing on stdout (fail-open — a stop
   must never be wedged).

   **Claude Stop-hook consumer** (part of deliverable 2's script set):
   on `Stop`, call `_internal-claim-queue`; if a message was claimed,
   emit Claude's literal block shape on stdout and exit 0:

   ```json
   {"decision": "block", "reason": "<claimed message body>"}
   ```

   If claim output was received but the block cannot be emitted, call
   `_internal-requeue-claim` (the `NudgeClaim` round-trip per AQ1
   AC 4a). Guardrails:
   - **At-most-once is the store's**: the shared atomic
     `claim_next_pending` — a concurrent AQ3 sweep and a Stop-pull for
     one pending message yield exactly one winner (AC 4).
   - **Loop policy (normative)**: the hook MAY pull on any `Stop`,
     including `stop_hook_active: true` — that is precisely how a
     backlog drains one-per-stop (each block-continuation ends in
     another Stop, which pulls the next message). The structural
     termination guarantee is: **never block when no message was
     claimed** (`Empty`/`NotDesignated`/daemon-down → exit 0, stop
     proceeds). A defensive consecutive-block cap
     (`ATM_QUEUE_PULL_MAX_CONSECUTIVE`, default 20, hook-side state)
     backstops pathological continuous refill. Parity with AQ3's
     one-per-transition rule is **behavioral** (one message per idle
     event, backlog drains at the harness's pace), not mechanistic —
     the hook never consults `RuntimeHealth`.
   - **Fail-open**: any error path exits 0 without blocking.

4. **AQ3 sweep viable-channel pre-check** (small amendment to AQ3's
   recovery-sweep deliverable, recorded there): the sweep claims a
   pending message **only for members with a viable dispatch channel**
   (tmux `pane_id`, graft root, or pull-designated — the latter is
   drained by deliverable 3, not the sweep). A no-channel member is
   skipped **before** any claim: no claim, no dispatch failure, no
   `requeue_pending`, no attempt increment — which is what makes the
   trigger table's row 4 ("stays claimable indefinitely") true instead
   of a stuck-flag contradiction.

5. **ADR-054 addendum**: the delivery-trigger policy table, the
   server-side enforcement points (claim-route gate + sweep pre-check),
   the heartbeat-producer decision (hook-side debounce, daemon stays
   dumb), the loop policy, and the disclosed Codex-non-tmux gap.

## Acceptance criteria

1. `atm _internal-heartbeat` drives `RuntimeHealth` transitions
   observable via the AQ3 sink (integration test over the existing
   Heartbeat route; deterministic clock per ADR-008).
2. Hook-script debounce cycle passes deterministically (env-overridable
   state root, debounce seconds, autostart): Stop schedules, PreToolUse
   cancels, expiry sends exactly one idle heartbeat.
3. Non-tmux Claude pull drain: with two pending queue messages, a
   genuine-idle Stop (`stop_hook_active: false`) claims the oldest and
   emits the literal block JSON; the follow-up Stop
   (`stop_hook_active: true`) claims the second and blocks again; the
   next Stop gets `Empty`, emits nothing, and the stop proceeds — the
   never-block-on-empty rule is the loop terminator. The consecutive-cap
   backstop triggers under forced continuous refill (test double).
4. Concurrency and channel gating: (a) AQ3 sweep and Stop-pull racing
   for one pending message → exactly one nudge and one clear (shared
   atomic-claim test); (b) a claim request for a member whose roster row
   has a `pane_id` (or graft root) returns `NotDesignated` and touches
   no store state — a tmux member's queue is never drainable via pull.
5. Identity scope (honest bound): the CLI surfaces accept **no
   target-member parameter** — the envelope's `member` is filled from
   presented caller context only, and the daemon validates it against
   the roster exactly as the Heartbeat handler does. A caller
   misrepresenting identity via `--as`/env is outside this sprint's
   threat model, identical to every other Local-ingress command (test:
   the clap surface rejects any attempt to pass a member argument; a
   crafted envelope for a non-roster member is rejected by validation).
6. Daemon down: all three CLI surfaces exit 0 within the bounded
   timeout; hooks never block a harness (timed test).
7. ADR-054 addendum merged with quality-mgr sign-off recorded (mirrors
   AQ1 AC 1's ADR gate).
8. AQ3 sweep pre-check: a pending message for a no-channel member
   survives N sweep passes with zero attempt increments and no stuck
   flag, then delivers on the first pass after the member gains a
   channel (integration test over a reopened store).
9. `just test` all three lanes. Claude hook scripts' Python unit tests
   green on **all three lanes including Windows** (Claude Code runs on
   Windows; cross-platform-guidelines apply). Codex hook scripts' unit
   tests green on ubuntu/macOS (Codex/hermes are not used on Windows).

## Required validation

- `just test` + daemon integration suite, ubuntu/macOS/Windows.
- Live evidence (AQ2.5's own): one real non-tmux Claude member with two
  queued messages observed pulling one-per-stop; transcript committed.
- AQ3's tmux live-evidence transcript is **AQ3's gate, not this
  sprint's** — AQ2.5 supplies the heartbeat producer that transcript
  depends on and claims no ownership of it.

## Non-closure / out of scope

- **Machine-global hook migration (ops follow-up, not a committed
  deliverable)**: migrating the existing `~/.codex/hooks.json` /
  `~/.codex/scripts/schook_codex_idle.py` installation on developer
  hosts to the `scripts/hooks/` reference scripts is a per-host ops
  task with no PR/CI gate; it is tracked as an explicit follow-up after
  this sprint merges and is intentionally NOT covered by any AC here.
- schook Rust plugin (links atm-core as a library) — deliverable 2's
  scripts + README are its MVP spec.
- Codex non-tmux injection — waits for graft adoption (AQ1.5–AQ1.9 make
  the graft path robust; AQ2 gives it the queue channel).
- Claude Stop-pull **live evidence** on Windows (unit tests run there
  per AC 9; the committed live transcript is macOS/ubuntu — disclosed
  platform bound, revisited if a Windows deployment materializes).
- Any daemon-side scheduling/state beyond the existing Heartbeat route,
  the new claim/requeue routes, and AQ1's store (no new state machines —
  lifecycle rule from AQ1.5).
- Re-nudge/reminder policies (AQ3 non-closure carries).

## Dependencies

- must_follow: AQ1 (store claim surfaces + kinds). Merge-forward
  trigger: AQ1 dev push.
- parallel_safe: AQ2 (graft channel — disjoint emitters/files).
- Coordinated with AQ3: deliverable 4 amends AQ3's sweep contract
  (viable-channel pre-check, recorded in both docs); AQ3's
  **live-evidence validation** requires this sprint's heartbeat
  producer (AQ3's code deliverables remain parallel-safe; only its
  live-evidence step gains the dependency — recorded in AQ3).
