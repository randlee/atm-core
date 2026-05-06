# ATM Testing Guidelines

## 1. Purpose

This document defines the required testing strategy for ATM after the daemon
singleton decision. It complements:

- [`requirements.md`](./requirements.md)
- [`architecture.md`](./architecture.md)
- [`adr/ADR-002-host-wide-daemon-singleton.md`](./adr/ADR-002-host-wide-daemon-singleton.md)
- [`adr/ADR-003-test-fidelity-and-daemon-isolation.md`](./adr/ADR-003-test-fidelity-and-daemon-isolation.md)

## 2. Default Rule

Ordinary ATM correctness tests must not require a real daemon process.

Most tests must not depend on:
- daemon spawn
- socket publication timing
- retry sleeps
- environment mutation races
- auto-start side effects

These patterns are treated as sources of flake and false confidence rather
than as acceptable test infrastructure.

## 3. Prohibited Patterns

The following patterns are prohibited in ordinary tests and are subject to the
singleton lint gate:

- `spawn_test_daemon`
- `warm_daemon`
- `DaemonGuard`
- `ATM_DAEMON_BIN`
- direct `Command::new(...atm-daemon...)`
- ad hoc daemon auto-start retries used as test stabilization
- fixed sleeps that attempt to wait for daemon socket publication
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

Use a loopback or in-process `ClientTransport` when tests need real request /
handler behavior without a real daemon process.

Required properties:
- implements the same shared `ClientTransport` contract
- routes requests to in-process dispatcher/handler logic
- preserves typed request/response behavior
- does not depend on socket publication or process supervision

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

## 5. Environment And Timing Rules

- Prefer explicit constructor parameters and injected test seams over shared
  process environment mutation.
- When environment variables are necessary, prefer `Command::env(...)` over
  mutating the parent process.
- Retry loops and sleeps are not correctness mechanisms.
- Bounded retry/sleep may appear only inside the dedicated daemon-runtime suite
  when required to observe a documented runtime invariant, and the reason must
  be explicit in the test.

## 6. Lint And CI Enforcement

The singleton/test-fidelity rule is enforced by a dedicated repository lint
gate integrated into `just lint`.

Initial planned entrypoint:
- `scripts/lint_daemon_singleton.py`

Required behavior:
- fail on prohibited daemon-spawn patterns in test code
- fail on new ad hoc daemon launch helpers
- fail on timing-based daemon stabilization patterns that bypass the approved
  test tiers

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
