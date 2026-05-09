# Phase S Issues

Planning baseline:
- branch: `feature/pS-s0-planning`
- base: `integrate/phase-R`
- post-`PR #200` review baseline SHA: `d5e49df`
- follow-on CI-only compatibility fixes do not change the architectural issue
  set tracked here

## Open Issues

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
