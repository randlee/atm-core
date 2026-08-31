---
phase: AV
sprint: AV.3
title: Mechanical hard gates against read-serialization regression
branch: feature/av3-read-concurrency-gates
integration_branch: integrate/phase-av
stack_parent: docs/av2-read-concurrency-requirements (dependency is on AV.1b below it)
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

- [ ] D1 — `BlockingCoreBridge` deleted (uncompilable gate): after the
      AV.1b cutover, remove the type and its remaining mutation-path
      usages in favor of the writer-ingress path, so no read path can be
      re-bridged without reintroducing a deleted type. I-5 found no
      boundary TOML governing the handler→writer edge; add a narrow TOML
      rule only if sc-lint-boundary supports semantic call-edge policy —
      otherwise D2 is the enforcement layer.
- [ ] D2 — Read-family architecture guard, positive-obligation first:
      the primary gate is a **handler dependency allowlist / typed
      boundary assertion** — the read-handler region's dependency
      surface must consist of the `AsyncMailboxRuntime` /
      `DoctorProjection` async ports (plus enumerated inert helpers);
      any *other* callable/type reference in that region fails the
      test, so a freshly named semaphore/bridge type or a new
      writer-queued async read fails without appearing on any list.
      The deny list (extend
      `crates/atm-architecture/tests/boundary_enforcement.rs:3389-3431`
      with `BlockingCoreBridge`, `spawn_blocking`, sync
      `*_with_runtime` read/list/doctor APIs,
      `MessageStore::list_messages`, writer ingress types) is retained
      as defense in depth, not the primary mechanism. Existing
      direct-SQLite prohibition retained.
- [ ] D2b — Behavior gate (mechanism-independent): tests run each
      read-family endpoint (list/peek/read/doctor) against an
      instrumented store whose **writer lane fails every submission** —
      each endpoint must still succeed. Any read path routed through
      writer-lane machinery, under any type name, fails this test
      regardless of what the source scan can see.
- [ ] D3 — WriteOp purity gate: a `.just` deny-list checker (alongside
      the existing Python checks, `justfile:112+` / `.just/`) asserting
      the `WriteOp` enum declares no pure-read variant and the
      read-handler file contains no bridge/spawn-blocking strings.
- [ ] D4 — Liveness tests owned as a permanent CI gate: the AV.1b D5
      stalled-op + read-storm and bounded-overload tests are wired into
      `just test` and documented as a release gate (removal requires an
      ADR change).
- [ ] D5 — Scratch-mutation demonstrations (recorded, then reverted)
      cover, at minimum: (a) reintroducing `spawn_blocking` in a read
      handler; (b) a **newly named** blocking-bridge type wrapping a
      1-permit semaphore in the read path; (c) routing an async read
      through the writer queue. Each must trip D2/D2b (and A3's lint
      where applicable).

## Code contracts

```rust
// boundary_enforcement.rs — indicative guard shape (D2).
#[test]
fn http_runtime_read_handlers_never_touch_writer_lane() {
    let src = read_http_runtime_source("storage_and_nudge_router.rs");
    let read_region = handler_region(&src, READ_FAMILY_HANDLERS);
    for banned in [
        "BlockingCoreBridge",
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
```

## Acceptance criteria

This is the authoritative acceptance checklist.

- [ ] A1 — `BlockingCoreBridge` no longer exists in the workspace.
      Two-part check: (1) a grep under `crates/` returns **zero**
      production-source occurrences; (2) remaining mentions exist only
      in named documentation paths (`docs/adr/`, `docs/plans/`,
      Phase-AM deletion ledger) as historical rationale.
- [ ] A2 — Every D5 scratch mutation (spawn_blocking reintroduction,
      newly named bridge type, writer-queued async read) fails
      `cargo test -p atm-architecture` and/or the D2b behavior tests
      (demonstrated once each, then reverted).
- [ ] A2b — D2b behavior tests pass on the real cutover code: all four
      read-family endpoints succeed against the writer-lane-failing
      instrumented store.
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
