# ADR-003 — Test Fidelity And Daemon Isolation

| Field | Value |
|---|---|
| ID | ADR-003 |
| Status | **Accepted** |
| Date | 2026-05-05 |
| Deciders | Rand Lee |
| Relates to | REQ-P-TEST-001, REQ-CORE-TEST-RUNTIME-001, ADR-002 |
| Supersedes | — |

---

## Context

The recent test harness grew around real daemon process spawning:

- `spawn_test_daemon`
- `DaemonGuard`
- `warm_daemon`
- `ATM_DAEMON_BIN`
- timing-based retry and sleep loops for socket publication

Those patterns are not harmless test conveniences. They couple ordinary tests
to:

- real Unix socket lifecycle
- singleton lock behavior
- process signal handling
- daemon publish races
- parent-process environment mutation

That produces flake and false confidence. A green run can mean "the timing
worked today" rather than "the design is production-worthy."

## Decision Drivers

- most tests should be deterministic and in-process
- real daemon process spawning is incompatible with the singleton requirement as
  an ordinary testing pattern
- CLI, core, and boundary behavior still need thorough coverage
- the replacement test architecture must improve confidence, not just reduce
  failures

## Options Considered

### Option 1 — Stabilize Existing Daemon-Spawn Tests

Add more retries, sleeps, warmup helpers, and launcher indirection.

**Rejected.** This optimizes the invalid pattern rather than correcting the
test architecture.

### Option 2 — Replace Ordinary Tests With In-Process Transport Seams

Use deterministic transport doubles for most CLI/composition and integration
coverage, while keeping a narrow daemon-runtime suite for true runtime
requirements.

**Accepted.**

### Option 3 — Delete Broad CLI Coverage And Test Only Lower Layers

Reduce CLI-surface tests heavily and push most coverage into `atm-core`.

**Rejected.** This lowers confidence in real request/response wiring and user
visible behavior.

## Decision

ATM adopts a layered testing model.

### Tier 1 — Fake Transport Tests

Use `FakeClientTransport` for deterministic CLI/composition tests.

Definition:
- an in-process implementation of `ClientTransport`
- used only in tests
- returns typed `ResponseEnvelope` or `AtmError` values directly
- never opens a socket
- never launches `atm-daemon`

Primary seam:
- `CliComposition::from_transport(...)`

### Tier 2 — Loopback Transport Tests

Use `LoopbackClientTransport`, an in-process `ClientTransport`, when tests need
real dispatcher or handler behavior without a real daemon process.

Definition:
- same `ClientTransport` contract
- routes requests to in-process dispatcher / handler logic
- preserves typed request/response behavior without process or socket timing

Naming note:
- the older term `test-socket` refers to this Tier 2 transport shape
- Tier 1 `FakeClientTransport` is a pure fake and does not dispatch to real
  handlers

### Tier 3 — Daemon Runtime Tests

Keep a narrow explicit daemon-runtime suite only for:
- singleton ownership
- startup rejection when ownership is already held
- stale-owner recovery
- graceful shutdown
- signal handling
- transport framing behavior when it is itself the runtime subject

These tests are not the ordinary correctness strategy.

## Prohibited Patterns

Ordinary tests must not depend on:
- daemon spawn
- socket publication timing
- retry sleeps
- environment mutation races
- auto-start side effects

Prohibited named patterns:
- `spawn_test_daemon`
- `warm_daemon`
- `DaemonGuard`
- `ATM_DAEMON_BIN`
- direct `Command::new(...atm-daemon...)`

## Enforcement

- a dedicated singleton lint gate is required in `just lint`
- existing generic tools such as `clippy` are not sufficient by themselves for
  this repository-specific rule
- tests that rely on prohibited patterns are architecture violations, not
  ordinary flaky tests

## Consequences

### Positive

- most tests become deterministic and in-process
- daemon runtime behavior is tested deliberately rather than incidentally
- CLI behavior remains testable through the real request/response seam

### Negative

- current daemon-spawning tests require redesign
- the test suite will temporarily lose some invalid coverage while the new
  seams replace it

## Follow-Up Work

- define `FakeClientTransport`
- define loopback transport requirements
- add the singleton lint gate
- remove the current daemon-spawn helpers from ordinary tests
- rewrite CLI tests around the approved transport seams
