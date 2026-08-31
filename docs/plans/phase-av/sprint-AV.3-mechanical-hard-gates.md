---
phase: AV
sprint: AV.3
title: Mechanical hard gates against read-serialization regression
branch: feature/av3-read-concurrency-gates
integration_branch: integrate/phase-av
stack_parent: docs/av2-read-concurrency-requirements (dependency is on AV.1b below it) — planned; stack provisioned by task AV.0 (phase plan §4)
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AV.1b
    relation: must_follow
    rationale: The gates assert the post-cutover state (bridge deleted from
      read paths, WriteOp pure). Stacked above AV.1b; restack before
      every round.
  - related: AV.2
    relation: parallel_safe
    rationale: AV.3 edits tests/lint tooling; AV.2 edits normative docs only.
  - related: AV.4
    relation: parallel_safe
    rationale: AV.4 edits benchmark harness/reports; no intersection with
      architecture tests or lint tooling.
---

# AV.3 — Mechanical hard gates against read-serialization regression

Convert the AV.2 normative rules into build/CI mechanisms, strongest
first: the wrong thing should fail to compile, then fail an architecture
test, then fail lint — never depend on reviewer vigilance.

## Deliverables

This is the authoritative deliverable checklist. Every listed
deliverable is expected to land at a production-ready level for the
scope this sprint claims; partial or shape-only completion fails the
sprint.

- [ ] D1 — `BlockingCoreBridge` identifier deleted; residual bridge
      narrowed and enumerated (uncompilable + exact-call-site gate).
      After the AV.1b cutover the bridge still has **non-read**
      callers that this phase does not migrate and that have no
      writer-ingress equivalent: the deferred-queue marker
      (`storage_and_nudge_router.rs:62-68`, intentionally synchronous
      by its own contract), `heartbeat` (`:644-675`) and
      `queue_get_next` (`:677-701`) — whose only bridged work is the
      synchronous roster check `validate_heartbeat_member` plus
      in-memory operations — the four `graft_receiver_*` handlers
      (`:703-800`), which bridge the same roster check plus the
      synchronous `GraftReceiverEndpointStore`, and `clear_messages`
      (`:557-576`), a **mutation** that runs the synchronous
      `atm_core::clear::clear_mail_with_runtime` and has no
      writer-ingress `WriteOp` today (it is not a read and is not on
      the acceptance contract). That is 12 bridge call sites at HEAD:
      AV.1b D1 migrates 4 (list/peek/read/doctor); these **eight**
      remain. Deleting the type outright is therefore not reachable
      from AV's scope (quality-mgr AV-R1-B1/AV-R2-B1). What this sprint
      does instead:
      1. rename the type to `ControlPathSyncBridge` and delete the
         identifier `BlockingCoreBridge` from `crates/` — any read path
         re-bridged under the old name fails to compile, and the new
         name states the residual scope;
      2. an architecture test asserts the **exact** set of
         `ControlPathSyncBridge::run` call sites (the eight enumerated
         above, by enclosing handler name); any new call site —
         read-family or otherwise — fails the test;
      3. the residual control-path/mutation migration (an async
         roster/member validation port, an async graft-receiver-store
         port, and a `WriteOp::ClearMailbox` ingress op for clear, after
         which the bridge is deleted for real) is **explicitly out of
         AV scope** and recorded as follow-up `AV-FU-1` in the phase plan
         §4, with the eight sites listed. It is not hidden behind "the
         bridge is gone".

      I-5 found no boundary TOML governing the handler→writer edge; add
      a narrow TOML rule only if sc-lint-boundary supports semantic
      call-edge policy — otherwise D2 is the enforcement layer.
- [ ] D2 — Read-family architecture guard, positive-obligation first:
      the primary gate is a **handler dependency allowlist / typed
      boundary assertion** — the read-handler region's dependency
      surface must match the AV.1b D1 split exactly — list/peek/read
      handlers may reference only the `AsyncMailboxRuntime` port, the
      doctor handler only the `DoctorProjection` port (plus enumerated
      inert helpers); a mailbox handler touching `DoctorProjection` or
      vice versa also fails;
      any *other* callable/type reference in that region fails the
      test, so a freshly named semaphore/bridge type or a new
      writer-queued async read fails without appearing on any list.
      The deny list (extend
      `crates/atm-architecture/tests/boundary_enforcement.rs:3389-3431`
      with `BlockingCoreBridge`, `ControlPathSyncBridge`,
      `spawn_blocking`, sync `*_with_runtime` read/list/doctor APIs,
      `MessageStore::list_messages`, writer ingress types) is retained
      as defense in depth, not the primary mechanism. Existing
      direct-SQLite prohibition retained.

      **Composition-layer sibling (same test file):** the gate also
      covers the `atm-runtime` module(s) implementing
      `AsyncMailboxRuntime` and `DoctorProjection` (AV.1a D6 / AV.1b
      D3), because a single-permit gate reintroduced *there* would be
      invisible to a handler-region scan. Allowlist for that region: the
      `AsyncMailboxReader` handle, the search/doctor lane handles, the
      pure `atm-core::read::selection` module, and the
      `StateHandoffSupervisor`. Deny list there additionally includes
      `tokio::sync::Semaphore`/`Mutex` acquisition around a storage
      read, `spawn_blocking`, `ControlPathSyncBridge`, and any
      `StorageWriterIngress` submission other than through the
      supervisor. The activated AV.1a composition assertion additionally
      permits `StorageAsyncMailboxRuntime`, `RequestDeadline`,
      `ReadDeadline`, `AtmError`/`ReadLaneError`, the ordinary mailbox
      domain values (`MailboxScope`, `Message`, `MessageKey`,
      `MessageQuery`), and all public
      `atm_core::read::selection` values
      (`MailboxSelectionRequest`, `MailboxSelectionCandidate`,
      `MailboxSelectionResult`, `SelectedMailboxMessage`). It derives only
      the production `impl AsyncMailboxRuntime for
      StorageAsyncMailboxRuntime` with `syn`; `#[cfg(test)]` writer fakes
      are outside the asserted region. `AsyncMessageStore` and
      `MessageStore` deliberately remain absent from the positive allowlist.
- [ ] D2b — Behavior gate (mechanism-independent): tests run each
      read-family endpoint against an **instrumented writer ingress
      that records every submission** (op variant, origin, outcome)
      and can be switched to reject. Assertions: `list`, `peek`, and
      `doctor` make **zero** writer submissions; `read` makes either
      zero submissions or only `WriteOp::ApplyReadDisplayState` via the
      AV.1b supervisor handoff — never any other variant, never a
      pure-read op, and never a submission whose *result* feeds the
      response data. Separately, with the ingress set to reject, every
      endpoint still succeeds and the read's handoff rejection is
      observed via the supervisor metrics, not via the response. Any
      read path that obtains data through writer machinery, under any
      type name, fails these assertions regardless of what the source
      scan can see.
- [ ] D3 — WriteOp purity gate: a `.just` deny-list checker (alongside
      the existing Python checks, `justfile:112+` / `.just/`) asserting
      the `WriteOp` enum declares no pure-read variant and the
      read-handler file contains no bridge/spawn-blocking strings.
- [ ] D4 — Liveness tests owned as a permanent CI gate: the AV.1b D5
      stalled-op + read-storm and bounded-overload tests are wired into
      `just test` and documented as a release gate (removal requires an
      ADR change). Until that permanent round-2 CI job lands, default
      `just lint` also runs the `arch-gates` task (`cargo test -p
      atm-architecture --quiet`) so the D1/D2/D2b architecture assertions
      remain live at the routinely cited lint entry point.
- [ ] D5 — Scratch-mutation demonstrations (recorded, then reverted)
      cover, at minimum: (a) reintroducing `spawn_blocking` in a read
      handler; (b) a **newly named** blocking-bridge type wrapping a
      1-permit semaphore in the read path; (c) routing an async read
      through the writer queue; (d) a 1-permit semaphore wrapped around
      the storage read inside the `atm-runtime` `AsyncMailboxRuntime`
      implementation (composition layer, not the handler); (e) a new
      `ControlPathSyncBridge::run` call site added to a read-family
      handler. Each must trip D1/D2/D2b (and A3's lint where
      applicable).

## Code contracts

```rust
// boundary_enforcement.rs — indicative guard shape (D2).
#[test]
fn http_runtime_read_handlers_never_touch_writer_lane() {
    let src = read_http_runtime_source("storage_and_nudge_router.rs");
    let read_region = handler_region(&src, READ_FAMILY_HANDLERS);
    for banned in [
        "BlockingCoreBridge",
        "ControlPathSyncBridge",
        "spawn_blocking",
        "list_mail_with_runtime",
        "peek_mail_with_runtime",
        "read_mail_with_runtime",
        "run_doctor_with_runtime",
        "MessageStore::list_messages",
        "StorageWriterIngress",
    ] {
        assert!(!read_region.contains(banned), "read path references {banned}");
    }
}

// D1 — exact residual call-site set for the renamed bridge.
#[test]
fn control_path_sync_bridge_call_sites_are_exactly_the_enumerated_residual() {
    // Enclosing handler fn of each `.run(` call site (12 at HEAD − 4
    // migrated by AV.1b). The deferred-marker site lives inside `send`.
    const RESIDUAL: [&str; 8] = [
        "send" /* retry_deferred_marker */, "clear_messages",
        "heartbeat", "queue_get_next",
        "graft_receiver_register", "graft_receiver_refresh",
        "graft_receiver_unregister", "graft_receiver_lookup",
    ];
    let sites = bridge_run_call_sites_by_enclosing_fn("storage_and_nudge_router.rs");
    assert_eq!(sites, RESIDUAL.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>());
}
```

## Acceptance criteria

## QA history

- 2026-08-31 QA-r1 fixes: `AV3-B1/B2/B3/I1/I2/M1/M2/M3` use a shared
  checked-in handler contract, hardened Python literal-aware scanner, and
  `syn` AST for Rust source gates. AST is chosen because handler ownership,
  generic functions, `#[cfg(test)]` exclusion, and bridge construction/type
  forms are semantic Rust properties; string scanning cannot make that
  contract robust. Commits: `27bd384eb`, `06b73c9f8`, `3a1736a7b`.
- 2026-08-31 QA-r2 fixes: `AV3-B3` masks raw, raw-byte, and byte string
  literals before balancing handler braces. `AV3-I2` replaces the D2b
  tautology with an `AsyncMailboxRuntime`-activated ingress recorder that
  derives each path from the real router handler body and proves a writer-lane
  fixture is rejected. AV.1b will activate the supervised state-handoff path.

This is the authoritative acceptance checklist.

- [ ] A1 — Bridge gate, three-part check: (1) a grep for
      `BlockingCoreBridge` under `crates/` returns **zero**
      production-source occurrences; remaining mentions exist only in
      named documentation paths (`docs/adr/`, `docs/plans/`, the
      phase-AV closeout record — AV.2 D4) as historical rationale;
      (2) the D1 call-site test enumerates exactly the eight residual
      `ControlPathSyncBridge::run` sites (incl. `clear_messages`) and
      no read-family handler is among them; (3) `AV-FU-1` is recorded
      in the phase plan §4 with those eight sites — the residual is
      tracked, not implied closed.
- [ ] A2 — Every D5 scratch mutation (spawn_blocking reintroduction,
      newly named bridge type, writer-queued async read,
      composition-layer single permit, new bridge call site in a read
      handler) fails `cargo test -p atm-architecture` and/or the D2b
      behavior tests (demonstrated once each, then reverted).
- [ ] A2b — D2b behavior tests pass on the real cutover code: recorded
      writer submissions are zero for list/peek/doctor and at most the
      named `ApplyReadDisplayState` handoff for read; all four endpoints
      succeed with the ingress rejecting; the D5 writer-queued-list and
      newly-named-bridge scratch mutations each fail a D2/D2b gate.
- [ ] A3 — Adding a pure-read `WriteOp` variant fails `just lint`
      (demonstrated once with a scratch mutation, then reverted).
- [ ] A4 — All gates run in default `just lint` / `just test` with no
      opt-in flags.

## Required validation

This is the authoritative validation checklist.

- [ ] `just lint`
- [ ] `just test`
- [ ] `just validate`
- [ ] Scratch-mutation demonstrations for A2/A3 recorded in the sprint
      QA history (live proof the gate trips before automated QA).

## Out of scope

- The reader-lane implementation itself — AV.1a/AV.1b.
- Normative requirement text — AV.2.
- Migrating the eight residual bridge callers (roster validation,
  graft-receiver store, deferred marker, clear) to async ports / writer
  ingress and deleting the bridge for real — follow-up `AV-FU-1` (phase
  plan §4).
