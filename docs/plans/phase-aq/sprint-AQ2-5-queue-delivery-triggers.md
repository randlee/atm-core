# Sprint AQ2.5 — Queue Delivery Triggers: Harness Idle Signal + Bare-CLI Stop-Pull

Status: complete · Branch: `feature/aq-2-5-queue-delivery-triggers` off
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
("non-tmux") — an alternate harness channel slots in as one new classifier
arm + target variant + emitter impl with no renames. AQ2.6 exercises this
seam by adding `HerdrSteer`; it retains `TmuxSteer` as a coexisting backend.

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
roster state. The **channel classifier (defined in AQ1, consumed here)
is the single code owner of where a new dispatch is routed** — core's
dispatch-target planner and AQ3's sweep/drain pre-check (implemented in
AQ3 over this sprint's classifier seam) both call the same function, so
those two enforcement points cannot drift. The queue-get handler
(deliverable 3) is deliberately **not** a third caller of the
classifier: it decides purely on FIFO existence (critical review I15;
see deliverable 3/4), because re-classifying at get-time would strand a
member's already-queued FIFO backlog the moment their classification
changes. A get from a tmux member is harmless anyway: its FIFO does not
exist (no code path ever appends to it), so the get returns nothing —
denied by construction, never raced against AQ3.

| Member's classified channel (inputs: roster row + graft lease) | Trigger | Delivery |
|---|---|---|
| **tmux steer** — explicit local `backend = tmux` configuration (Claude or Codex — identical) | AQ3 idle-transition drain (fed by this sprint's heartbeats) | retained tmux emitter |
| **Herdr steer** — explicit local `backend = herdr` configuration (AQ2.6) | AQ2.7 lifecycle-gated wake pump for queue; immediate for steer | Herdr `agent prompt`; mailbox remains authoritative |
| **graft** — no local backend, graft lease registered (AQ1.5 store) | AQ2 queue channel | graft queue-kind wire message |
| **bare-CLI** — no local backend, no graft lease (Claude or Codex) | message arrival appends to the member's RAM FIFO; drain at the member's next Stop-pull get | queue-kind: one item per get · steer-kind: all items at once. Claude injects via Stop-hook block-with-reason. **Codex has no injection surface yet** — its FIFO accumulates bounded (drop-oldest ages out stale nudges) until Codex gains one or the member adopts graft; disclosed, not silent. |

The pending-marker machinery (AQ1 store) applies to tmux-steer, Herdr-steer,
and graft members. AQ3 schedules only tmux/graft; AQ2.7 is the sole
Herdr-marker claimant. For bare-CLI members the FIFO **is** the mechanism:
the emitter's append is the handoff (marker cleared, exactly AQ2's
handoff-clears-marker semantics), so neither scheduler has bare-CLI work and
the false-stuck problem cannot arise.

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
   - **FIFO**: one bounded in-memory FIFO per bare-CLI (team, agent).
     Type: `BareCliFifo = Arc<Mutex<HashMap<MemberKey,
     VecDeque<QueuedNudgeMessage>>>>` — a plain shared map, no new async
     machinery. **Key type**: AQ1's canonical public
     `atm_core MemberKey { team, agent }` (defined by AQ1 per the
     ruthless-boundary-qa one-canonical-key finding; both crates already
     depend on atm-core). The PRIVATE `runtime_health::MemberKey`
     (atm-http-runtime) is intentionally untouched and must be
     module-qualified wherever both are in scope. This sprint's earlier
     `BareCliMemberKey` (identical shape) is superseded — no such type
     is introduced. **Wiring (explicit, one interpretation)**: constructed
     ONCE in atm-daemon-bootstrap's `run_replacement_daemon_with_selector`
     composition root (defined at `atm-daemon-bootstrap/src/lib.rs:640`;
     it constructs `RuntimeHealth::with_owner` at line 653 and calls
     `build_replacement_handler`, whose `selector_factory(...)` invocation
     sits at line ~504 — corrects this doc's earlier "~lib.rs:217" anchor
     against the verified baseline) and cloned into BOTH consumers: (1)
     `StorageAndNudgeRouter` via a new `with_bare_cli_fifo(...)` builder
     step (mirroring `with_runtime_health`,
     `storage_and_nudge_router.rs:145-148`) for the get handler, and (2) a
     widened selector factory — `active_received_hook_selector(service_runtime,
     bare_cli_fifo)` and the matching `selector_factory` closure
     signature — for `PullPendingReceivedHook`. The FIFO deliberately
     does NOT live inside `LocalServiceRuntime` or `RuntimeHealth`; it is
     composition-root state like they are, reached by clone, so neither
     atm-core nor the health type grows daemon-RAM concerns.
     Capacity `BARE_CLI_FIFO_CAPACITY` (constant, default 32); overflow
     drops the **oldest** item (staleness preference) and increments a
     drop counter — **placement**: a plain `Arc<AtomicU64>`
     (`BareCliQueueFullDrops`, not folded into the `BareCliFifo` map type
     itself), constructed once beside `bare_cli_fifo` in the same
     composition root and cloned into the same two consumers. The
     `PullPendingReceivedHook` producer increments it on overflow; the
     `doctor()` handler in `storage_and_nudge_router.rs` (~437-465, which
     already does `report.runtime_status = Some(runtime_health.snapshot())`
     after `blocking_core_bridge.run`) reads it via the router's existing
     `self.bare_cli_fifo`-adjacent handle and sets a new
     `#[serde(default)] pub bare_cli_queue_full_drops_total: u64` field on
     `RuntimeStatusSnapshot` (`atm-core/src/protocol.rs:424`) — populated
     at the doctor call site, not inside `RuntimeHealth::snapshot()`,
     because `RuntimeHealth` deliberately has no FIFO knowledge. This is
     the concrete home for the `queue_full_drops_total` precedent this
     doc cites; the legacy daemon's `daemon_observability` module is
     off-limits and not a candidate.
   - **Producer**: `PullPendingReceivedHook` (deliverable 4) appends
     `{kind, msg_id, body}` on message arrival and clears exactly that
     message's marker via AQ1's
     `PendingNudgeStore::clear_pending_on_handoff(member, msg)` — the
     specific-message handoff clear, same as AQ2's graft channel (never
     `claim_next_pending`, which selects the oldest pending and would
     clear the wrong marker under a backlog). The append **is** the
     handoff. Bounded and synchronous; the al3 no-detached-work test
     stays green by construction. **Store handle**: mirroring AQ2's
     `PublishedGraftReceivedHook { service_runtime }`
     (`received_hook_selector.rs:78`, real code today),
     `PullPendingReceivedHook` is constructed with the whole
     `LocalServiceRuntime` (or, equivalently, the
     `Arc<dyn PendingNudgeStore>` obtained once from it) as a struct
     field at composition time — `service_runtime.pending_nudge_store()`
     — inside the widened `ReplacementReceivedHookSelector::new`. No
     separate plumbing is needed beyond the `service_runtime` parameter
     `active_received_hook_selector` already receives.
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
   `validate_heartbeat_member`) → drain per the policy above whatever is
   in that member's FIFO entry, if any — **FIFO existence wins, not a
   fresh classifier re-check** (critical review I15; deliverable 4).
   Empty or no FIFO entry → empty vec, regardless of the member's current
   classification. A get from a `TmuxSteer`/`Graft`/`HerdrSteer` member is
   harmless: nothing ever appends to their FIFO, so it is always empty in
   practice — denied by construction, never by a classifier check racing
   AQ3's sweep. Nothing else.

   ```text
   atm _internal-queue-get [--team <TEAM>] [--as <ACTOR>]
   ```

   stdout: one JSON line per drained message
   `{"kind": "...", "msg_id": "...", "body": "..."}`; **nothing when
   nothing is pending** (exit 0). Daemon unreachable: exit 0 within the
   bounded timeout, nothing on stdout for the raw CLI surface (fail-open — a
   direct CLI call must never be wedged). The lifecycle Stop hook invokes the
   hidden `--require-daemon` mode and reports an unavailable daemon or missing
   caller context as non-zero stderr rather than treating a diagnostic failure
   as an empty pull.

   **Claude Stop-hook consumer** (part of deliverable 2's script set),
   equally a straight line: on `Stop`, run `_internal-queue-get --team <TEAM>
   --as <ACTOR> --require-daemon`; got
   messages → emit Claude's literal block shape (bodies joined,
   oldest first) and exit 0; got nothing → exit 0.

   **Interaction with the heartbeat producer (deliverable 2, critical
   review M10)**: the same `Stop` event drives two independent calls with
   different timing, and they are never sequenced against each other.
   `_internal-queue-get` runs synchronously on every raw `Stop`, undebounced
   — it is a direct pull, not gated by idle detection. The idle heartbeat
   (`_internal-heartbeat --activity idle`) is debounced separately (`Stop`
   schedules it, `PreToolUse` cancels it, expiry sends it) and feeds AQ3's
   idle-transition drain, which only ever matters for `TmuxSteer`/`Graft`
   members. A given member is classified into exactly one channel, so in
   practice only one of the two consumers does anything for that member —
   the get always fires (hooks are uniform and roster-blind per this
   sprint's naming rule) but returns empty for a non-bare-CLI member,
   while the debounced heartbeat only ever produces an AQ3 drain for a
   non-bare-CLI member. Neither call blocks or waits on the other; the raw CLI
   surface remains fail-open, while the Stop hook reports queue-pull
   diagnostics fail-closed.

   ```json
   {"decision": "block", "reason": "<drained message bodies>"}
   ```

   **Loop policy (normative)**: the hook MAY pull on any `Stop`,
   including `stop_hook_active: true` — that is how a queue backlog
   drains one-per-stop. The termination guarantee is structural:
   **never block when the get returned nothing.** No hook-side counters
   or state. A direct CLI daemon-down path remains fail-open, but the Stop hook
   reports missing context, an unavailable daemon, malformed queue output, or
   an ATM CLI failure as non-zero stderr rather than silently exiting 0.

4. **Bare-CLI arm of AQ1's channel classifier + received-hook selector
   extension.** This sprint consumes AQ1's classifier; it adds only the
   `BareCli` consequences. AQ1 owns and defines the `DeliveryChannel`
   enum, `classify_delivery_channel`, the `LocalMessageReceivedBackend`
   input, and the `LocalSteer` target (trait-foundation deliverables); this
   sprint adds no new enum variants and implements no part of the
   classifier itself — only what a `BareCli` classification result drives
   (the `QueuePull` target, the FIFO, and the emitter):
   - (Reference — defined in AQ1) the classification function in core: inputs are the
     roster row's `LocalMessageReceivedBackend` (not a pane-id shape) and
     `GraftReceiverEndpointStore::lookup` for the graft lease — the
     AQ1.5 registry, **never** the retired file record, mirroring AQ2's
     `must_follow AQ1.7` reasoning — returning
     `TmuxSteer | HerdrSteer | Graft | BareCli`. The handler obtains the store
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
     `atm-core/src/boundary/mod.rs`'s emission-path enum. AQ2.6 replaces the
     local execution label with backend-neutral `LocalSteer`, so planners and
     selectors do not encode tmux-vs-Herdr branching;
     `PullPendingReceivedHook::emit_received_message` returns it, so the
     impl is fully specified.
   - `PullPendingReceivedHook` — a third `AsyncMessageReceivedHookEmitter`
     impl (sealed, beside `TokioTmuxReceivedHook` /
     `PublishedGraftReceivedHook` in
     `atm-daemon-bootstrap/src/received_hook_selector.rs`), selected by
     `ReplacementReceivedHookSelector` for `QueuePull` targets; its
     emit is deliverable 3's FIFO append.
   - **Seam ownership**: AQ1 owns the classifier itself. This sprint owns
     the `QueuePull` target variant, the `PullPendingReceivedHook`
     emitter, and the FIFO — the consequences of a `BareCli`
     classification, not the classification decision. AQ3 owns the sweep
     (and drain) pre-check **code** that calls the classifier — claiming
     only for members classified `TmuxSteer` or `Graft` (recorded in
     AQ3's deliverables 2 and 3 and gated by AQ3's own AC; AQ3 takes
     `must_follow AQ2.5` for this seam). `received_hook_selector.rs` is
     shared with AQ2's queue-channel edits, so this sprint takes
     `must_follow AQ2` and lands its emitter + selector arm on top of
     AQ2's merged changes. Exactly one sprint authors each diff —
     ownership is sequenced, never concurrent, on every shared file.
   - **Classification drift at get-time (critical review I15)**: the
     `QueueGetNextRequest` handler (deliverable 3) does not re-run
     `classify_delivery_channel` fresh and trust it blindly against a
     member whose roster/lease state may have changed since their last
     write-time dispatch. The rule is FIFO-existence-wins: the handler
     drains whatever is in that member's FIFO entry if one exists,
     regardless of the member's *current* classification. A `TmuxSteer`
     or `Graft` member's FIFO is always empty in practice (no code path
     ever appends to it), so this rule is a no-op for them today — a get
     from a tmux member is harmless exactly because nothing ever put
     anything in its FIFO — but it means a member who has *since*
     migrated off bare-CLI (added a local backend or a graft lease) still
     drains any messages a prior bare-CLI window queued for them, instead
     of those messages silently stranding because the live classifier now
     says `TmuxSteer`/`Graft`/`HerdrSteer`. The classifier remains the
     single decision point for where a *new* dispatch is routed
     (deliverable 4's `QueuePull` branch); it is not re-consulted to
     decide whether an *existing* FIFO entry may be drained.

   ```rust
   // atm-core (beside the existing delivery-policy/dispatch planning;
   // exact module placement follows DeliveryHarnessPath's home):
   /// Single code owner of the delivery-trigger table.
   pub enum DeliveryChannel { TmuxSteer, HerdrSteer, Graft, BareCli }

   /// Pure decision over already-fetched inputs — the caller performs
   /// the roster read and the GraftReceiverEndpointStore::lookup; the
   /// classifier itself does no I/O (trivially unit-testable).
   pub fn classify_delivery_channel(
       local_backend: Option<&LocalMessageReceivedBackend>, // roster row
       graft_lease: GraftLeaseState,  // AQ1 D7: Absent | Active; AQ1.7 maps the AQ1.5 lookup result onto it
   ) -> DeliveryChannel

   // core's post-persistence dispatch-target planning gains:
   pub enum PostSendBuiltInTarget {
       /// Backend-neutral opaque local-steer target. AQ2.6 binds this to
       /// either retained tmux or Herdr through the sealed emitter contract;
       /// planner/selector code does not match on that choice.
       LocalSteer(/* target + sealed backend handle */),
       Graft(/* existing payload, unchanged */),
       /// NEW: bare-CLI members — emitter appends to the RAM FIFO.
       QueuePull(QueuePullTarget), // { team, agent, kind, msg_id, body }
   }

   // Key: AQ1's canonical public atm_core::MemberKey { team, agent }
   // (NOT the private runtime_health::MemberKey, which is unchanged —
   // module-qualify where both are in scope). BareCliMemberKey is
   // superseded; no new key type is introduced by this sprint.
   // Derivation at the producer:
   //   PullPendingReceivedHook::emit_received_message constructs
   //   MemberKey { team: envelope.recipient_team.clone(),
   //               agent: envelope.recipient.clone() }
   //   from the delivered message envelope — the same fields the
   //   read-path clear uses, so producer and store key can never skew.
   pub type BareCliFifo =
       Arc<Mutex<HashMap<MemberKey, VecDeque<QueuedNudgeMessage>>>>;

   // atm-daemon-bootstrap/src/received_hook_selector.rs (on top of
   // AQ2's merged changes — must_follow AQ2):

   struct PullPendingReceivedHook {
       fifo: BareCliFifo,
       drops_total: BareCliQueueFullDrops, // Arc<AtomicU64>, deliverable 3
       // Store handle mirrors AQ2's PublishedGraftReceivedHook
       // (received_hook_selector.rs:78, real code today: `graft:
       // PublishedGraftReceivedHook { service_runtime }`) — captured
       // once at construction, no per-call plumbing:
       service_runtime: atm_core::LocalServiceRuntime,
   }
   impl boundary::sealed::Sealed for PullPendingReceivedHook {}
   impl AsyncMessageReceivedHookEmitter for PullPendingReceivedHook {
       // emit = bounded synchronous FIFO append +
       // clear_pending_on_handoff(member, msg); returns
       // PostSendEmissionPath::QueuePull (no detached work — al3
       // green by construction)
   }
   // ReplacementReceivedHookSelector::select_emitter gains:
   //   PostSendBuiltInTarget::QueuePull(_) => Some(&self.queue_pull)

   // Composition-root wiring (atm-daemon-bootstrap/src/lib.rs:640,
   // run_replacement_daemon_with_selector — today's RuntimeHealth
   // construction is at line 653; selector_factory(...) is invoked
   // inside build_replacement_handler at line ~504):
   //   let bare_cli_fifo: BareCliFifo = Arc::default();
   //   let bare_cli_queue_full_drops: BareCliQueueFullDrops = Arc::default();
   //   StorageAndNudgeRouter ... .with_bare_cli_fifo(bare_cli_fifo.clone(),
   //       bare_cli_queue_full_drops.clone())
   //   active_received_hook_selector(service_runtime, bare_cli_fifo,
   //       bare_cli_queue_full_drops)
   //   // selector_factory widens to
   //   //   FnOnce(LocalServiceRuntime, BareCliFifo, BareCliQueueFullDrops) -> ...
   //
   // Benchmark harness (second real caller of the widened signature —
   // received_hook_selector.rs ~:55, lib.rs ~:171, Justfile-wired):
   //   benchmark_received_hook_selector's Active arm calls
   //   active_received_hook_selector(service_runtime, Arc::default(),
   //       Arc::default())
   //   — an empty FIFO and a zeroed drop counter; bare-CLI delivery is intentionally outside
   //   benchmark semantics (the benchmark roster has no bare-CLI
   //   members), and the harness keeps compiling with no compat shim.
   ```
   - Extensibility (naming rule above): AQ2.6 adds mode-only `HerdrSteer` and
     one emitter impl after this sprint lands. Herdr derives its live target
     from the member `AgentName`; it adds no persisted target field and does
     not rename or remove `TmuxSteer`. Later channels use the same seam.

5. **ADR-054 addendum**: one clarifying sentence that 'steer = immediate'
   describes the KIND's delivery intent, while the bare-CLI mechanism may
   still defer steer-kind physically until the member's next Stop-pull
   (FIFO deferral is a mechanism property, not a kind change); plus the
   delivery-trigger policy table, the
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
   append drops the oldest item and increments the
   `bare_cli_queue_full_drops_total` counter, observable on the doctor
   report's `RuntimeStatusSnapshot` (not inside `RuntimeHealth`, which has
   no FIFO knowledge — see deliverable 3's placement note); (d) after a
   simulated daemon restart the get returns nothing and no error — the
   FIFO is empty by design, the underlying mail is still unread in the
   mailbox.
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
6. Classifier totality, selector, and gating: every classification outcome
   resolves through the single classifier function (test over all four member
   shapes); its local input is the backend enum, never `pane_id` syntax.
   `QueuePull` targets select
   `PullPendingReceivedHook` whose emit is bounded and synchronous (al3
   stays green); a get for a member classified `TmuxSteer`, `HerdrSteer`, or
   `Graft` returns empty because no code path ever appends to their FIFO —
   the get handler itself performs no classifier call (FIFO existence
   wins, critical review I15); the backend/lease →
   channel mapping exists in exactly one function (review gate: no duplicate
   backend or lease match outside the classifier). Migration case: a member
   with a stale FIFO entry from an earlier bare-CLI window still drains it
   on get even after their classification has since changed (test double:
   pre-seed a FIFO entry, flip the roster/lease inputs, assert the get
   still returns it).
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
    **Justfile lane**: `scripts/hooks/`'s tests are not auto-discovered by
    `.just/run_pytests.py` (it only globs `.just/tests/test_*.py` and
    `scripts/smoke/test_*.py`, verified) and not folded into default
    `just test`'s mode, so this sprint adds a dedicated recipe following
    the existing standalone-lane precedent (`test-graft-python`,
    `Justfile:165-166`: `{{python_cmd}} scripts/test_atm_graft_python.py`;
    `test-admission-capacity`, invoked as its own CI step,
    `.github/workflows/ci.yml:310`, on all three `Test (${{ matrix.os }})`
    OSes): a new `test-queue-hooks-python:` recipe running
    `{{python_cmd}} -m unittest discover -s scripts/hooks -p "test_*.py"`
    (or an explicit file list, matching `test-admission-capacity`'s
    `-m unittest` shape), invoked as its own CI step on all three matrix
    OSes for the Claude scripts and gated to ubuntu/macOS only for the
    Codex-specific script's tests (a second recipe or an env-conditional
    skip inside the same one, whichever keeps the Justfile recipe
    single-purpose).

11. Boundary-manifest freshness: `boundaries/atm-core/
   message-received-hook-emitter.toml`'s `[status].notes` implementer
   list — brought current by AQ1's L3.2 (baseline: `TokioTmuxReceivedHook`,
   `PublishedGraftReceivedHook`, sync `GraftReceiveHook`; verified today's
   pre-AQ1 manifest instead says only "the daemon tmux receiver and
   atm_graft::nudge_sink::GraftReceiveHook", so AQ1 must land its
   currency fix first) — is extended in this sprint's PR by exactly one
   name, `PullPendingReceivedHook`, the third
   `AsyncMessageReceivedHookEmitter` implementer (after
   `TokioTmuxReceivedHook` and `PublishedGraftReceivedHook`;
   `received_hook_selector.rs` has exactly those two `impl
   AsyncMessageReceivedHookEmitter for` occurrences today, verified). No
   manifest-vs-code count test exists in the repo yet
   (`crates/atm-architecture/tests/boundary_enforcement.rs`'s existing
   `al1`/`al3`/`al9` checks reference the manifest file only, never an
   implementer count) — this sprint adds one, a new test function in that
   same file (e.g. `al_message_received_hook_emitter_manifest_matches_async_implementers`,
   following the file's existing `.matches("...").count()` idiom at
   `al3_received_hook_is_single_receiver_side_path_without_detached_work`),
   asserting `received_hook_selector.rs`'s literal
   `impl AsyncMessageReceivedHookEmitter for` count (3 after this sprint)
   against the manifest's implementer-list length, so a future drift in
   either fails CI. If AQ1 has already introduced this test as part of
   its own manifest-currency fix, this sprint extends the same test's
   literal count from 2 to 3 rather than adding a second one.

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

## Evidence/validation

| Runner | Status | Run ID | Head | Files |
|--------|--------|--------|------|-------|
| ubuntu-latest | PASS | [33100596324](https://github.com/randlee/atm-core/actions/runs/33100596324) | bc3c9ee95 | [queue-delivery-trigger-clean-runner-linux.json](evidence/AQ2.5/queue-delivery-trigger-clean-runner-linux.json), [queue-delivery-trigger-clean-runner-linux.md](evidence/AQ2.5/queue-delivery-trigger-clean-runner-linux.md) — both scenarios (queue_kind_one_per_stop, steer_kind_full_drain) confirmed |
| macOS | PASS | [33110344593](https://github.com/randlee/atm-core/actions/runs/33110344593) | df5900a5e | [queue-delivery-trigger-clean-runner-macos.json](evidence/AQ2.5/queue-delivery-trigger-clean-runner-macos.json), [queue-delivery-trigger-clean-runner-macos.md](evidence/AQ2.5/queue-delivery-trigger-clean-runner-macos.md) — both scenarios (queue_kind_one_per_stop, steer_kind_full_drain) confirmed |

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
- Additional harness channels beyond the retained tmux and the AQ2.6 Herdr
  backend — designed-for via the classifier/target/emitter extension seam,
  not delivered here.
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
- must_follow: AQ2.6 (2026-08-26 reorder: Herdr lands before this sprint;
  the selector already carries the Herdr arm and this sprint adds the
  bare-CLI arm beside it). Merge-forward trigger: AQ2.6 dev push.
- Downstream: AQ3 takes `must_follow AQ2.5` for its live-evidence
  validation (heartbeat producer) and the bare-CLI "never sweep" rule;
  the classifier seam itself is AQ1's.
- Removed 2026-08-26: "AQ2.6 takes must_follow AQ2.5" — inverted; AQ2.6
  is now upstream and neither sprint owns the classifier (AQ1 does).
