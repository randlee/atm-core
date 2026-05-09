# ATM Testing Guidelines

## 1. Purpose

This document defines the required testing strategy for ATM after the daemon
singleton decision. It complements:

- [`requirements.md`](./requirements.md)
- [`architecture.md`](./architecture.md)
- [`adr/ADR-002-host-wide-daemon-singleton.md`](./adr/ADR-002-host-wide-daemon-singleton.md)
- [`adr/ADR-003-test-fidelity-and-daemon-isolation.md`](./adr/ADR-003-test-fidelity-and-daemon-isolation.md)

## 1.1 Backing Requirements

This guidance derives directly from:

- `REQ-P-TEST-001`
- `REQ-CORE-TEST-RUNTIME-001`
- `REQ-DAEMON-TEST-002`

## 2. Default Rule

Ordinary ATM correctness tests must not require a real daemon process.

Most tests must not depend on:
- daemon spawn
- socket publication timing
- retry sleeps
- environment mutation races
- auto-start side effects
- unbounded waits
- panic-unsafe shared/global test hooks

These patterns are treated as sources of flake and false confidence rather
than as acceptable test infrastructure.

## 3. Prohibited Patterns

The following patterns are prohibited in ordinary tests and are subject to the
singleton lint gate:

- `spawn_test_daemon`
- `warm_daemon`
- `DaemonGuard`
- `ATM_DAEMON_BIN`
- `atm-daemon.sock`
- direct `Command::new(...atm-daemon...)`
- launcher indirection such as `test_daemon_launcher(...)` used to hide the
  daemon binary behind helper resolution
- daemon-start retry helpers such as `is_daemon_start_transient(...)` when used
  to justify fixed warmup sleeps in ordinary tests
- ad hoc daemon auto-start retries used as test stabilization
- fixed sleeps that attempt to wait for daemon socket publication
- unbounded `recv()`, `wait()`, or equivalent blocking operations used as the
  correctness mechanism in risky daemon/runtime tests
- bare `join()` when the test has no prior bounded proof that the worker is
  already complete
- parent-process environment mutation when command-local `Command::env(...)`
  or explicit in-process injection can be used instead

There is no approved "test daemon launch" path for ordinary CLI or core
correctness tests.

## 4. Approved Test Tiers

### 4.1 Fake Transport Tests

Use `FakeClientTransport` for deterministic CLI/composition tests.

Required properties:
- implements the shared `ClientTransport` contract
- returns typed `ResponseEnvelope` / `AtmError` values directly
- any `AtmError` returned by the fake must use the registered ATM error-code
  inventory rather than ad hoc string-only failures
- never opens a socket
- never spawns `atm-daemon`
- plugs into `CliComposition::from_transport(...)`

Use this tier for:
- request construction
- response mapping
- error rendering
- command output behavior
- CLI-side observability and diagnostics shaping

### 4.2 Loopback Transport Tests

Use `LoopbackClientTransport`, an in-process `ClientTransport`, when tests need real request /
handler behavior without a real daemon process.

Required properties:
- implements the same shared `ClientTransport` contract
- routes requests to in-process dispatcher/handler logic
- preserves typed request/response behavior
- does not depend on socket publication or process supervision

The older term `test-socket` refers to this Tier 2 transport shape: an
in-process dispatcher-backed transport used for subsystem and daemon-boundary
tests without a real daemon process.

Use this tier for:
- service orchestration
- request dispatch integration
- handler-to-boundary interaction
- CLI-to-core behavioral integration without process flake

### 4.3 Daemon Runtime Tests

A narrow daemon-runtime suite may use a real daemon process only for daemon
requirements that cannot be proven in-process.

Allowed scope:
- singleton ownership
- startup failure when ownership is already held
- stale-owner recovery
- graceful shutdown
- signal handling
- framed socket transport behavior when required by daemon runtime invariants

Restrictions:
- these tests are not the ordinary correctness path
- they must remain small, explicit, and isolated
- they must not leak processes
- they must not validate unrelated CLI or business-logic behavior

### 4.4 Cross-Platform Same-Host Functional Coverage

Phase S adds one mandatory coverage layer for daemon hosting:

- same-host functional tests must exercise the real local-IPC boundary on Unix
  and Windows through shared dispatcher/handler infrastructure
- the harness shape must stay shared across supported operating systems; only
  the owned local-IPC adapter code may vary by platform
- Windows coverage must prove real daemon hosting behavior, not just
  compilation or unsupported-path errors
- Unix-only functional transport tests are insufficient once the parity line is
  active

## 5. Environment And Timing Rules

- Prefer explicit constructor parameters and injected test seams over shared
  process environment mutation.
- When environment variables are necessary, prefer `Command::env(...)` over
  mutating the parent process.
- Retry loops and sleeps are not correctness mechanisms.
- Tests must not contain a path that can block indefinitely.
- Bounded retry/sleep may appear only inside the dedicated daemon-runtime suite
  when required to observe a documented runtime invariant, and the reason must
  be explicit in the test.
- Fixed sleeps are prohibited in cross-platform same-host functional tests even
  inside daemon-host coverage; readiness and shutdown must use explicit
  synchronization, bounded protocol deadlines, or observable runtime state
- Shared/global test hooks must use panic-safe cleanup so failure paths cannot
  strand state into later tests

## 6. Lint And CI Enforcement

The singleton/test-fidelity rule is enforced by a dedicated repository lint
gate integrated into `just lint`.

Entrypoint:
- `scripts/lint_daemon_singleton.py`

Current status:
- this script exists and is the `R.10.4` lint-gate deliverable

Required behavior:
- fail on prohibited daemon-spawn patterns in test code
- fail on new ad hoc daemon launch helpers
- fail on timing-based daemon stabilization patterns that bypass the approved
  test tiers
- fail on newly-added cheap-to-detect unbounded wait patterns in the narrow
  same-host daemon/runtime suites once the rule family is promoted from S.5
  planning into the default lint path
- document an explicit allow-list for Tier 3 daemon-runtime suite patterns so
  the narrow exceptions remain auditable
- treat an empty allow-list as explicit "no approved exceptions"

Existing generic tools such as `clippy` are not sufficient on their own for
this repository-specific architectural rule.

## 7. Quality Bar

A test architecture is acceptable only if it increases confidence in the
production design.

The test suite must therefore optimize for:
- determinism
- explicit ownership boundaries
- typed error behavior
- production-faithful request/response contracts
- a narrow and auditable real-daemon runtime surface

Any new test strategy that makes daemon multiplicity, timing races, or hidden
auto-start side effects easier to rely on is a design regression.
