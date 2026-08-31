# AV.1b worktree checklist

Source: `docs/plans/phase-av/sprint-AV.1b-read-handler-cutover.md`.

- [x] D1/A1/A7 — Cut list, peek, and read HTTP handlers to `AsyncMailboxRuntime`; cut doctor to `DoctorProjection`; preserve authorization, selection, visibility, and healthy-output parity; prove no bridge/read sync API/spawn-blocking reference.
- [x] D2/A4 — Add the explicit `ApplyReadDisplayState` writer transition and the readiness-gated bounded `StateHandoffSupervisor`; implement fail-safe rejection, retry/restart/fail-closed lifecycle, metrics, and all specified corner-case tests.
- [x] D3/A5 — Add a typed multi-worker async `DoctorProjection`, with independently bounded legs, healthy parity, per-leg degradation, explicit control-lane overload, and doctor/read fan-out tests.
- [x] D4/A6 — Delete `WriteOp::ListMessages`, the shared-db async writer-list method, and the async-store delegation; threaded-message projection now uses the bounded reader capability. Reader-focused storage tests cover the replacement path.
- [x] D5/A2/A3 — Add deterministic router-fixture liveness and bounded-overload tests for concurrent list/peek/read/doctor requests.
- [ ] Closeout — Update sprint frontmatter (`status`, `branch`, `worktree`) and planning index; run lint, full tests, validate, architecture tests, and isolated live-stall proof; commit/push and notify Fenix.

## Checkpoint evidence

- D2 foundation: explicit SQLite `ApplyReadDisplayState` and the bounded
  `StateHandoffSupervisor` are implemented. Focused tests cover response-before-
  commit, buffer-full rejection, transient recovery, permanent writer failure
  with retained queue, startup without a Tokio runtime, forced worker restart,
  and restart-budget exhaustion.

- D1/D3/D5: `StorageAndNudgeRouter` uses only `AsyncMailboxRuntime` for
  list/peek/read and `DoctorProjection` for doctor. The production-handler
  architecture guard rejects bridge use; twelve concurrent mailbox/doctor
  calls remain live while the residual bridge is occupied. The real bounded
  doctor lane accepts work within capacity and returns
  `DaemonConnectionSaturated` for excess concurrent requests.
- D2 lifecycle: forced worker cancellation now restarts and drains a retained
  transition; restart-budget exhaustion and permanent writer failure retain
  the queued transition and reject future handoffs explicitly.
- Validation complete: `just lint`; `just test` (916 passed, 21 skipped);
  `cargo test -p atm-architecture` (80 passed); and `just validate` (pass).
  The only remaining closeout item is the plan-required manual proof on a
  separately owned local daemon. This worktree intentionally does not touch
  the active single-owner daemon under `/Users/randlee/.atm`.
