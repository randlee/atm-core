---
phase: phase-W
branch: integrate/phase-W
head: 96f197e
participants: team-lead, arch-ctm, quality-mgr
date: 2026-05-14
integration_review: passed (QA-2 @ 96f197e — 0B+0I+0m)
---

# Phase W Post-Mortem

## Summary

Phase W delivered 8 sprints (W.1–W.8) closing the SQLite observability path and phase-W design gaps.
The integration review required 2 QA rounds (4B+2I+1m → 0B+0I+0m). All findings were fixed or
formally deferred. Total triage records: 80 across the phase.

**Phase-level outcome**: `integration_review_passed`

---

## Finding Family Analysis

### Family 1 — Sprint / Phase Doc Update Gaps

**Finding IDs**: NEW-001, NEW-002, NEW-003, NEW-004, ATM-QA-W3-008, ATM-QA-W7-001, ATM-QA-W7-002,
ATM-QA-W8-001, ATM-QA-W8-005, ATM-QA-W3-013

**Pattern**: Sprint docs (`sprint-WN.md`) launched without `status: complete`, `branch:`, or `worktree:`
fields set. `project-plan.md` sprint entries and execution-shape blocks missing W.3/W.5–W.8 data until
explicitly fixed. Sprint-W2.md body remained at planning state through the entire phase. Sprint-W7.md
and sprint-W8.md missing until end-gate catch.

**Root cause**: Sprint plan docs are not a required deliverable in the dev-template acceptance criteria.
arch-ctm completes implementation and ships code but has no explicit gate requiring the sprint doc and
project-plan.md entry to be present before close.

**Recommended action**: `planning_process_improvement`
- Add to `dev-template.xml.j2` AC section: doc deliverables block requiring
  (a) `docs/phase-X/sprint-XN.md` exists with `status: complete`, `branch:`, `worktree:` set, and
  (b) `docs/project-plan.md` section has the sprint entry added.
- Add to `qa-template.xml.j2` step (b): verify sprint doc and project-plan.md entry as explicit
  acceptance gate items (not just Rust output).

**Owner**: team-lead
**Target artifact**: `.claude/skills/codex-orchestration/dev-template.xml.j2`,
`.claude/skills/codex-orchestration/qa-template.xml.j2`

---

### Family 2 — Silent Emit Discards

**Finding IDs**: ATM-QA-W1-R2-001 through R2-006, ATM-QA-W1-R3-001, ATM-QA-W1-R3-002, NEW-EMIT-001

**Pattern**: `let _ = *.emit(...)` silently discards observability events. W.1 introduced
`emit_or_warn` but existing call sites across 5+ files retained the discard pattern. Multiline
`let _ =\n    x.emit(...)` form evaded the initial grep gate. W.3 introduced a regression
(NEW-EMIT-001) while fixing a W.3 finding, demonstrating the pattern recurs under normal dev pressure.

**Root cause**: No static lint or CI check enforcing `emit_or_warn` over `let _ = *.emit(...)`.
Pattern is mechanically detectable but requires explicit gate.

**Recommended action**: `new_lint`
- Add `scripts/check-silent-emit.sh` (or a Clippy lint) to CI: `grep -rn 'let _ = .*emit'` with
  multiline variant (`let _\s*=.*emit`). Fail on any match in non-test Rust.
- Consider adding to `.claude/skills/codex-orchestration/qa-template.xml.j2` a pre-flight check
  step for this pattern.

**Owner**: arch-ctm
**Target artifact**: `scripts/check-silent-emit.sh`, CI workflow

---

### Family 3 — Typed Observability API (Partial)

**Finding IDs**: ATM-QA-W1-R2-007, DESIGN-004-W, RBP-W3-F001, RBP-W6-F001, RBP-W6-F002, RBP-W8-F001

**Pattern**: W.8 completed `DaemonSubsystem` enum and typed `emit_subsystem_event` boundary. But
`DaemonEvent.action`/`outcome` fields, `SubsystemLogger` helpers, and `SubsystemObservability::event()`
still accept `&'static str`. The full typing chain (struct fields → constructors → call sites) remains
a partial migration. RBP-W8-F001 deferred because `sc-observability-types` lacks `const new_static()`.

**Root cause**: Incremental typed migration scoped per-sprint without a phase-level completion gate.
No requirements entry capturing the full migration path.

**Recommended action**: `requirements_update` + `architecture_update`
- Update `docs/requirements.md` to add a tracked requirement: full `ActionName`/`OutcomeLabel` typing
  at all `DaemonEvent` construction and `SubsystemObservability::event()` call sites.
- Update `docs/architecture.md` to note the intentional phased migration strategy and the upstream
  dependency on `sc-observability-types` adding `validated_static!` or `const new_static()`.
- Create Phase X sprint for completing the remaining typed boundary migration (76 call sites, 10 files).

**Owner**: arch-ctm (requirements update); team-lead (Phase X sprint planning)
**Target artifact**: `docs/requirements.md`, `docs/architecture.md`, Phase X plan

---

### Family 4 — Missing Trace / SQLite Events (Observability Coverage)

**Finding IDs**: ATM-QA-003–006, ATM-QA-011, ATM-QA-W2-R2-001, ATM-QA-W2-R2-002,
ATM-QA-W3-001–007

**Pattern**: W.1–W.3 added observability infrastructure but daemon runtime call sites were not audited
for coverage. DaemonSupervisor bootstrap missing initial-connect-miss, auto-start-timeout, and
launch-gate-timeout trace events. SqliteWriter shutdown and assembly paths missing emission. GraftClient
bootstrap using `NullObservability`, discarding graft traceability silently.

**Root cause**: Sprint AC for observability sprints specifies new paths but does not require a
coverage audit of existing paths touched by the sprint diff. Missing "what paths do I now own that
lack events?" check.

**Recommended action**: `qa_process_improvement`
- Add to `qa-template.xml.j2` for observability sprints: req-qa must verify that every code path
  touched by the sprint diff that can reach a user-visible failure has an emit call site.
- Add `scripts/audit-emit-coverage.sh` as a helper to list all error-path returns without adjacent
  emit calls (best-effort grep heuristic).

**Owner**: team-lead (template update); arch-ctm (coverage script)
**Target artifact**: `.claude/skills/codex-orchestration/qa-template.xml.j2`, `scripts/`

---

### Family 5 — RULE-002 Function Length (Pre-existing, Deferred)

**Finding IDs**: ARCH-W-001, ARCH-W-002, ARCH-W-003

**Pattern**: `send_to_endpoint` (108 lines), `send_once` (123 lines), `build_runtime_status_cache_state`
(102 lines) all exceed the 80-line RULE-002 limit. All three predate Phase W. Phase W reduced W-001 and
W-002 but did not refactor them below the limit. Deferred to Phase X.

**Root cause**: RULE-002 is not CI-enforced. Violations are only caught at QA review time, making
them expensive to fix mid-phase (non-trivial production I/O and cache logic).

**Recommended action**: `new_lint`
- Add a function-length check to CI: `scripts/check-function-length.py` scanning Rust source,
  failing on any non-test function exceeding 80 lines.
- Gate should warn at 70 lines (advisory) and fail at 80 lines (hard gate), mirroring the RULE-002
  threshold.
- Phased rollout: first pass as warning-only on all existing violations (grandfather list), hard-fail
  only on new violations introduced by a PR diff.

**Owner**: arch-ctm
**Target artifact**: `scripts/check-function-length.py`, CI workflow

---

### Family 6 — Code Duplication (IPC Helpers + Error Strings)

**Finding IDs**: ARCH-001, ATM-QA-008, ATM-QA-009, ATM-QA-W3-004

**Pattern**: `try_connect()` and `exchange()` duplicated across `crates/atm` and `crates/atm-graft`.
`DaemonUnavailable` error strings duplicated across submit/acquire_connection_guard. `unexpected_response`
error construction duplicated across crates. All raised during Phase W, none assigned to a sprint.

**Root cause**: No explicit sprint for IPC helper consolidation. Known gap queued but deferred across
multiple phases.

**Recommended action**: `project_plan_update`
- Add Phase X sprint: "IPC helper deduplication — extract try_connect/exchange into atm-daemon-client,
  consolidate DaemonUnavailable error strings and unexpected_response construction."
- Add to sprint AC: `grep -rn 'try_connect\|exchange'` finds exactly one definition per helper.

**Owner**: team-lead (Phase X planning)
**Target artifact**: `docs/project-plan.md`, Phase X plan

---

### Family 7 — Infallible Result + Mutex Rationale (RBP Ongoing)

**Finding IDs**: RBP-W4-F001, RBP-W3-F002, RBP-W7-F001 (infallible); RBP-W1-F001, RBP-W1-F002,
RBP-W4-F002, RBP-W6-F003 (mutex)

**Pattern**: `Result<T, E>` returns where `E` is never constructed (`heartbeat_message_key`,
`NullSqliteObservability::emit`, `replay_metadata_for_request`). Mutex/RwLock fields missing inline
rationale and lock-order comments (`advisory_runtime.rs`, `test_observability.rs`,
`RetainedJsonlFileSink`).

**Root cause**: Standard RBP patterns detected at QA-1. No pre-QA automation for either.

**Recommended action**: `test_hardening` (infallible) + `no_systemic_followup` (mutex rationale)
- For infallible results: add to `rust-qa-agent` checklist an explicit scan for `Ok(x)` as the
  sole return path in `Result`-returning functions. Low automation cost.
- For mutex rationale: ongoing manual RBP-QA vigilance is sufficient; no new gate needed.

**Owner**: arch-ctm (rust-qa-agent checklist update)
**Target artifact**: `.claude/assets/sc-rust/quality-mgr/templates/`

---

### Family 8 — Unstructured Logging (minor)

**Finding IDs**: ATM-QA-W1-R2-006, QA-W1-001, QA-W1-002

**Pattern**: `tracing::warn!()` calls in peer_transport.rs and local_ipc_transport.rs without
`subsystem=`, `action=`, `outcome=` structured fields. One instance at local_ipc_transport.rs:532
introduced in W.1 sprint work.

**Root cause**: No lint or template enforcing structured fields on daemon warn!/error! calls.

**Recommended action**: `new_lint` (low priority)
- Add advisory note to arch-ctm's dev guidelines: any `warn!`/`error!` inside `atm-daemon` must
  include `subsystem=`, `action=`, `outcome=` structured fields.
- Future: Clippy lint for unstructured `warn!` in daemon crate scope.

**Owner**: arch-ctm
**Target artifact**: `.claude/skills/rust-development/guidelines.txt`

---

## Deferred Findings (Phase X Obligations)

| ID | Title | Deferred To |
|----|-------|-------------|
| ARCH-W-001 | peer_transport.rs send_to_endpoint 108 lines (RULE-002) | Phase X |
| ARCH-W-002 | peer_transport.rs send_once 123 lines (RULE-002) | Phase X |
| ARCH-W-003 | runtime_status_cache.rs build_runtime_status_cache_state 102 lines (RULE-002) | Phase X |
| RBP-W8-F001 | SubsystemObservability::event() raw &'static str (typed-API) | sc-observability-types update |

Phase X planning must include sprint(s) to resolve ARCH-W-001/002/003 before the integration gate.

---

## Systemic Action Summary

| Action | Type | Owner | Target |
|--------|------|-------|--------|
| Add sprint doc + project-plan.md update to dev-template AC | planning_process_improvement | team-lead | dev-template.xml.j2 |
| Add sprint doc verification to qa-template step (b) | qa_process_improvement | team-lead | qa-template.xml.j2 |
| Add silent-emit-discard lint to CI | new_lint | arch-ctm | scripts/check-silent-emit.sh |
| Update requirements.md with full typed observability migration | requirements_update | arch-ctm | docs/requirements.md |
| Update architecture.md with phased migration strategy | architecture_update | arch-ctm | docs/architecture.md |
| Add observability coverage audit to qa-template | qa_process_improvement | team-lead | qa-template.xml.j2 |
| Add RULE-002 function-length lint to CI | new_lint | arch-ctm | scripts/check-function-length.py |
| Add Phase X sprint for IPC helper deduplication | project_plan_update | team-lead | docs/project-plan.md |
| Add infallible-result check to rust-qa-agent checklist | test_hardening | arch-ctm | sc-rust templates |
| Add structured-logging advisory to dev guidelines | new_lint (advisory) | arch-ctm | rust-development/guidelines.txt |
