# Phase S Issues

Planning baseline:
- branch: `feature/pS-s0-planning`
- base: `integrate/phase-R`
- post-`PR #200` review baseline SHA: `d5e49df`
- follow-on CI-only compatibility fixes do not change the architectural issue
  set tracked here

## Closed Historical Planning Issues

1. Product requirement miss: full daemon functionality is expected on Windows,
   but the integrated daemon host shell only supports Unix-hosted same-host
   serving.

2. Same-host transport is modeled as Unix domain sockets in the active daemon
   docs and code shape, which blocks Windows daemon serving instead of
   abstracting it behind one local IPC boundary.

3. Lifecycle control is modeled as Unix signals instead of one platform-neutral
   shutdown/reload control source.

4. Active connection drain and forced-cancel mechanics are tied to Unix stream
   interruption semantics instead of a transport-neutral abort model.

5. Same-host daemon functional tests are Unix-only and do not prove Windows
   runtime behavior through a real local transport path.

6. Host ownership, lifecycle control, and local transport are not yet split
   into separate review-visible portability boundaries, which weakens lint and
   design enforcement for the Windows parity line.

7. The original Phase S plan was too abstract to execute safely: it did not
   name the exact integrated daemon files/methods that must be refactored in
   each sprint.

8. The original Phase S docs did not contain an explicit product-level feature
   parity requirement or ADR making Windows daemon functionality mandatory.

9. Same-host daemon test coverage was not yet specified as shared Windows/Unix
   infrastructure, which left room for Unix-only transport tests plus Windows
   compile-only support.

10. The original Phase S docs did not explicitly prohibit broad `#[cfg(unix)]`
    runtime gating or unsupported-path stubs as a final Windows support model.

11. Windows full-workspace clippy currently overreports dead-code churn in the
    non-Unix daemon path; CI temporarily narrows Windows lint scope by
    excluding `atm-daemon` until S.4 removes that guardrail after parity work
    lands.

12. PID liveness semantics remain a carried-forward seam from Phase R; Phase S
    preserves the current PID continuity model and does not redesign it unless
    a later ADR reopens that work explicitly.

13. The original Phase S host-ownership notes did not freeze a
    cross-platform-compatible lock-file shape; they left room for Unix-shaped
    deletion signaling instead of one stable `launch.lock` / `owner.lock`
    model with held-lock owner metadata.

14. The original Phase S anti-flake wording was too narrow: it prohibited
    fixed sleeps, but it did not define the broader no-flaky-test and
    no-unbounded-wait policy needed for cross-platform daemon work.

15. Phase S did not explicitly classify which anti-flake guardrails are
    feasible in the default `just lint` path versus which require deferred
    Rust-aware analyzer work.

16. Phase S did not explicitly require panic-safe cleanup of shared/global test
    hooks or ban unbounded wait shapes such as bare `recv()` / `wait()` /
    `join()` in the risky same-host daemon test surfaces.

Disposition:
- these planning issues are closed by the merged S.0-S.5 documentation and
  implementation line
- they remain listed here as the historical problem inventory that justified
  the Phase S sprint sequence

## Open Remaining Implementation / Process Issues

17. Phase S still carries four concrete post-S.4 daemon/runtime remediation
    items that must remain in the active sprint line until closed:
    - `RSH-001`
    - `RSH-014`
    - `WIN-001`
    - `ATM-QA-S4-001`

18. The mailbox-query redesign exposed by GitHub issues `#213` and `#214`
    remains unimplemented after S.4. `atm list`, single-message `atm read`,
    shared selector semantics, and bounded durable query behavior must stay
    assigned to follow-on Phase S execution work.

19. ATM-authored Claude JSONL compatibility projection still lacks the
    implemented export-cap, retrieval-stub, and watcher no-churn behavior
    accepted in ADR-010.

20. The triage process hardening line is incomplete until `qa-triage`,
    `triaging-findings`, and the phase integration-worktree ownership rule all
    agree on the canonical `triage_root`.

## Assigned Follow-On Sprint Ownership

- `S.6`
  - `RSH-001`
  - `RSH-014`
  - `WIN-001`
  - `ATM-QA-S4-001`
- `S.7`
  - `atm list`
  - single-message `atm read`
  - shared list/read filter surface
  - legacy `atm read` flag migration
  - bounded durable query behavior
- `S.8`
  - `[atm].claude_jsonl_body_export_max_bytes`
  - oversized ATM-authored retrieval-stub export
  - watcher/reconcile no-churn handling for ATM-authored projections

## Deferred Follow-Up

- `FTQ-001`
  - remains deferred to later lint/analyzer work
  - it is not treated as an S.6-S.8 code-implementation item in the current
    Phase S line
