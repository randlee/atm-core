# ADR-007 — Supported Platform Feature Parity

| Field | Value |
|---|---|
| ID | ADR-007 |
| Status | **Accepted** |
| Date | 2026-05-08 |
| Deciders | Rand Lee |
| Relates to | REQ-P-PLATFORM-001, REQ-P-PLATFORM-002, REQ-DAEMON-PLATFORM-001, REQ-DAEMON-PLATFORM-002 |
| Supersedes | — |

---

## Context

Phase R merged a daemon architecture that was production-real on Unix but only
compile-clean on Windows. That is not acceptable for ATM `1.0`.

The missed requirement was not "Windows CI should pass." The actual product
expectation is:

- ATM features work on every supported operating system
- same-host daemon hosting is part of the product, not an optional Unix add-on

Without an explicit parity decision, it is too easy to accept:

- broad `#[cfg(unix)]` runtime gating
- Windows `daemon_unavailable(...)` stubs
- Unix-only functional transport coverage
- documentation that says "cross-platform" while the real implementation is
  still Unix-hosted

## Decision Drivers

- Windows is a supported ATM operating system
- compile-only support is not product support
- parity must be reviewable and enforceable, not just aspirational
- platform-specific implementation details must stay isolated behind owned
  boundaries

## Decision

ATM requires feature parity across all supported operating systems for the
retained `1.0` product surface.

Required rule:
- a retained feature is complete only when it works on macOS, Linux, and
  Windows with the same product-level behavior and typed error semantics

Allowed implementation differences:
- same-host local IPC adapter internals
- lifecycle-control source adapter internals
- host-ownership adapter internals

Forbidden end states:
- Windows compile-only support for daemon hosting
- permanent `daemon_unavailable(...)` stubs in supported same-host runtime
  paths
- Unix-only functional transport validation for a feature documented as
  supported on Windows
- business logic or dispatcher behavior that diverges by operating system

Test rule:
- shared infrastructure must prove the same handler/dispatcher contract on Unix
  and Windows
- platform-specific tests are allowed only for the adapter internals

## Consequences

### Positive

- product docs, implementation, and QA use one support contract
- cross-platform work is driven by owned boundaries rather than ad hoc cfg
  branching
- reviewers can reject "support" claims that stop at compilation

### Negative

- Phase S must do real Windows daemon work instead of a small CI cleanup
- existing Unix-shaped host assumptions require architectural extraction
- closeout requires more shared functional coverage

## Follow-Up Work

- add explicit product and crate-local parity requirements
- enumerate the allowed OS-specific implementation seams in architecture docs
- add boundary guardrails that reject unsupported-path stubs as a final state
- land shared Unix/Windows same-host functional coverage

## S.4 Closeout

Phase S.4 closed the temporary Windows lint narrowing and restored the full
workspace Windows `clippy -D warnings` gate in both local `just lint` and CI.

The closeout also added:

- a dedicated `same-host-portability` lint that rejects broad Unix-only
  same-host gating above the adapter layer
- a real local-IPC same-host request/response smoke test that runs through the
  shared frame helpers and daemon server transport on supported hosts
