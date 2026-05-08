# Phase S Issues

Planning baseline:
- branch: `phase-S-planning`
- base: `integrate/phase-R`
- baseline SHA: `6a072c1`

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
