# Sprint AQ2.5 — Queue Delivery Triggers: Harness Idle Signal + Bare-CLI Stop-Pull

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
the idle-signal producer, a **bare-CLI** delivery path (RAM FIFO +
simple get) for members with no push channel, and the single-code-owner
channel classifier that makes the trigger policy enforceable.

**Simplicity mandate (Rand, 2026-08-24, binding)**: this implementation
must stay very simple — no Rust machinery that is hard to debug. The
bare-CLI path is a **RAM-only bounded FIFO** in the daemon and **a plain
get that returns nothing when the FIFO is empty**. No persistence, no
claim tokens, no requeue round-trips, no hook-side state machines.
Staleness is the concern, not loss: a daemon restart empties the FIFO
and that is accepted — the mail itself is always durably in the mailbox.

**Naming rule (Rand, 2026-08-24)**: channels are named by their
**mechanism** (tmux steer, graft, bare-CLI), never by negation
("non-tmux") — a future harness channel (e.g. a tmux replacement) must
slot in as one new classifier arm + target variant + emitter impl with
no renames.

**Verified baseline (2026-08-24, fenix)**: a production Codex hook
implementation already exists machine-globally (`~/.codex/hooks.json` →
`~/.codex/scripts/schook_codex_idle.py`) and proves the pattern this
sprint standardizes: `Stop` fires reliably on Codex (schook docs
corrected via randlee/schook#168), `Stop` writes a debounced pending
record and spawns a detached timer worker, `PreToolUse` cancels it, and
the debounce expiry is the idle event. An isolated end-to-end smoke test
of that cycle passed the same day. Claude Code supports the same `Stop`
/ `PreToolUse` / `SessionEnd` hook surface via `~/.claude/settings.json`,
so **one hook shape serves both harnesses, with or without tmux**. That
baseline is prior art only — every committed deliverable of this sprint
is in-repo (see Non-closure for the machine-global follow-up).

## Delivery-trigger policy (normative; recorded in ADR-054 addendum)

For a message pending for member M, the trigger and channel are decided
by M's classified delivery channel. **Hooks are uniform and
roster-blind**: every harness runs the same scripts and never consults
roster state. The **channel classifier (deliverable 4) is the single
code owner of this table** — core's dispatch-target planner, the
queue-get handler (deliverable 3), and AQ3's sweep pre-check
(implemented in AQ3 over this sprint's classifier seam) all call the
same function, so the enforcement points cannot drift. A get from a
tmux member is harmless: its FIFO does not exist, the get returns
nothing — denied, never raced against AQ3.

| Member's classified channel (inputs: roster row + graft lease) | Trigger | Delivery |
|---|---|---|
| **tmux steer** — roster `pane_id` set (Claude or Codex — identical) | AQ3 idle-transition drain (fed by this sprint's heartbeats) | existing steer selector (tmux send-keys) |
| **graft** — no `pane_id`, graft lease registered (AQ1.5 store) | AQ2 queue channel | graft queue-kind wire message |
| **bare-CLI** — no `pane_id`, no graft lease (Claude or Codex) | message arrival appends to the member's RAM FIFO; drain at the member's next Stop-pull get | queue-kind: one item per get · steer-kind: all items at once. Claude injects via Stop-hook block-with-reason. **Codex has no injection surface yet** — its FIFO accumulates bounded (drop-oldest ages out stale nudges) until Codex gains one or the member adopts graft; disclosed, not silent. |

The pending-marker machinery (AQ1 store, AQ3 sweep) applies **only** to
tmux-steer and graft members. For bare-CLI members the FIFO **is** the
mechanism: the emitter's append is the handoff (marker cleared, exactly
AQ2's handoff-clears-marker semantics), so the sweep never has bare-CLI
work and the false-stuck problem cannot arise.

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

3. **Bare-CLI RAM FIFO + simple get.** RAM only — **deliberately not
   persisted**; a daemon restart empties it and that is the accepted
   trade (staleness beats loss; the mail remains durably unread in the
   mailbox, visible to `atm read` and the operator).
   - **FIFO**: one bounded in-memory FIFO per bare-CLI (team, member).
     Type: `BareCliFifo = Arc<Mutex<HashMap<MemberKey,
     VecDeque<QueuedNudgeMessage>>>>` — a plain shared map, no new async
     machinery. **Wiring (explicit, one interpretation)**: constructed
     ONCE in atm-daemon-bootstrap's `run_replacement_daemon_with_selector`
     composition root (beside where `RuntimeHealth` is constructed today,
     `atm-daemon-bootstrap/src/lib.rs` ~:217) and cloned into BOTH
     consumers: (1) `StorageAndNudgeRouter` via a new
     `with_bare_cli_fifo(...)` builder step (mirroring
     `with_runtime_health`) for the get handler, and (2) a widened
     selector factory — `active_received_hook_selector(service_runtime,
     bare_cli_fifo)` and the matching `selector_factory` closure
     signature — for `PullPendingReceivedHook`. The FIFO deliberately
     does NOT live inside `LocalServiceRuntime` or `RuntimeHealth`; it is
     composition-root state like they are, reached by clone, so neither
     atm-core nor the health type grows daemon-RAM concerns.
     Capacity `BARE_CLI_FIFO_CAPACITY` (constant, default 32); overflow
     drops the **oldest** item (staleness preference) and increments a
     cumulative dropped counter on the health report
     (`queue_full_drops_total` precedent).
   - **Producer**: `PullPendingReceivedHook` (deliverable 4) appends
     `{kind, msg_id, body}` on message arrival and clears exactly that
     message's marker via AQ1's
     `PendingNudgeStore::clear_pending_on_handoff(member, msg)` — the
     specific-message handoff clear, same as AQ2's graft channel (never
     `claim_next_pending`, which selects the oldest pending and would
     clear the wrong marker under a backlog). The append **is** the
     handoff. Bounded and synchronous; the al3 no-detached-work test
     stays green by construction.
   - **Consumer — one route, one CLI surface, a straight line**:

   ```rust
   // protocol.rs additions
   // RequestEnvelope::QueueGetNext(QueueGetNextRequest)
   // + matching ResponseEnvelope variant; api.rs gains one
   //   HttpRouteKind + route spec, modeled on Heartbeat.

   pub struct QueueGetNextRequest {
       pub team: TeamName,
       pub member: AgentName, // filled from caller context ONLY —
                              // the CLI surface has no target-member
                              // flag (see AC 5)
   }

   /// Drain policy: ALL steer-kind items currently in the FIFO plus
   /// AT MOST ONE queue-kind item (oldest first). Empty vec when the
   /// FIFO is empty or the member is not classified bare-CLI — the
   /// caller cannot and need not distinguish the two.
   pub struct QueueGetNextResponse {
       pub messages: Vec<QueuedNudgeMessage>, // { kind, msg_id, body }
   }
   ```

   Handler beside the Heartbeat handler in
   `storage_and_nudge_router.rs`: gate `AuthenticatedIngress::Local` →
   validate member against the roster (mirroring
   `validate_heartbeat_member`) → classifier says `BareCli`? → drain
   per the policy above. Not bare-CLI or empty → empty vec. Nothing
   else.

   ```text
   atm _internal-queue-get [--team <TEAM>] [--as <ACTOR>]
   ```

   stdout: one JSON line per drained message
   `{"kind": "...", "msg_id": "...", "body": "..."}`; **nothing when
   nothing is pending** (exit 0). Daemon unreachable: exit 0 within the
   bounded timeout, nothing on stdout (fail-open — a stop must never be
   wedged).

   **Claude Stop-hook consumer** (part of deliverable 2's script set),
   equally a straight line: on `Stop`, run `_internal-queue-get`; got
   messages → emit Claude's literal block shape (bodies joined,
   oldest first) and exit 0; got nothing → exit 0.

   ```json
   {"decision": "block", "reason": "<drained message bodies>"}
   ```

   **Loop policy (normative)**: the hook MAY pull on any `Stop`,
   including `stop_hook_active: true` — that is how a queue backlog
   drains one-per-stop. The termination guarantee is structural:
   **never block when the get returned nothing.** No hook-side counters
   or state. **Fail-open**: any error path exits 0 without blocking.

4. **Channel classifier + received-hook selector extension** — the
   trigger table's single code owner, deliberately small (one enum, one
   function, one trivial emitter):
   - A `DeliveryChannel` classification function in core: inputs are
     the roster row (`pane_id`) and
     `GraftReceiverEndpointStore::lookup` for the graft lease — the
     AQ1.5 registry, **never** the retired file record, mirroring AQ2's
     `must_follow AQ1.7` reasoning — returning
     `TmuxSteer | Graft | BareCli`. The handler obtains the store
     handle exactly as AQ2's queue channel wires it into
     `storage_and_nudge_router.rs`.
   - `PostSendBuiltInTarget::QueuePull` — a third variant in core's
     post-persistence dispatch-target planning. **This sprint owns the
     third branch in `build_built_in_dispatch`
     (`atm-core/src/send/hook.rs:17`, invoked from
     `build_received_hook_dispatches`, `send/mod.rs` ~:391)**: classify
     via `classify_delivery_channel`; a `BareCli` member's dispatch —
     any kind, steer or queue — becomes a `QueuePull` target. This is a
     shared-file seam with AQ1, whose deliverable 2 gives the same
     function the kind decision; AQ1 lands first (already
     `must_follow AQ1`), and this sprint's branch builds on it —
     sequenced single ownership, mirroring the `received_hook_selector.rs`
     seam with AQ2.
   - `PostSendEmissionPath::QueuePull` — a matching variant in
     `atm-core/src/boundary/mod.rs`'s emission-path enum
     (`ExternalHook | LocalTmux | GraftPort` today);
     `PullPendingReceivedHook::emit_received_message` returns it, so the
     impl is fully specified.
   - `PullPendingReceivedHook` — a third `AsyncMessageReceivedHookEmitter`
     impl (sealed, beside `TokioTmuxReceivedHook` /
     `PublishedGraftReceivedHook` in
     `atm-daemon-bootstrap/src/received_hook_selector.rs`), selected by
     `ReplacementReceivedHookSelector` for `QueuePull` targets; its
     emit is deliverable 3's FIFO append.
   - **Seam ownership**: AQ2.5 owns the classifier, the target variant,
     the emitter, and the FIFO. AQ3 owns the sweep pre-check **code**
     that calls the classifier — the sweep claims only for members
     classified `TmuxSteer` or `Graft` (recorded in AQ3's deliverable 3
     and gated by AQ3's own AC; AQ3 takes `must_follow AQ2.5` for this
     seam). `received_hook_selector.rs` is shared with AQ2's
     queue-channel edits, so this sprint takes `must_follow AQ2` and
     lands its emitter + selector arm on top of AQ2's merged changes.
     Exactly one sprint authors each diff — ownership is sequenced,
     never concurrent, on every shared file.

   ```rust
   // atm-core (beside the existing delivery-policy/dispatch planning;
   // exact module placement follows DeliveryHarnessPath's home):
   /// Single code owner of the delivery-trigger table.
   pub enum DeliveryChannel { TmuxSteer, Graft, BareCli }

   /// Pure decision over already-fetched inputs — the caller performs
   /// the roster read and the GraftReceiverEndpointStore::lookup; the
   /// classifier itself does no I/O (trivially unit-testable).
   pub fn classify_delivery_channel(
       pane_id: Option<&str>,                     // roster row
       graft_lease: Option<&GraftReceiverLease>,  // AQ1.5 lookup result
   ) -> DeliveryChannel

   // core's post-persistence dispatch-target planning gains:
   pub enum PostSendBuiltInTarget {
       LocalTmux(/* existing payload, unchanged */),
       Graft(/* existing payload, unchanged */),
       /// NEW: bare-CLI members — emitter appends to the RAM FIFO.
       QueuePull(QueuePullTarget), // { team, member, kind, msg_id, body }
   }

   // atm-daemon-bootstrap/src/received_hook_selector.rs (on top of
   // AQ2's merged changes — must_follow AQ2):
   pub type BareCliFifo =
       Arc<Mutex<HashMap<MemberKey, VecDeque<QueuedNudgeMessage>>>>;

   struct PullPendingReceivedHook { fifo: BareCliFifo, /* + store
       handle for clear_pending_on_handoff */ }
   impl boundary::sealed::Sealed for PullPendingReceivedHook {}
   impl AsyncMessageReceivedHookEmitter for PullPendingReceivedHook {
       // emit = bounded synchronous FIFO append +
       // clear_pending_on_handoff(member, msg); returns
       // PostSendEmissionPath::QueuePull (no detached work — al3
       // green by construction)
   }
   // ReplacementReceivedHookSelector::select_emitter gains:
   //   PostSendBuiltInTarget::QueuePull(_) => Some(&self.queue_pull)

   // Composition-root wiring (atm-daemon-bootstrap/src/lib.rs, beside
   // today's RuntimeHealth construction ~:217):
   //   let bare_cli_fifo: BareCliFifo = Arc::default();
   //   StorageAndNudgeRouter ... .with_bare_cli_fifo(bare_cli_fifo.clone())
   //   active_received_hook_selector(service_runtime, bare_cli_fifo)
   //   // selector_factory widens to
   //   //   FnOnce(LocalServiceRuntime, BareCliFifo) -> ...
   ```
   - Extensibility (naming rule above): a future harness channel adds
     one classifier arm + one target variant + one emitter impl — no
     renames anywhere.

5. **ADR-054 addendum**: the delivery-trigger policy table, the
   classifier as its single code owner with its enforcement call sites,
   the mechanism-positive channel naming rule, the heartbeat-producer
   decision (hook-side debounce, daemon stays dumb), the RAM-only FIFO
   decision (staleness-over-loss, restart empties it, drop-oldest
   overflow), the drain policy (one queue item / all steer items), the
   loop policy, and the disclosed Codex-drain gap.

## Acceptance criteria

1. `atm _internal-heartbeat` drives `RuntimeHealth` transitions
   observable via the AQ3 sink (integration test over the existing
   Heartbeat route; deterministic clock per ADR-008).
2. Hook-script debounce cycle passes deterministically (env-overridable
   state root, debounce seconds, autostart): Stop schedules, PreToolUse
   cancels, expiry sends exactly one idle heartbeat.
3. FIFO semantics: (a) two queued queue-kind messages drain one per
   get, oldest first; (b) three steer-kind items drain all at once in
   one get alongside at most one queue-kind item; (c) at capacity, an
   append drops the oldest item and increments the dropped counter;
   (d) after a simulated daemon restart the get returns nothing and no
   error — the FIFO is empty by design, the underlying mail is still
   unread in the mailbox.
4. Stop-pull drain: with two pending queue messages, a genuine-idle
   Stop (`stop_hook_active: false`) gets the oldest and emits the
   literal block JSON; the follow-up Stop (`stop_hook_active: true`)
   gets the second and blocks again; the next Stop gets nothing, emits
   nothing, and the stop proceeds — never-block-on-empty is the loop
   terminator.
5. Identity scope (honest bound): the CLI surfaces accept **no
   target-member parameter** — the envelope's `member` is filled from
   presented caller context only, and the daemon validates it against
   the roster exactly as the Heartbeat handler does. A caller
   misrepresenting identity via `--as`/env is outside this sprint's
   threat model, identical to every other Local-ingress command (test:
   the clap surface rejects any attempt to pass a member argument; a
   crafted envelope for a non-roster member is rejected by validation).
6. Classifier totality, selector, and gating: every classification
   outcome resolves through the single classifier function (test over
   all three member shapes); `QueuePull` targets select
   `PullPendingReceivedHook` whose emit is bounded and synchronous (al3
   stays green); a get for a member classified `TmuxSteer` or `Graft`
   returns empty and touches no FIFO/store state; the roster-shape →
   channel mapping exists in exactly one function (review gate: no
   duplicate match on pane-id/lease outside the classifier).
7. Marker handoff: for a bare-CLI member, message arrival appends to
   the FIFO and clears the pending marker in the same dispatch (AQ2
   handoff semantics) — the AQ3 sweep subsequently finds nothing for
   that member (test double).
8. Daemon down: both CLI surfaces exit 0 within the bounded timeout;
   hooks never block a harness (timed test).
9. ADR-054 addendum merged with quality-mgr sign-off recorded (mirrors
   AQ1 AC 1's ADR gate).
10. `just test` all three lanes. Claude hook scripts' Python unit tests
    green on **all three lanes including Windows** (Claude Code runs on
    Windows; cross-platform-guidelines apply). Codex hook scripts' unit
    tests green on ubuntu/macOS (Codex/hermes are not used on Windows).

## Required validation

- `just test` + daemon integration suite, ubuntu/macOS/Windows.
- Live evidence (AQ2.5's own): one real bare-CLI member (Claude, no
  pane, no graft lease) with two queued messages observed pulling
  one-per-stop, and a steer message observed arriving in full on the
  next get; transcript committed.
- AQ3's tmux live-evidence transcript is **AQ3's gate, not this
  sprint's** — AQ2.5 supplies the heartbeat producer that transcript
  depends on and claims no ownership of it. Likewise the
  sweep-skips-bare-CLI test is **AQ3's AC** (it owns the sweep
  pre-check code); AQ2.5's AC 6 covers only the classifier both rely
  on.

## Non-closure / out of scope

- **Machine-global hook migration (ops follow-up, not a committed
  deliverable)**: migrating the existing `~/.codex/hooks.json` /
  `~/.codex/scripts/schook_codex_idle.py` installation on developer
  hosts to the `scripts/hooks/` reference scripts is a per-host ops
  task with no PR/CI gate; it is tracked as an explicit follow-up after
  this sprint merges and is intentionally NOT covered by any AC here.
- schook Rust plugin (links atm-core as a library) — deliverable 2's
  scripts + README are its MVP spec.
- Codex bare-CLI **drain** — the FIFO fills for Codex members too, but
  Codex has no injection surface; drain waits for one (or graft
  adoption via AQ1.5–AQ1.9/AQ2). Bounded drop-oldest ages out stale
  nudges meanwhile. Disclosed in the trigger table.
- Additional harness channels (e.g. a tmux replacement) — designed-for
  via the classifier/target/emitter extension seam, not delivered here.
- Claude Stop-pull **live evidence** on Windows (unit tests run there
  per AC 10; the committed live transcript is macOS/ubuntu — disclosed
  platform bound, revisited if a Windows deployment materializes).
- Any persistence, claim/requeue machinery, or delivery-guarantee
  bookkeeping for the bare-CLI path — rejected per the simplicity
  mandate; RAM-only with the disclosed restart/overflow bounds is the
  accepted trade.
- Any daemon-side scheduling/state beyond the existing Heartbeat route,
  the get route, the FIFO, and AQ1's store (no new state machines —
  lifecycle rule from AQ1.5).
- Re-nudge/reminder policies (AQ3 non-closure carries).

## Dependencies

- must_follow: AQ1 (kinds + pending-marker dispatch the emitter hands
  off from; `clear_pending_on_handoff` in its store contract; AND a
  shared-file seam — AQ1's deliverable 2 edits
  `send/hook.rs::build_built_in_dispatch` first, this sprint adds the
  `QueuePull` branch on top, sequenced single ownership like the
  `received_hook_selector.rs` seam with AQ2). Merge-forward trigger:
  AQ1 dev push.
- must_follow: AQ1.7 (the classifier's graft-lease input reads AQ1.5's
  `GraftReceiverEndpointStore` — the daemon registry, never the retired
  file record; same reasoning as AQ2's identical dependency).
  Merge-forward trigger: AQ1.7 dev push.
- must_follow: AQ2 (shared file:
  `atm-daemon-bootstrap/src/received_hook_selector.rs` — AQ2 edits
  `PublishedGraftReceivedHook`'s emit path and this sprint adds
  `PullPendingReceivedHook` + the `ReplacementReceivedHookSelector`
  match arm to the same file and match statement. AQ2 lands its
  selector-file changes first; this sprint's diff builds on them —
  single owner per diff, sequenced, mirroring the AQ2.5→AQ3 classifier
  seam resolution). Merge-forward trigger: AQ2 dev push. The resulting
  sprint chain is AQ2 → AQ2.5 → AQ3.
- Downstream: AQ3 takes `must_follow AQ2.5` for the classifier seam its
  sweep pre-check calls, and its **live-evidence validation** requires
  this sprint's heartbeat producer (both recorded in AQ3; AQ3's other
  deliverables and its parallel_safe AQ2 claim are unaffected).
