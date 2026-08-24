# Sprint AQ9 — Nudge Taxonomy: ADR-055 and Code Refactor

Status: draft · Branch: `feature/aq-9-nudge-taxonomy` off `integrate/phase-aq`
· PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Establishes the canonical taxonomy — **nudge** is the umbrella term for
post-delivery recipient notification; **steer** (immediate) and **queue**
(deferred) are its two kinds — and lands the code refactor that makes the
daemon use it consistently, so AQ7/AQ8 build on named concepts instead of
retrofitting them. The complete refactor inventory below was produced by a
code sweep of `integrate/phase-ao2`; a type missing from it that needs
renaming is a sprint defect, not deferred work.

## Deliverables

1. **ADR-055 nudge-taxonomy-and-queue-mechanism**, reviewed by quality-mgr
   before AQ7/AQ8 dispatch, deciding with rationale:
   (a) the taxonomy above, used consistently through daemon code — "nudge"
   reserved for the umbrella, steer/queue for the kinds (aligned with, but
   distinct from, Hermes's session-dispatch `mode="queue"|"steer"`;
   disambiguation noted);
   (b) the `nudge_pending_at` column, derived-FIFO, and atomic-claim
   semantics (consumed by AQ7/AQ8);
   (c) the steer-suppression seam — caller-owned in
   `PreparedWrite::build_received_hook_dispatches` per ADR-019; the
   `emit_received_hook` router call site, its `newly_persisted` guard, the
   `al3_*` architecture test, and `http-runtime.toml`'s unconditional
   post-write invariant stay untouched;
   (d) `PendingNudgeStore` governance via the ADR-018 §3 follow-up process
   (AQ7 implements);
   (e) `MemberStateTransitionSink`'s relationship to ADR-019's caller-owned
   model and `RuntimeHealth`'s observability scope (AQ8 implements);
   (f) the graft dual-channel contract (independent steer-shaped and
   queue-shaped channels; harness integration owns landing; Hermes
   `/steer`+`/queue` complete) and the queue-channel handoff failure
   policy;
   (g) **rename/compat policy** for the inventory below — explicitly: the
   `.atm.toml` `post_send_hooks` key and the external command-hook system
   it configures are a DISTINCT mechanism from built-in nudges and are NOT
   renamed (user-facing config compat; the `reject_legacy_post_send_hook_keys`
   precedent governs if that ever changes); the `NudgeTemplateOverrideStore`
   cluster (templates, SQLite table, CLI subcommands, error codes) keeps
   its names — "nudge" there is already the umbrella sense; wire-crossing
   contracts (`GraftPostSendRequest`/`Response` over loopback TCP — the
   receiver process can lag/lead the daemon binary — and the
   `ATM_INTERNAL_NUDGE`/`InternalNudgeEnvelope` env payload) change only
   with an explicit versioned/both-sides plan or stay; `PyNudge` and the
   Python callback shape (`nudge.body`/`notice_text`/`source`,
   `activate_receiver`) are consumer-facing — kept, with any future rename
   via deprecation shim; `atm doctor --json` report field names kept.
2. **Kind-aware received-hook contract**: the message-received family
   accepts both kinds — `BuiltInPostSendDispatch` (or its successor)
   carries `NudgeKind::Steer | NudgeKind::Queue`; `ReplacementReceivedHookSelector`
   routes both; the graft emitter transmits the kind on its channel wire
   (dual-channel per ADR-055 (f)); the tmux emitter handles Steer only
   (Queue for tmux is AQ8's drain, which re-dispatches as Steer at drain
   time). `NudgeMode` on the write side maps 1:1 onto the dispatch kind.
3. **Mechanical rename pass** (internal-only identifiers; complete
   inventory from the phase-ao2 code sweep, updated in ONE change set
   together with the literal string assertions in
   `atm-architecture/tests/boundary_enforcement.rs` (`al3_*`,
   `canonical_write_router_has_one_host_routing_decision`) that pin
   `emit_received_hook`, `build_received_hook_dispatches`,
   `received_hook_dispatches`, and `MessageReceivedHookSelector`):
   - atm-core boundary: `MessageReceivedHookEmitter` (sync, still
     implemented by `GraftReceiveHook`), `AsyncMessageReceivedHookEmitter`,
     `MessageReceivedHookSelector`, `BuiltInPostSendDispatch`,
     `PostSendBuiltInTarget::{LocalTmux,Graft}` (+ new Queue-kind
     representation), `LocalTmuxNudgeTarget`, `GraftNudgeTarget`,
     `BuiltInNudgeSinkTarget`, `ResolvedBuiltInNudgeTemplate`,
     `built_in_nudge_template_kind_from_post_send_event`,
     `TMUX_NUDGE_CONFIRM_KEY`/`TMUX_DOUBLE_ENTER_DELAY`;
   - atm-core send: `PreparedReceivedHook`, `prepare_received_hook`,
     `build_built_in_dispatch` (gains the kind decision),
     `render_built_in_nudge_for_dispatch`,
     `DeliveryRecipientSnapshot.{local_tmux_post_send,graft_post_send}`
     predicates;
   - atm-http-runtime: `StorageAndNudgeRouter` + module
     `storage_and_nudge_router.rs` (rename optional — decided in ADR-055
     (g)), `emit_received_hook`, `CommittedWrite.received_hook_dispatches`,
     test doubles (`RecordingReceivedHook`, `FixedReceivedHookSelector`,
     `NoReceivedHookSelector`, `HarnessReceivedHookSelector`);
   - atm-daemon-bootstrap: `active_received_hook_selector`,
     `ReplacementReceivedHookSelector`, `TokioTmuxReceivedHook` (the
     steer emitter — note real name; there is no `LocalTmuxReceivedHook`),
     `PublishedGraftReceivedHook`, benchmark selector variants;
   - atm-graft: module `nudge_sink`, `GraftReceiveHook`, `HostNudge`,
     `HostNudgeInjector`, `BoundedHostNudgeInjector`,
     `spawn_host_nudge_helper` family, `listen_for_graft_nudges`,
     `GraftObservability::nudge_delivered`;
   - log/event strings: `delivery_policy.new_message.{primary_nudge,
     error_nudge,post_send_hook_fallback}` (kind-qualified; the duplicate
     literal in `daemon_observability.rs:1084` deduped),
     `subsystem = "atm_graft.{nudge_sink,host_nudge}"`.
   Renames follow the taxonomy (steer for today's immediate paths; nudge
   only where both kinds flow); every rename decision cross-checked against
   ADR-055 (g)'s compat policy.
4. **Terminology gate**: a grep-gate (precedent:
   `scripts/check-legacy-mailbox-paths.py`) enumerated in CI that fails on
   new daemon-code identifiers using "nudge" where a kind is meant (list of
   allowed umbrella-sense identifiers maintained in the gate script).

## Acceptance criteria

1. ADR-055 merged with decisions (a)–(g) closed, none deferred; quality-mgr
   sign-off recorded.
2. Rename change set compiles and passes `just test` on all three CI lanes
   with `boundary_enforcement.rs` literal assertions updated in the same
   commit — no intermediate broken state.
3. Kind-aware dispatch: a unit test drives one Steer and one Queue dispatch
   through the selector; tmux emitter receives Steer only; graft emitter
   receives both and emits on the matching channel.
4. Compat surfaces proven unchanged: `.atm.toml` fixtures with
   `[[atm.post_send_hooks]]` still parse; `atm doctor --json` field names
   unchanged (fixture test); `GraftPostSendRequest` wire JSON unchanged (or
   versioned per ADR-055 (g)); `PyNudge` attribute names unchanged
   (hermes-atm test suite green).
5. Terminology grep-gate enumerated in CI.

## Paths to delete

None beyond identifier renames; no behavior change other than the
kind-awareness plumbing (Queue dispatches exist but nothing produces them
until AQ7).

## Required validation

- `just test` workspace + `cargo test -p atm-architecture`, all three CI
  lanes; hermes-atm Python tests green.
- ADR-055 reviewed by quality-mgr with explicit sign-off on (a)–(g).

## Non-closure / out of scope

- Producing Queue dispatches (`atm queue`, AQ7). Draining (AQ8). Repo-doc
  terminology updates (landed on the plan branch during planning, per
  Rand — this sprint owns only code).

## Dependencies

- must_follow: AQ1 (contract root; ADR numbering) — merge-forward before
  every dev/fix round.
- parallel_safe: AQ2, AQ3, AQ4, AQ5 (disjoint surfaces; AQ2's send-surface
  work coordinates on the shared `NudgeMode`/`PreparedWrite` seam only at
  merge-forward points). AQ7 and AQ8 must_follow AQ9.
