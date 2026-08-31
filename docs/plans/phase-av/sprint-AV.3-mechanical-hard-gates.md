---
phase: AV
sprint: AV.3
title: Mechanical hard gates against read-serialization regression
branch: feature/av3-read-concurrency-gates
integration_branch: integrate/phase-av
status: planned
recommended_agent: arch-ctm
recommended_model: deep-reasoning
dependency_relations:
  - related: AV.1b
    relation: must_follow
    rationale: The gates assert the post-cutover state (bridge deleted from
      read paths, WriteOp pure). Merge-forward AV.1b → AV.3 before every
      round.
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

This is the authoritative deliverable checklist.

- [ ] D1 — `BlockingCoreBridge` deleted (uncompilable gate): after the
      AV.1b cutover, remove the type and its remaining mutation-path
      usages in favor of the writer-ingress path, so no read path can be
      re-bridged without reintroducing a deleted type. I-5 found no
      boundary TOML governing the handler→writer edge; add a narrow TOML
      rule only if sc-lint-boundary supports semantic call-edge policy —
      otherwise D2 is the enforcement layer.
- [ ] D2 — Read-family architecture guard: extend the existing
      http-runtime scan in
      `crates/atm-architecture/tests/boundary_enforcement.rs:3389-3431`
      to assert the read-handler region references none of:
      `BlockingCoreBridge`, `spawn_blocking`, sync `*_with_runtime`
      read/list/doctor APIs, `MessageStore::list_messages`, writer
      ingress types. Existing direct-SQLite prohibition retained.
- [ ] D3 — WriteOp purity gate: a `.just` deny-list checker (alongside
      the existing Python checks, `justfile:112+` / `.just/`) asserting
      the `WriteOp` enum declares no pure-read variant and the
      read-handler file contains no bridge/spawn-blocking strings.
- [ ] D4 — Liveness tests owned as a permanent CI gate: the AV.1b D5
      stalled-op + read-storm and bounded-overload tests are wired into
      `just test` and documented as a release gate (removal requires an
      ADR change).

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

- [ ] A1 — `BlockingCoreBridge` no longer exists in the workspace; a
      grep for it in `crates/` returns only historical docs/ADRs.
- [ ] A2 — Reintroducing `spawn_blocking` or a sync read API into the
      read-handler region fails `cargo test -p atm-architecture`
      (demonstrated once with a scratch mutation, then reverted).
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
