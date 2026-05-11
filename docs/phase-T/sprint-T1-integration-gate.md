# Sprint T.1 — Phase-S Integration Gate Patch

```yaml
plan_type: sprint_plan
phase: phase-T
sprint: T.1
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-S
branch: integrate/phase-S
status: planned
estimated_scope: medium (16 findings: 2B+6I+8m — code + doc fixes, no new features)
```

## Goal

Fix all 16 open INTG-* findings on `integrate/phase-S` so that PR #231
(`integrate/phase-S → develop`) can clear the quality-mgr gate and merge.
After this sprint merges, `integrate/phase-T` fast-forwards to pick up the
fixes as the baseline for T.2–T.5.

## Scope Summary

All fixes target `integrate/phase-S` at `bdac03c`. The findings break into four
groups: two architectural naming violations (blocking), five runtime-shutdown
hardening gaps (important), three error-handling gaps (important), one doc
baseline drift (important), and five doc consistency minors plus one code
comment minor and one duplicate-fire-and-forget minor. One platform-limitation
doc addendum (ATM-QA-018) is included as a doc minor.

## Governing Requirements

- RULE-002: `pub(crate) fn emit_*` naming prohibited on library-crate concrete
  structs that are not trait impls
- REQ-P-RUNTIME-002: daemon shutdown must be bounded and orderly
- REQ-P-RUNTIME-003: multi-guard enforcement required for write-path correctness
- docs/architecture.md §daemon-runtime: shutdown deadline contract

## Governing ADRs

- ADR-002 (`docs/adr/ADR-002-host-wide-daemon-singleton.md`): owner-lock and
  serving-ownership separation — no changes, but INTG-RBP-003/004 fixes must not
  break acquisition/release sequencing
- ADR-ATM-RUSQLITE-002: single-writer design (informational for T.2, not T.1)

## Governing Boundaries

- `atm-daemon` crate: `DaemonRequestDispatcher` is a concrete internal struct;
  its methods must not carry `emit_*` names per RULE-002
- `crates/atm-daemon/src/runtime_health.rs`: `SHUTDOWN_FINALIZER_THREADS` is a
  `static Lazy<Mutex<...>>`; any lock acquisition in production code must
  propagate poison rather than panic

## Prerequisites

- `integrate/phase-S` worktree exists at
  `/Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-S`
- HEAD is `bdac03c` (all Phase S sprints merged: S.13 PR #226, S.14 PR #230,
  S.15 PR #228)
- All 16 INTG-* triage records committed at `bdac03c` under
  `.triage/phase-S/findings/`

## Hard Dependencies

None. All 16 fixes are independent of each other and can be landed in a single
commit. INTG-ARCH-002 auto-resolves when INTG-ARCH-001 rename is applied.

## Non-Goals

- SQLite single-writer lane implementation (T.2 / INTG-ARCH-001 is a rename
  only — the writer architecture is T.2 scope)
- Immutable message-row semantics (T.3)
- Windows runtime parity tests (T.4)
- RuntimeStatusCache all-conflict overflow fix (T.5)
- Shutdown deadline contract reconciliation — code vs architecture doc (T.5)
- Any new feature work

## Sub-Tasks

Each sub-task must be concrete and reviewable.

### ST-1: Rename `emit_runtime_event` → `record_lifecycle_event` [INTG-ARCH-001, INTG-ARCH-002] *(blocking)*

**Development work**

- `crates/atm-daemon/src/runtime_health.rs:452` — rename
  `pub(crate) fn emit_runtime_event` to `pub(crate) fn record_lifecycle_event`
  on `impl DaemonRequestDispatcher`
- `crates/atm-daemon/src/composition.rs:220` and all other call sites (~8 total)
  — update every `self.request_dispatcher.emit_runtime_event(...)` call to
  `self.request_dispatcher.record_lifecycle_event(...)`
- `crates/atm-daemon/src/daemon_runtime_observability.rs` — verify the trait
  `DaemonRuntimeObservability::emit_runtime_event` is a **trait method** (not
  affected by RULE-002) and leave its name unchanged; only the concrete
  free-standing wrapper on `DaemonRequestDispatcher` is renamed
- Search workspace for any other `emit_runtime_event` references and update

**Required tests**

- Existing tests in `tests.rs` that exercise the lifecycle event path must
  compile and pass with the new name — no new tests required for a pure rename
- `cargo clippy --workspace -- -D warnings` must pass on Windows-target path

**Doc/boundary updates**

- None — rename is internal `pub(crate)`, no public API surface change

---

### ST-2: Bounded join in `NotificationRuntime::shutdown` [INTG-RSH-005] *(important)*

**Development work**

- `crates/atm-daemon/src/notification_runtime.rs:105`
- Replace unbounded `handle.join()` with a 3-second bounded join mirroring the
  pattern used by other runtimes in the codebase:
  ```rust
  // current
  let _ = handle.join();
  // target
  if handle.join_timeout(Duration::from_secs(3)).is_err() {
      tracing::warn!("notification runtime thread did not exit within deadline");
  }
  ```
  (Use the actual bounded-join API available in the codebase — match the pattern
  in `WatchRuntime` or `ReconcileRuntime`.)

**Required tests**

- Add one unit test: `notification_runtime_shutdown_returns_within_bounded_deadline`
  — spawns the runtime with a no-op worker, calls `shutdown()`, asserts elapsed
  time is ≤ 5 s (same pattern as the bounded-shutdown tests in `tests.rs`)

---

### ST-3: Terminate flag before TCP connect in peer transport [INTG-RSH-006] *(important)*

**Development work**

- `crates/atm-daemon/src/peer_transport.rs:249`
- Add a terminate-flag check immediately before `send_once()` is called, so
  that a shutdown in-flight skips the TCP connect entirely:
  ```rust
  if self.terminate.load(Ordering::Relaxed) {
      return Err(AtmError::daemon_unavailable("peer transport shutting down"));
  }
  // existing send_once() call follows
  ```
- Verify the terminate flag is also checked between retry attempts (existing
  check during backoff must remain)
- Verify this covers the RSH-NEW-002 concern: terminate-flag gap before TCP
  connect in the same file. If RSH-NEW-002 describes an additional gap beyond
  this check, escalate as a separate minor item in the QA report.

**Required tests**

- Add one unit test or extend an existing test: assert that a peer transport
  send started after the terminate flag is set returns immediately with an error
  (does not block for the connect timeout)

---

### ST-4: Replace `.expect()` on static mutex in `run_bounded_shutdown_step` [INTG-RBP-003] *(important)*

**Development work**

- `crates/atm-daemon/src/runtime_health.rs:393` and `:396`
- Replace `.expect("shutdown finalizer threads")` with a pattern that propagates
  lock poison as an `AtmError` rather than panicking:
  ```rust
  let handles = SHUTDOWN_FINALIZER_THREADS
      .lock()
      .unwrap_or_else(|e| e.into_inner());
  ```
  (Recovering from poison is acceptable here — the alternative of returning an
  error from `run_bounded_shutdown_step` would require a signature change that
  is out of scope. Use `unwrap_or_else(PoisonError::into_inner)` to recover
  without panic.)

**Required tests**

- No new test required — the RAII guard in ST-5 prevents the poisoned-mutex
  scenario in practice; this fix is defensive production hardening

---

### ST-5: RAII drain guard for `SHUTDOWN_FINALIZER_THREADS` in tests [INTG-FTQ-008] *(important)*

**Development work**

- `crates/atm-daemon/src/runtime_health.rs` test section (or `test_support.rs`)
- Add a RAII guard type that drains `SHUTDOWN_FINALIZER_THREADS` on drop, and
  use it in `bounded_shutdown_step_does_not_exceed_retained_finalizer_cap` and
  any other inline test that pushes to the registry:
  ```rust
  struct FinalizerGuard;
  impl Drop for FinalizerGuard {
      fn drop(&mut self) {
          SHUTDOWN_FINALIZER_THREADS.lock().unwrap_or_else(|e| e.into_inner()).clear();
      }
  }
  ```

**Required tests**

- The existing `bounded_shutdown_step_does_not_exceed_retained_finalizer_cap`
  test must use the guard and must not leak state across parallel test runs

---

### ST-6: Propagate thread spawn failure in `schedule_delayed_listener_wake` [INTG-RBP-004, INTG-RSH-008] *(important + minor)*

**Development work**

- `crates/atm-daemon/src/local_ipc_transport.rs:718` (RBP-004) and `:708`
  (RSH-008)
- Replace `.expect()` on `thread::Builder::new().spawn()` at line 718 with
  `.map_err(|e| AtmError::daemon_unavailable(...).with_source(e))?`
- This resolves both INTG-RBP-004 (`.expect()`) and INTG-RSH-008
  (fire-and-forget silently swallowed) since the same spawn path covers both

**Required tests**

- No new test required — propagation is the correct behavior and is validated
  by the existing local IPC transport tests; a mock-spawn-failure test would
  require unsafe and is out of scope

---

### ST-7: Add rationale comment to `SHUTDOWN_FINALIZER_THREADS` static [INTG-RBP-005] *(minor)*

**Development work**

- `crates/atm-daemon/src/runtime_health.rs:44`
- Extend the existing comment to explain **why** a static is required (the
  finalizer threads must outlive any particular `DaemonRequestDispatcher`
  instance and must be drainable from the shutdown path regardless of ownership)

**Required tests**

- None — comment-only change

---

### ST-8: Add debug log on orphan path in `shutdown_lane_with_deadline` [INTG-RSH-007] *(minor)*

**Development work**

- `crates/atm-daemon/src/composition.rs:563`
- Add `tracing::debug!("shutdown_lane_with_deadline: thread {:?} orphaned on timeout", thread_id)`
  on the timeout/orphan path so operators can identify stuck threads in logs

**Required tests**

- None — observability-only change

---

### ST-9: Doc fixes — project-plan.md S.15 baseline [INTG-ATM-QA-001] *(important)*

**Development work**

- `docs/project-plan.md`
- Add one entry for Sprint S.15 in the Phase S sprint table/list so the
  top-level plan matches `docs/plan-phase-S.md §5`

**Required tests**

- None — doc-only

---

### ST-10: Doc fixes — five Phase S consistency minors [INTG-ATM-QA-002…006] *(minor)*

**Development work**

- `docs/plan-phase-S.md §5`: fix title drift — align entry for S.15 with
  `sprint-S15-rusqlite-hardening.md` title (INTG-ATM-QA-002); add
  `sprint-S15-rusqlite-hardening.md` to S.15 artifact list (INTG-ATM-QA-005)
- `docs/phase-S/sprint-S14-runtime-plan.md:49`: fix filename reference from
  `daemon_observability.rs` → `daemon_runtime_observability.rs` (INTG-ATM-QA-003)
- `docs/phase-S/` metadata format: add one comment or frontmatter note in the
  earliest S.10 sprint doc acknowledging the format change from YAML frontmatter
  (S.0–S.9) to flat bold headers (S.10–S.15); do not reformat all docs
  (INTG-ATM-QA-004)
- `docs/phase-S/sprint-S15-rusqlite-hardening.md`: add `REQ-P-RUNTIME-003`
  to the requirements reference list (INTG-ATM-QA-006)

**Required tests**

- None — doc-only

---

### ST-11: Platform-limitation note for SIGBREAK/console-only [ATM-QA-018] *(minor)*

**Development work**

- `docs/architecture.md` Windows section
- Add one sentence: "On Windows, SIGBREAK (Ctrl+Break / reload signal) is only
  delivered to console-attached processes; daemon instances running as a Windows
  service or in a detached process will not receive SIGBREAK and must be
  lifecycle-managed through the local IPC control port."

**Required tests**

- None — doc-only

## Split Recommendation

No split. All 16 fixes + 1 ATM-QA-018 addendum are independent, bounded, and
deliverable in a single commit on `integrate/phase-S`. Code changes span 6
files; doc changes span 5 files. Combined diff is well within one reviewable
PR.

## Acceptance Criteria

- `cargo check --workspace` passes on the integrate/phase-S worktree
- `cargo clippy --workspace -- -D warnings` passes (including Windows-target
  path via CI)
- `cargo test --workspace` passes — no regressions
- `emit_runtime_event` does not appear in any `impl DaemonRequestDispatcher`
  block (grep confirms)
- `NotificationRuntime::shutdown` join is bounded (≤ 3 s deadline)
- `schedule_delayed_listener_wake` spawn failure propagates as `AtmError`
- `SHUTDOWN_FINALIZER_THREADS.lock().expect(` does not appear in production
  code paths (only in test panic-recovery paths if any)
- `SHUTDOWN_FINALIZER_THREADS` RAII drain guard used in all inline tests that
  push to registry
- All doc files updated: `docs/project-plan.md`, `docs/plan-phase-S.md`,
  `docs/phase-S/sprint-S14-runtime-plan.md`,
  `docs/phase-S/sprint-S15-rusqlite-hardening.md`, `docs/architecture.md`
- QA-2 verdict: 0B+0I+0m

## Required Validation

```bash
# From integrate/phase-S worktree
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace

# Confirm ARCH-001 rename complete
grep -r 'emit_runtime_event' crates/atm-daemon/src/ --include='*.rs' \
  | grep -v 'trait\|fn emit_runtime_event' \
  | grep 'impl DaemonRequestDispatcher'
# Expected: no output

# Confirm no bare .expect() on static mutex in production code
grep -n 'SHUTDOWN_FINALIZER_THREADS.*expect' crates/atm-daemon/src/runtime_health.rs
# Expected: no matches outside #[cfg(test)] blocks

just check  # if available in worktree
```

## Required Document Updates

- `docs/project-plan.md` — add S.15 sprint entry under Phase S
- `docs/plan-phase-S.md` — fix S.15 title drift + add sprint-S15 to artifact list
- `docs/phase-S/sprint-S14-runtime-plan.md` — fix daemon_observability.rs filename
- `docs/phase-S/sprint-S15-rusqlite-hardening.md` — add REQ-P-RUNTIME-003
- `docs/architecture.md` — add SIGBREAK/console-only platform note
- `docs/phase-S/sprint-S10-daemon-retained-logger.md` (or earliest S.10 doc) —
  add metadata-format-change acknowledgement

## Risks And Watchouts

- **INTG-RSH-006 terminate check**: must not short-circuit sends when the flag
  is not set — only skip when `terminate.load(Ordering::Relaxed)` returns true.
  Existing retry-backoff check must remain in place.
- **INTG-RBP-003 poison recovery**: `unwrap_or_else(PoisonError::into_inner)`
  recovers from poison; if the recovered guard contains stale state, drain it
  rather than using it. Verify no subsequent logic assumes a clean registry
  after recovery.
- **RSH-NEW-002 overlap**: after INTG-RSH-006 is fixed, quality-mgr will
  verify whether the terminate-flag check before TCP connect fully covers the
  RSH-NEW-002 concern. If it does not (e.g., RSH-NEW-002 describes a separate
  gap later in the send path), quality-mgr will escalate as a new minor finding
  at QA-2.
- **INTG-ARCH-001 rename scope**: only rename the free-standing method on
  `DaemonRequestDispatcher`. Do NOT rename `DaemonRuntimeObservability::emit_runtime_event`
  — that is a trait method and is not prohibited by RULE-002. Renaming the
  trait method would be a breaking change and is out of scope.
- **CI xwin path**: after the rename, run a mental diff of all
  `composition.rs` call sites to ensure no Windows-only conditional paths were
  missed. The CI xwin job will catch any missed references.
