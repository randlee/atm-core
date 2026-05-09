# ADR-008 — No-Flaky-Test Policy And Mechanical Enforcement

| Field | Value |
|---|---|
| ID | ADR-008 |
| Status | **Accepted** |
| Date | 2026-05-09 |
| Deciders | Rand Lee |
| Relates to | REQ-P-TEST-001, REQ-P-LINT-POSTMORTEM-001, REQ-CORE-TEST-RUNTIME-001, REQ-DAEMON-TEST-004 |
| Supersedes | — |

---

## Context

Phase S hardens same-host daemon parity across macOS, Linux, and Windows.
During implementation and QA, several failures were not ordinary logic bugs:

- Windows runs that could block long enough to consume CI time without naming
  the active test
- test logic that still depended on timing or process-scheduling luck
- runtime/test helper patterns that could strand threads, hooks, or global
  state after panic or timeout paths

The existing docs already reject fixed sleeps and timing-only stabilization, but
that rule is not strong enough by itself. A test can still be flaky or hang
indefinitely without using `thread::sleep(...)`.

ATM therefore needs one explicit policy:

- a test that might hang is not acceptable
- a test that depends on timing luck is not acceptable
- any prohibited pattern that can be detected mechanically must become a lint
  or CI gate instead of recurring QA rediscovery

## Decision Drivers

- Phase S parity work is cross-platform and CI-heavy; hanging tests waste
  review time and block multiple branches at once
- same-host daemon migration adds threads, channels, deadlines, shutdown, and
  lifecycle-control behavior that is especially vulnerable to hidden flake
- ATM already uses repository-local lint gates; the policy should be enforced
  there when feasible
- not every risky pattern is cheap to detect mechanically, so the enforcement
  plan must distinguish immediate rules from deferred analyzer work

## Decision

ATM adopts a repository-wide no-flaky-test policy for retained test surfaces,
with special emphasis on Phase S same-host daemon and runtime coverage.

### Required Policy

Tests must be:

- deterministic
- bounded
- explicit about readiness and shutdown predicates
- panic-safe in their cleanup of shared/global test state

Tests must not:

- depend on timing luck
- contain a path that can block indefinitely
- rely on missed-wakeup-sensitive synchronization without a bounded fallback
- leave cross-thread helpers running after the test has already concluded

### Required Synchronization Shape

Approved shapes include:

- channel handshakes with bounded timeout
- `Barrier`, `Condvar`, or latch/predicate synchronization with bounded wait
- readiness probes tied to explicit observable state on documented deadlines
- bounded shutdown/finalizer drain with observable completion

Forbidden shapes include:

- fixed sleeps as the primary correctness mechanism
- unbounded `recv()`, `wait()`, or similar waits in flaky-risk test paths
- bare `join()` when the test has no prior bounded proof that the worker has
  already completed
- retry-until-success loops with no explicit state predicate or deadline
- global test hooks or registries without panic-safe cleanup

### Mechanical Enforcement Rule

If a prohibited pattern is cheap and deterministic to detect, it must be
enforced in `just lint` or an equivalent CI gate. Review-only enforcement is
not sufficient for mechanically detectable cases.

## Enforcement Partition

### Feasible Now In Phase S

- fixed-sleep test hygiene checks, with the current repository-local rule
  treated as the proving implementation before `sc-lint` extraction
- repository-local daemon-spawn and warmup helper checks
- reusable production runtime liveness checks for:
  - bare `Condvar::wait(...)`
  - discarded `wait_timeout*` results
- repository-local targeted checks for unbounded test waits in the narrow
  same-host daemon/runtime suites when the syntax is cheap to detect

### Deferred Beyond Immediate Phase S

- proving every polling loop checks terminate state at the correct point
- proving every global test hook has panic-safe cleanup automatically
- proving every `join()` is safe without path-sensitive control-flow analysis
- proving every bounded wait inspects timeout state correctly inside test code

Deferred work is still required; it is not optional. It simply needs either
Rust-aware analyzer support or a narrower rule design before it can become a
default lint.

## Consequences

### Positive

- Phase S and follow-on daemon work get one explicit anti-flake contract
- reviewers can reject hang-prone tests by policy rather than by preference
- repository lints can grow from a clear source-of-truth document

### Negative

- some existing test/helper idioms will need redesign rather than stabilization
- a few desired lint families must remain planned work until their false-
  positive shape is understood

## Follow-Up Work

- add S.5 planning for phase-wide policy hardening and lint-family expansion
- tighten top-level and Phase S sprint language so “no fixed sleeps” becomes
  the broader “no flaky or unbounded waits” contract
- add a feasible-now vs deferred lint inventory to the Phase S planning docs
