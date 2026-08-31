# AV.1b worktree checklist

Source: `docs/plans/phase-av/sprint-AV.1b-read-handler-cutover.md`.

- [ ] D1/A1/A7 — Cut list, peek, and read HTTP handlers to `AsyncMailboxRuntime`; cut doctor to `DoctorProjection`; preserve authorization, selection, visibility, and healthy-output parity; prove no bridge/read sync API/spawn-blocking reference.
- [ ] D2/A4 — Add the explicit `ApplyReadDisplayState` writer transition and the readiness-gated bounded `StateHandoffSupervisor`; implement fail-safe rejection, retry/restart/fail-closed lifecycle, metrics, and all specified corner-case tests.
- [ ] D3/A5 — Add a typed multi-worker async `DoctorProjection`, with independently bounded legs, healthy parity, per-leg degradation, explicit control-lane overload, and doctor/read fan-out tests.
- [x] D4/A6 — Delete `WriteOp::ListMessages`, the shared-db async writer-list method, and the async-store delegation; threaded-message projection now uses the bounded reader capability. Reader-focused storage tests cover the replacement path.
- [ ] D5/A2/A3 — Add deterministic router-fixture liveness and bounded-overload tests for concurrent list/peek/read/doctor requests.
- [ ] Closeout — Update sprint frontmatter (`status`, `branch`, `worktree`) and planning index; run lint, full tests, validate, architecture tests, and isolated live-stall proof; commit/push and notify Fenix.

## Checkpoint evidence

- D2 foundation: explicit SQLite `ApplyReadDisplayState` and the bounded
  `StateHandoffSupervisor` are implemented. Focused tests cover response-before-
  commit, buffer-full rejection, transient recovery, permanent writer failure
  with retained queue, and startup without a Tokio runtime. Handler cutover,
  metrics/doctor projection, and forced-worker-restart coverage remain open.
