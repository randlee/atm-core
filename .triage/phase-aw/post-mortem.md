# Phase AW Post-Mortem

**Phase:** phase-AW (AW.1 – AW.5) — Unified retained runtime logging, graft observability, native tool parity
**Integration branch:** integrate/phase-aw
**Final QA commit:** f546c0c5a
**Integration review result:** `integration_review_failed` (superseded — see Family M)
**Date:** 2026-09-05
**Author:** quality-mgr

> **Status note:** The sprint-scoped QA ledger (70/70 findings fixed) closed cleanly and `qa-pr1199-r1` PASSed on f546c0c5a. However, four independent phase-ending readiness reviews run afterward on the same commit — arch-ctm's `review-phase-aw-r1`, and fenix's three reviewers (`review-phase-aw-r1-fenix-a` correctness/data-hygiene, `-fenix-b` flaky-test audit, `-fenix-c` architecture/boundaries) — together surfaced **4 new blocking + 12 new important + 14 new minor findings** on the same commit, none of which were in the original 70-record ledger (Families M, N, O below; several are duplicate confirmations of the same underlying defect across reviewers, called out explicitly where that occurs). quality-mgr independently re-verified the most consequential of these against actual source at f546c0c5a and confirms them as real, reproducible, correctly-severed findings — not false positives. **This supersedes the `qa-pr1199-r1` PASS verdict.** None of these are triaged into `.triage/phase-aw/findings/*.ttl` or dispatched for a fix yet; Rand reviews them before any fix dispatch. The architecture reviewer (fenix-c) confirms no legacy-daemon or boundary-violation regression — the new findings are contract/hygiene gaps, not a boundary breach.

---

## Finding Set Summary

| Metric | Count |
| --- | --- |
| Total findings triaged (sprint-scoped ledger) | 70 |
| Blocking | 33 |
| Important | 22 |
| Minor | 15 |
| Fixed | 70 |
| Waived | 0 |
| Deferred | 0 |
| Repeatable (flagged) | 5 (AW-PHASEGATE-003, AW1-RBP-F001, AW1-RBP-F002, AW2-QA-HARNESS-001, AW5-QA-DOC1) |
| **New, post-ledger findings (Families M+N+O, not yet triaged)** | **4 blocking, 12 important (2 are duplicate cross-reviewer confirmations, not separately counted), 14 minor** |

All 70 sprint-scoped findings closed before the final merge decision — independently confirmed by direct enumeration of every `.ttl` record under `.triage/phase-aw/findings/` at f546c0c5a, 0 remain `open`. That closure is real and is not being walked back. What changed after closure is a **separate, later readiness-review pass** (arch-ctm plus fenix's three independent reviewers) that looked at data-hygiene, test determinism, and architecture/boundary hygiene the sprint-scoped QA rounds were never scoped to check, and found 30 new defects (net of duplicates) on the same commit. See Families M, N, O below — this is the headline finding of this post-mortem.

---

## Finding Families

### Family A — Doc/status staleness (project-plan tables, sprint frontmatter, section headers)

**Findings:** AW-PHASEGATE-001, AW-PHASEGATE-002, AW-INTEG-002, AW2-QA-M2, AW5-QA-DOC1, AW1-QA-005, ATM-QA-AW5R1-005, AW4-QA-M1, RBQA-1181-F001 (9)
**Severities:** 3 important, 6 minor

**Description:** `docs/project-plan.md`'s AW sprint-status table and section-55 header repeatedly lagged actual sprint state (stuck at "AW.1" or "[IN PROGRESS]" after later sprints, or even after this PR's own closeout). Sprint docs (AW.2, AW.5) shipped without the `status:`/`worktree:` frontmatter present on siblings. Smaller drift: a dependency omitted from a narrative list, a doc/comment mismatch on a pyo3 attribute name, a documented field cited at the wrong line number, a documented error field that is never actually emitted.

**Root cause:** Missing lint or static verification — no gate checks that `docs/project-plan.md`'s sprint table and section header match the sprint docs' own `status:` field, and no gate checks that a sprint doc has the same frontmatter shape as its siblings before a sprint PR can PASS.

**Pattern recurrence:** YES — recurred in AW.2, AW.5, and again at phase-end gate review (AW-PHASEGATE-001/002), meaning even the phase-closeout commit itself repeated the pattern it was supposed to fix. Same defect class as `.triage/phase-Yb/post-mortem.md` Family A.

**Classifications:**
- `new_lint` — a doc-consistency check comparing each `sprint-*.md`'s `status:` field against its row in `docs/project-plan.md`'s phase table and against the section header bracket tag, run in CI (not just QA)
- `qa_process_improvement` — sprint QA checklist must explicitly diff sprint-doc frontmatter against a sibling sprint doc before issuing PASS

**Target artifacts:**
- `.just/run_lint.py` — add `sprint_doc_status_matches_plan_table` rule
- `.claude/skills/quality-management-gh/SKILL.md` — add frontmatter-parity check to sprint QA checklist
- `docs/plans/phase-aw/sprint-*.md` template — closeout step already exists per phase-Yb's fix; this recurrence shows it was not durable (`new_lint` is required this time, not another template note)

---

### Family B — Build/lint breakage discovered only at CI (non-flake)

**Findings:** AW2-CI-001, AW2-CI-002, AW2-CI-003, AW3-CI-001, AW3-CI-002, AW3-CI-003, AW3-CI-005, AW3-CI-006, AW5-CI-002 (9)
**Severities:** 9 blocking

**Description:** Compile errors (`E0063`, `E0282`), stale test mocks left after a refactor removed the code they mocked, boundary-lint violations (`pub` types breaking the private-implementation-type rule), and an already-merged arch-gate (`al8`/`av3`) silently breaking after a module move — all first surfaced by CI, not by the dev agent or by QA before dispatch. Matches Rand's own observation: **"I don't think I have ever seen AW.2/AW.3 CI green"** on first push.

**Root cause:** Weak QA scoping combined with missing local pre-dispatch verification — `just lint`/`cargo build --tests` was not run by the dev agent before pushing, so QA (and CI) discovered basic compile/lint failures that a local gate would have caught before a human or reviewer agent ever needed to look at the diff.

**Pattern recurrence:** YES — 9 of 70 findings (13%) are this exact class, spread across three of five sprints (AW.2, AW.3, AW.5).

**Classifications:**
- `qa_process_improvement` — dev-QA loop must gate on `just validate` (build + lint, not full test suite) locally before a QA dispatch is issued; a QA request against a branch that doesn't build/lint locally should be rejected at the door, not spent on a reviewer round
- `merge_forward_process_improvement` — the AW3-CI-002 case (previously-passing arch gates broke silently after a module move) shows gates must be re-run after every merge-forward, not just at sprint-branch tip

**Target artifacts:**
- `.claude/skills/team-lead/SKILL.md` / dev dispatch template — add "run `just validate` locally, attach output" as a required field before `qa-pr<N>-r1` dispatch
- `.claude/agents/quality-mgr.md` — QA intake step: reject dispatch if `cargo build --workspace --tests` or `just lint` fails on the reviewed branch

---

### Family C — Test flakiness / platform nondeterminism

**Findings:** AW1-CI-001, AW2-CI-004, AW3-CI-007 (3)
**Severities:** 3 blocking

**Description:** `atm-observability` unit tests raced on a shared process environment variable (`ATM_LOG`) run in parallel. Two separate golden-byte/source-contract tests broke only on Windows because they depended on LF-only line endings, which the Windows checkout silently converts to CRLF.

**Root cause:** Test coverage gap combined with missing lint — no rule forbids tests from mutating shared process-global state (env vars) without serialization, and no rule forbids line-ending-sensitive string/byte comparisons against `include_str!`/checked-out fixtures without an explicit LF-normalization step.

**Pattern recurrence:** Same defect class as flagged repeatedly in `flaky-test-qa`'s existing mandate; explicitly named by Rand as non-tolerable ("race conditions are not tolerated").

**Classifications:**
- `new_lint` — detect tests that set process-wide env vars without `#[serial]` (or the crate's existing serialization primitive) in the same file
- `test_hardening` — normalize all golden-fixture comparisons to LF before compare (`.replace("\r\n", "\n")`) as a standard helper, and require its use via a lint or a shared harness function
- `qa_process_improvement` — `flaky-test-qa` review should be a standing gate on any sprint touching shared process state or fixture-comparison tests, not just an ad hoc dispatch

**Target artifacts:**
- `.just/run_lint.py` — add `unserialized_env_mutation_in_test` rule
- `crates/atm-observability/tests/` and similar — shared `normalize_golden(bytes) -> Vec<u8>` helper
- `.claude/agents/flaky-test-qa.md` — confirm this class is in its standing scan checklist for future sprints

---

### Family D — Boundary mirror-guard / duplicate-ownership drift

**Findings:** AW-PHASEGATE-003, AW-INTEG-001, AW-INTEG-003, AW-INTEG-004, AW1-QA-B1, AW1-QA-B2, AW2-QA-M1, AW3-QA-M1, RBQA-AW2-F001, RBQA-F101-AW3, RBQA-F102-AW3, RBQA-F103-AW3 (12)
**Severities:** 2 blocking, 9 important, 1 minor

**Description:** The largest single family. Recurring pattern: a boundary TOML record or a Rust boundary-gate catalog omits a new crate/edge (`AW-PHASEGATE-003`, `AW-INTEG-001`); a gate's coverage has a structural blind spot (`AW-INTEG-003`'s first-`#[cfg(test)]`-marker truncation, `AW1-QA-B1`'s self-untested gate); a crate imports across a boundary directly instead of through the declared seam (`AW1-QA-B2`); two independently-owned classifiers silently share one machine-readable error code (`AW-INTEG-004`); or the exact same logic/response shape is hand-duplicated on both sides of a crate boundary with no shared owner (`RBQA-F101/102/103-AW3`, `AW3-QA-M1`, `AW2-QA-M1`, `RBQA-AW2-F001`).

**Root cause:** Missing boundary enforcement — `boundaries/*.toml` records and their Rust mirror-guard catalogs (`EXPECTED_FORBIDDEN_EDGES`, `guarded_boundary_files()`) require a human to remember to update both sides whenever a new crate/edge/type is introduced; there was no auto-discovery until the AW-PHASEGATE-003 fix in PR #1200.

**Pattern recurrence:** YES — this is the **second** occurrence of "a boundary TOML omitted from the Rust mirror-guard catalog" across phases (first: `.triage/phase-an/findings/QA-001-AN2.ttl`). The PR #1200 fix (`guarded_boundary_files()` → `all_boundary_files()` auto-discovery via `fs::read_dir` over `boundaries/*/*.toml`) is the correct systemic fix and should retire this specific sub-pattern going forward; the broader "hand-duplicated logic across a crate seam" sub-pattern (RBQA-F101/102/103-AW3) is not yet structurally prevented.

**Classifications:**
- `boundary_update` — (already delivered this phase for the TOML-mirror sub-pattern via PR #1200's auto-discovery fix; no further action needed there)
- `new_lint` — detect near-duplicate response-shape structs/enums declared independently on both sides of a crate boundary (the RBQA-F101/102/103-AW3 sub-pattern) — this is the still-open half of the family
- `architecture_update` — for machine-readable error codes shared across independently-owned classifiers (AW-INTEG-004 pattern): document a rule that a new classifier must mint its own `AtmErrorCode` variant rather than reuse one from another module, and add a lint or `arch-qa` checklist item for it

**Target artifacts:**
- `crates/atm-architecture/tests/boundary_enforcement.rs` — the auto-discovery fix already landed; add a companion structural-similarity check (AST hash or field-set comparison) for cross-boundary response types, flagged by RBQA-F101/102/103-AW3
- `docs/adr/` — new ADR: "machine-readable error codes are owned exactly once; a new classifier mints a new variant"
- `.claude/agents/arch-qa.md` — add explicit check item for shared-error-code reuse across classifiers

---

### Family E — Missing/insufficient test coverage vs acceptance criteria

**Findings:** ATM-QA-001, ATM-QA-AW2R4-001, ATM-QA-AW2R9-001, ATM-QA-AW5R1-002, ATM-QA-AW5R1-004, AW1-QA-004, AW1-QA-B3, AW1-RBQA-F002, AW2-QA-I1, AW2-QA-I2, AW3-QA-B4, AW3-QA-I1, AW4-QA-B2, AW4-QA-B4, AW4-QA-I1, RBQA-F003-AW5 (16)
**Severities:** 4 blocking, 11 important, 1 minor

**Description:** The largest count of any family (16/70, 23%). A named acceptance criterion or documented behavior existed but had zero or partial test coverage at initial QA: doc-test enforcement missing for a sprint's own logging doc, a source-scan test for a "no non-test reference" claim missing, concurrent-writer non-interference untested, a CLI parity test gated on a fixture flag that CI never sets (so it silently never ran), canonical-timestamp and deprecation-shim behavior untested, an entire module (`tracing_bridge.rs`) shipped with zero `#[test]`s, a writer-priority-under-load scenario untested, a saturation test that didn't exercise the real overflow path, restart-under-concurrency untested, a non-pending-ack CLI error path untested, and a doctor structured-observability block untested end-to-end.

**Root cause:** Weak sprint planning / weak QA scoping — acceptance criteria in the sprint docs name a behavior but don't require a specific named test for it, so "the code does X" and "a test proves X" silently diverge; QA's first pass (the reviewer round before follow-up) is scoped to what's in the diff, not to what the AC list requires.

**Pattern recurrence:** YES — every one of the five AW sprints (AW.1 through AW.5) has at least one finding in this family; same class as phase-Yb Family D.

**Classifications:**
- `sprint_plan_update` — every AC in a sprint doc must name its own proof test (`named_test:` field) at plan-authoring time, not left implicit
- `qa_process_improvement` — QA-1 checklist item: for every AC in the sprint doc, confirm a specific named test exists and actually executes (not just gated behind an unset fixture flag) before PASS

**Target artifacts:**
- `docs/plans/phase-aw/sprint-*.md` template (and future phase templates) — AC table gains a `proof_test:` column
- `.claude/skills/quality-management-gh/SKILL.md` — add "AC-to-test traceability" as an explicit QA-1 gate item, including verifying the test isn't skipped by an unset env/fixture flag in CI

---

### Family F — Function-length / file-size lint violations (RULE-002 / RULE-003)

**Findings:** AW2-QA-B2, AW3-CI-004, AW3-QA-B5, AW3-QA-B6, AW4-QA-B1, AW5-CI-001 (6)
**Severities:** 6 blocking

**Description:** Every one of the five sprints introduced at least one new hard violation of the existing, documented, machine-checkable RULE-002 (function length) or RULE-003 (file size) rules.

**Root cause:** Missing lint or static verification at development time — RULE-002/003 are QA-enforced, not locally enforced; a dev agent has no feedback until QA/CI runs.

**Pattern recurrence:** YES — identical to phase-Yb Family B, which already recommended `new_lint` as a CI-blocking gate; that recommendation was evidently not fully completed/enforced before phase-aw, since the same class recurred 6 times across 5 sprints in this phase alone.

**Classifications:**
- `new_lint` — this is a repeat of an already-issued phase-Yb recommendation; escalate from "add a lint" to "verify the lint actually blocks CI, not just QA," since it clearly did not prevent recurrence this phase

**Target artifacts:**
- `.just/run_lint.py` / CI workflow — confirm RULE-002/003 checks run as a required CI status check on every PR (not only during a QA dispatch), and if they already do, investigate why 6 violations still reached QA before being caught

---

### Family G — AW.3 log-query sprint delivered materially incomplete functionality vs its own acceptance criteria

**Findings:** AW3-QA-B1, AW3-QA-B2, AW3-QA-B3 (3)
**Severities:** 3 blocking

**Description:** `atm log --source merged` was not actually implemented (both `Timeline` and `Merged` routed through the same SQLite-only path, with no JSONL/graft-fallback read, no rank/seq tiebreak, and a hardcoded `source` field). `--since` and all but the first `--level` filter were silently dropped for `timeline`/`merged` sources. CLI queries bypassed the documented `GET /v1/diagnostics` daemon route entirely and read the store in-process instead.

**Root cause:** Unclear requirements combined with weak QA scoping — the sprint doc's acceptance criteria described the merged/timeline query surface at a level that a partial implementation could technically claim to satisfy; this was caught by QA, but the depth of the gap (three separate blocking findings on the sprint's central deliverable) suggests the AC itself needed concrete example inputs/outputs, not prose description.

**Pattern recurrence:** Localized to AW.3 this phase; no evidence of the identical failure mode elsewhere in phase-aw, so not (yet) a cross-phase recurring class.

**Classifications:**
- `sprint_plan_update` — acceptance criteria for query/filter surfaces should include concrete example queries and their expected output shape, not just a feature-name description
- `qa_process_improvement` — for CLI surfaces that claim to route through a documented API contract (`GET /v1/diagnostics`), QA must explicitly trace the call path end-to-end rather than trusting the CLI output shape alone

**Target artifacts:**
- `docs/plans/phase-aw/sprint-AW.3-*.md` (already fixed this phase) — pattern for future sprint docs describing CLI query surfaces
- `.claude/agents/req-qa.md` — add explicit call-path tracing requirement for any CLI command claiming to route through a documented HTTP/daemon contract

---

### Family H — Storage single-writer-lane architecture violations (AW.2)

**Findings:** AW2-QA-B1, AW2-QA-B3 (2)
**Severities:** 2 blocking

**Description:** `SqliteDiagnosticTimeline::prune` ran `DELETE`s on a pooled control-path connection instead of routing through the single-writer lane the phase plan explicitly mandated (`docs/plans/phase-aw/phase-aw-plan.md` §4: "AW.2 must not add a second SQLite writer connection"); the read-concurrency gate's allowlist also had to be amended to admit the new `WriteOp` variants the sprint needed.

**Root cause:** Architectural drift under a documented constraint — the phase plan named the rule explicitly, but nothing in the codebase mechanically prevented a new code path from opening a second writer/pooled connection for diagnostic writes.

**Pattern recurrence:** Localized to AW.2 this phase. Related to but distinct from Family D's boundary-mirror gaps (this is an ADR-level storage rule, not a crate-boundary TOML).

**Classifications:**
- `new_lint` — a source-scan (or `arch-qa`) gate asserting no `rusqlite::Connection` is opened for diagnostic writes outside the writer-lane module (already effectively partly covered by `AW-INTEG-001`'s "exactly one production construction path" test — confirm it also covers `prune`/DELETE call sites, not just construction)

**Target artifacts:**
- `crates/atm-architecture/tests/boundary_enforcement.rs` — extend the single-writer-path gate to cover mutation call sites, not just constructor call sites
- `docs/adr/ADR-ATM-RUSQLITE-002.md` — confirm the amendment AW.2 recorded is reflected accurately post-fix

---

### Family I — Parity/spec-consistency: sprint deliverables claimed compliance the design didn't actually provide

**Findings:** AW4-QA-B3, AW5-QA-B1, ATM-QA-AW5R1-003, RBQA-F002-AW5, RBQA-F001-AW5 (5)
**Severities:** 3 blocking, 2 important

**Description:** AW.4's sprint doc claimed ack-parity that `AtmAckResult`'s actual field subset could not satisfy (self-contradictory doc). AW.5's `to_json()` was a hand-rolled string-concatenation encoder rather than the shared `serde_json` path `output.rs` uses elsewhere, making CLI parity coincidental rather than structural. The "ack parity" test case was a conditional pass-through on an optional fixture field rather than the actual twin-message scenario the AC required. A redundant-field code-quality issue and a Python binding manifest omission (new pymodule classes not declared) round out the family.

**Root cause:** Weak sprint planning (claims not verified against the actual type/field surface before being written into the doc) combined with a QA gap (parity claims were not verified against the underlying implementation mechanism, only against surface output).

**Pattern recurrence:** Localized mostly to AW.4/AW.5 (the two "parity" sprints); plausible that any future parity-style sprint repeats this unless closed structurally.

**Classifications:**
- `qa_process_improvement` — for any sprint whose AC is "parity with X," QA must verify the *mechanism* achieves parity (shared code path), not just that current output happens to match
- `architecture_update` — CLI JSON output must go through one shared serialization path; a hand-rolled encoder for a subset of output should be flagged as an automatic boundary/req-qa finding, not something that requires a dedicated reviewer to notice

**Target artifacts:**
- `crates/atm/src/output.rs` (already fixed this phase for AW5-QA-B1) — establish this as the single sanctioned JSON-encoding path and note it in `docs/requirements.md`
- `.claude/agents/req-qa.md` — add "verify parity claims against implementation mechanism, not output snapshot" as an explicit check

---

### Family J — Test-infra/harness fixture staleness

**Findings:** AW2-QA-HARNESS-001 (1)
**Severity:** 1 important, repeatable

**Description:** `run_admission_capacity.py` silently reused a stale `target/release/atm-daemon-benchmark` binary across commits instead of rebuilding, risking benchmark results measuring the wrong code.

**Root cause:** Missing lint/static verification — no check that a benchmark harness's target binary's mtime/hash is newer than the source it's supposed to measure.

**Pattern recurrence:** Flagged `repeatable: true` by the original triage; single occurrence this phase but structurally capable of recurring in any benchmark harness.

**Classifications:**
- `test_hardening` — harness should assert (or rebuild) the probe binary's build timestamp against the current source tree before running, and fail loudly rather than silently reusing stale output

**Target artifacts:**
- `scripts/run_admission_capacity.py` (or wherever it now lives) — add a staleness assertion
- `[[project_readiness memory: benchmark infra is frozen at 8 sprints]]` — this is a behavioral fix inside an existing harness, not new infra, so it does not conflict with the standing benchmark-infra freeze

---

### Family L — Defensive-coding / code-quality gaps (RBP)

**Findings:** AW1-RBP-F001, AW1-RBP-F002, AW1-RBP-F003, AW1-RBP-F004 (4)
**Severities:** 2 important, 2 minor

**Description:** String-literal comparison instead of a typed enum for `retained.origin`; an invalid `CorrelationId` silently dropped via `.ok()` with no diagnostic counter; five distinct observability error codes mapped to one generic remediation string; a `Result`-returning function with an unreachable `Err` path in production.

**Root cause:** Missing lint / static verification for a set of well-known Rust defensive-coding anti-patterns that `rust-best-practices-agent` already checks for, but only at QA-1 (per this phase's own reviewer-cadence rule: "Rust best-practices review is QA-1 only").

**Pattern recurrence:** Two of the four (`AW1-RBP-F001`, `AW1-RBP-F002`) are flagged `repeatable: true`, implying the same anti-pattern likely recurs elsewhere in the codebase beyond AW.1's diff, but no phase-wide sweep for them is recorded.

**Classifications:**
- `no_systemic_followup` for this occurrence specifically — QA-1-only cadence for `rust-best-practices-agent` is an intentional, already-decided tradeoff (per this phase's own dispatch rules), not a gap to re-litigate here
- `test_hardening` — for the two `repeatable: true` findings, a workspace-wide grep sweep for the same string-literal-vs-enum and silent-`.ok()`-drop patterns should be scheduled as a standalone hardening task, independent of phase-aw's own QA cadence

**Target artifacts:**
- A dedicated cross-phase hardening ticket (not phase-aw scope) — workspace sweep for `retained.origin`-style string comparisons and `CorrelationId`-style silent `.ok()` drops

---

### Family M — Retained-data hygiene and retention-window violations, found only by phase-end readiness review (NEW, not in the 70-record ledger)

**Findings (informal ids, pending formal triage):** M1–M4 blocking, M5–M7 important, M8 minor-bundle (4 items)
**Sources:** arch-ctm `review-phase-aw-r1` (msg 01M1S6VD1Y1G7CK9WKKA42DVKM); fenix's reviewer-A `review-phase-aw-r1-fenix-a`
**Verification:** quality-mgr independently confirmed M1–M4 and M7 by direct source read at f546c0c5a (cited below); M5, M6, and the M8 bundle are accepted on the strength of two structurally-independent reviewers' converging line-level citations plus quality-mgr's own confirmation of the underlying query text for M6/M8.

| id | severity | claim | quality-mgr independent verification |
| --- | --- | --- | --- |
| M1 | blocking | `command_event_fields()` (`crates/atm-daemon-bootstrap/src/daemon_observability.rs:227-278`) inserts `team`, `agent`, `sender`, `message_id`, `task_id`, `error_message` directly into `LogEvent.fields`, bypassing the tracing bridge's `RETAINED_FIELD_ALLOWLIST` redaction path entirely; `crates/atm/src/output.rs::print_log_snapshot`/`print_log_records` serializes the full record via `serde_json`, so all of it reaches `atm log --json`. | CONFIRMED — read `daemon_observability.rs:227-300` directly: `fields.insert("team"...)`, `"agent"`, `"sender"`, `"message_id"`, `"task_id"`, `"error_message"` all present verbatim; `output.rs:231-249` confirmed unconditional `serde_json::to_string[_pretty]` of the full snapshot/record. `docs/atm-daemon/logging.md:7-13` documents "retained records never contain message bodies, recipients, tokens, raw environment/configuration, or absolute user paths" as the retained-log contract — this is a second, undocumented retained-sink boundary that does not honor that contract. |
| M2 | blocking | `tracing_bridge.rs` allowlists the *keys* `message` and `detail` by name, but their *content* is arbitrary interpolated text, copied verbatim (`RetainedVisitor::keep`, `record_str`/`record_debug`); the only existing test (`ac5_removes_sensitive_fields_and_values`) proves structured field **names** like `body`/`recipient`/`token`/`env` are dropped, not that free text interpolated into `message`/`detail` is safe. | CONFIRMED — read `tracing_bridge.rs:31-50` (`RETAINED_FIELD_ALLOWLIST` includes `"message"`, `"detail"`), `:366-378` (`keep()` stores `message` unconditionally when `field.name() == "message"`, bypassing the allowlist check that gates every other field), `:546-562` (`ac5` test only asserts named-field secrets don't leak, never exercises a warn! with an interpolated secret inside the message string itself). Two independent reviewers (arch-ctm, fenix) converged on this exact gap from different code paths (bridge internals vs. call-site grep), which increases confidence this is a real, not incidental, boundary hole. |
| M3 | blocking | The 7-day age-based prune (`WriteOp::PruneDiagnostics`) only runs when `diagnostic_rows_since_prune + batch_len >= DIAGNOSTIC_PRUNE_CHECK_EVERY` (500); the 250ms `start_flush_worker` background thread only flushes/observes and never independently triggers a time-based prune. A daemon receiving fewer than 500 diagnostic events total therefore never prunes, regardless of row age — violating sprint AW.2's own AC4 ("rows older than 7 days must prune regardless of count"). | CONFIRMED — read `writer/mod.rs:611-668`: `should_prune` is computed purely from the row-count threshold; `diagnostic_timeline.rs:70-89` (`start_flush_worker`) confirmed to call only `writer.flush_due()` on its timer, no prune call anywhere in that loop. |
| M4 | blocking | The count-based prune's DELETE (`writer/ops.rs:233-239`) is `DELETE ... WHERE id IN (SELECT id FROM diagnostic_events ORDER BY ts_unix_ms ASC LIMIT ?1 OFFSET ?2)` with `OFFSET = DIAGNOSTIC_MAX_ROWS` — in ascending order, this selects and deletes everything *after* the oldest `DIAGNOSTIC_MAX_ROWS` rows, i.e. the **newest** rows, not the oldest. Once at the 20k cap, the timeline freezes at its first 20k rows and every subsequent insert's excess gets pruned immediately, until the separate age-based prune eventually clears space. | CONFIRMED — read `ops.rs:230-239` directly; the ordering bug is exactly as described. The only existing regression test (`ac4_prune_reduces_a_25k_fixture_to_the_documented_row_bound`) asserts row **count** only, never which rows survive — so it cannot and does not catch this class of bug. |
| M5 | important | Both prune passes (age-based and count-based) run as unbounded per-scheduled-pass deletes inside one `Immediate` transaction on the shared, priority-biased but non-preemptible durable writer lane; a large expired/backlogged table can monopolize the writer and add latency to primary (non-diagnostic) writes. | Accepted on arch-ctm's citation (`writer/ops.rs:214-244`, `writer/mod.rs:564` biased-lane design) — consistent with quality-mgr's own read of the same file for M3/M4; not independently re-derived beyond confirming the cited lines exist and match the description. |
| M6 | important | `GET /v1/diagnostics` advertises/accepts up to 5000 rows (`diagnostics_route.rs`) but the storage layer silently clamps to 1000 (`diagnostic_timeline.rs:91` — confirmed directly, `.min(1_000)`), with no cursor/`next_cursor` and no route-specific read-concurrency or deadline bound, so a 20k-row retained timeline is neither fully traversable nor explicitly admission-bounded. | CONFIRMED the storage-side clamp directly (`let limit = query.limit.unwrap_or(100).min(1_000) as i64;`); route-side 5000 cap and absence of cursor/concurrency control accepted on both reviewers' converging citations. |
| M7 | important | `/v1/diagnostics?level=` performs a **lexical string comparison** (`level >= ?3` on a TEXT column storing lowercase level names) rather than a severity-rank comparison; alphabetically `"error" < "info"`, so `level=info` silently drops every `error` row from the result set. No test exercises `level_at_least` semantics. | CONFIRMED — read the exact query text (`diagnostic_timeline.rs:92`): `"... AND (?3 IS NULL OR level >= ?3) ..."` against a column populated with the literal strings `"error"`/`"warn"`/`"info"`/`"trace"`; lexical ordering does put `"error"` below `"info"`, exactly as fenix described. The CLI itself is unaffected because it filters client-side after fetching (`crates/atm/src/commands/log.rs:232-238`) — but any direct HTTP consumer of the route is exposed. |
| M8 | minor (bundle, 4 items) | (a) `component LIKE (?4 \|\| '%')` has no `ESCAPE` clause, so a component value containing `%`/`_` behaves as a wildcard, not a literal (bounded by the 20k/1000 caps, not a DoS, but incorrect filtering); (b) daemon bootstrap hard-fails if a global tracing subscriber is already installed (`BridgeError::AlreadyInstalled` → bootstrap error) — not reachable today since nothing else installs a subscriber first, fragility only; (c) `register_bridge`/`ACTIVE_TIMELINE`/`ACTIVE_COUNTERS` use `let _ = X.set(...)`, silently discarding a failed re-attach, and `start_flush_worker`'s loop has no stop signal; (d) "queue is full" detection is a brittle `error.message().contains(...)` string match rather than a typed error variant. | CONFIRMED (a) and (c) by direct source read (`component LIKE` query text; `let _ = INSTALLED_BRIDGE.set(bridge)` and the flush-worker's unconditional `loop`); (b) confirmed by reading `daemon_observability.rs:94-99`'s `AlreadyInstalled => AtmError::observability_bootstrap(...)` hard-error mapping; (d) accepted on fenix's citation without a separate independent grep (low-severity, low-ambiguity claim). |

**Root cause:** This is a **direct miss of Rand's standing data-hygiene constraint** (`docs/atm-daemon/logging.md`'s "retained records never contain message bodies, recipients, tokens, raw environment/configuration, or absolute user paths" guarantee) by every sprint-scoped QA round this phase. The sprint-scoped reviewers (req-qa, arch-qa, rust-qa-agent, ruthless-boundary-qa) each verified their own scoped acceptance criteria and boundary rules, and the phase's own AC5 test (`ac5_removes_sensitive_fields_and_values`) verified the allowlist mechanism works for the specific secret **field names** it was written to check — but no reviewer role in this phase's QA cadence was scoped to ask "does the retained-log contract hold for arbitrary interpolated free text, and does every retained-sink boundary (not just the tracing bridge) honor it?" That is a **missing boundary enforcement** + **weak QA scoping** compound cause: the redaction guarantee is enforced as a field-name allowlist at exactly one of at least two retained-sink boundaries (the tracing bridge), and `DaemonObservability::command_event_fields()` is a second boundary that was never subject to the same check. M3/M4 (retention-window and prune-ordering bugs) are a distinct root cause — a test-coverage gap: `ac4_prune_reduces_a_25k_fixture_to_the_documented_row_bound` proves the row **count** invariant post-prune but never asserts **which** rows survive or **when** prune fires relative to daemon activity level, so a correctness bug in the core mechanism the AC exists to guarantee went undetected by the test written specifically to prove that AC.

**Pattern recurrence:** This is the first occurrence of this specific defect shape (retained-data content-hygiene bypass via a second, unchecked retained-sink boundary) in the ledger, but it rhymes with Family D's boundary-mirror-guard pattern (a rule enforced at one point in the codebase silently has a second, unenforced entry point) and Family E's test-coverage-vs-AC gap (a test exists, but proves a weaker property than the AC actually requires).

**Classifications:**
- `boundary_update` — establish a single, mandatory redaction/sanitization boundary that every retained-sink write path (tracing bridge AND `DaemonObservability::command_event_fields`) must pass through; add a boundary-lint or `arch-gate` test asserting there is exactly one code path that can construct a persisted `LogEvent`/`DiagnosticEvent`, and that path enforces the allowlist-and-content-check together, not allowlist-of-names alone
- `test_hardening` — (1) add an adversarial test that a `warn!`/`error!` call with dynamic, secret-bearing content interpolated into `message`/`detail` does not reach retained JSONL/SQLite/CLI-JSON output (closes M2); (2) rewrite `ac4_prune_reduces_a_25k_fixture_to_the_documented_row_bound` to assert **surviving-row identity** (highest timestamps), not just count (closes M4); (3) add a quiet-daemon/low-volume clock-driven test asserting age-based prune fires independent of batch volume (closes M3)
- `new_lint` — integer-rank comparison for level filtering instead of string comparison (closes M7); `ESCAPE` clause or literal-match helper for all user-supplied `LIKE` patterns (closes part of M8)
- `architecture_update` — new ADR or amendment to the existing retained-log contract doc (`docs/atm-daemon/logging.md`) making explicit that the guarantee applies to **every** retained-sink boundary, not implicitly just the tracing bridge, and naming both current boundaries
- `qa_process_improvement` — add "trace every retained-sink boundary, not just the one the diff touches, against the documented retained-log contract" as a standing phase-end (not just sprint-scoped) QA checklist item — this is precisely the class of cross-cutting, whole-system property that a sprint-scoped reviewer, correctly bounded to their sprint's diff, cannot be expected to catch, and that only a phase-ending or arch-level review will surface

**Target artifacts:**
- `crates/atm-daemon-bootstrap/src/daemon_observability.rs` — route `command_event_fields()` through the same redaction boundary as the tracing bridge, or drop identity/error-message fields from the retained projection entirely
- `crates/atm-observability/src/tracing_bridge.rs` + `crates/atm-storage-rusqlite/src/writer/ops.rs` — sanitize/bound `message`/`detail` content; fix the prune DELETE ordering; add an independent time-based prune trigger
- `crates/atm-storage-rusqlite/src/diagnostic_timeline.rs` — integer-rank level comparison; `ESCAPE` on `LIKE`
- `docs/atm-daemon/logging.md` — amend contract to name all retained-sink boundaries explicitly
- `.claude/skills/quality-management-gh/SKILL.md` or a new phase-end QA checklist — add the cross-boundary retained-log-contract trace as a standing phase-end item

---

### Family N — Wall-clock-polling test determinism and host-isolation gaps, found only by a dedicated flaky-test readiness audit (NEW, not in the 70-record ledger)

**Findings (informal ids, pending formal triage):** N1–N4 important, N5–N8 minor
**Source:** fenix's reviewer-B `review-phase-aw-r1-fenix-b-flaky` (flaky/racy test audit), integrate/phase-aw @ f546c0c5a
**Verification:** quality-mgr independently confirmed N5 (the most consequential item) by direct source read; N1–N4 and N6–N8 accepted on the reviewer's specific, falsifiable line-level citations (busy-poll loops and wall-clock deadlines are a well-defined, low-ambiguity class to identify by inspection) and cross-checked against the reviewer's own explicit finding that AW2/AW3's CI failures were **not** timing races (AW1-CI-001's true race was fixed by redesign, not a widened timeout) — a claim consistent with Family C's own closure evidence.

| id | severity | claim |
| --- | --- | --- |
| N1 | important | `tracing_bridge.rs` tests (`ac1`, `ac2`, `ac5`) busy-poll the retained JSONL file for ≥N lines with a 2s wall-clock cap and no synchronization with the async flush worker; on expiry the test silently reads partial content instead of failing loudly. |
| N2 | important | `ac4_queue_full_offer_is_non_blocking` (`tracing_bridge.rs:527-536`) asserts `elapsed < 1s` as its non-blocking proof instead of asserting the actual `QueueFull` variant/drop counter. |
| N3 | important | `diagnostic_timeline.rs:177-188`'s `fresh_database_migrates_and_writer_lane_persists_a_diagnostic` polls `query()` to a 2s deadline after an async `try_send`, instead of using the diagnostic lane's own FIFO `Prune{reply}` signal to synchronize. |
| N4 | important | `atm-graft-python/src/lib.rs` `test_session()` (5 call sites) shares `std::env::temp_dir()/atm-graft-python-tests` as its fallback-log path across parallel tests and concurrent test runs on the same host — latent (nothing currently asserts on the file) but the exact shared-mutable-path class Family C already exists to catch. |
| N5 | important (environment/host-safety) | `crates/atm-graft-python/tests/test_cli_parity.py` isolates `HOME`/`ATM_HOME`/`ATM_LOG_DIR`/`TMPDIR`, but `crates/atm-core/src/home.rs::current_host_runtime_scope` **by explicit design** (confirmed: the function has an inline comment stating it intentionally ignores `HOME`/`ATM_HOME`/cwd, resolving via `getpwuid` instead, because those are process-scoped and cannot define a host-wide daemon singleton boundary) resolves `owner_lock`/`socket`/`durable_state_root` from the real OS account home. The "disposable" parity-test daemon therefore owns the real `~/.atm/daemon` and writes the real `~/.atm/db`; any live daemon already running on the host will collide on `owner_lock`. This is the same design tension already on record as a phase-aw follow-up ("parity fixture owner.lock isolation," referenced in the AW5-CI-002 closure note) — this review adds that it now also carries hang hazards: `readline()` has no timeout, `stderr=PIPE` is never drained (>64KiB output stalls the subprocess), and `CliParityTests._cli` has no subprocess timeout at all. Currently masked because CI only runs this suite once per job, gated by `ATM_CLI_PARITY_CI=1`, on fresh GitHub-hosted VMs with no other daemon present. |
| N6-N8 | minor | A `OnceLock`-backed "warn once" test asserts exactly one warning and breaks under test-order interference; `Instant::now() - 61s` can panic on a fresh VM with a young monotonic clock; `ac7_second_global_install_is_rejected` depends on no other test installing a tracing subscriber globally first. |

**Root cause:** Test coverage gap of the same class already named in Family C (test flakiness / platform nondeterminism), but discovered by a *dedicated, systematic* flaky-test sweep rather than by a CI failure forcing investigation. This confirms a pattern already visible in Family C's own accounting: the sprint-scoped QA cadence catches a race only after it manifests as a CI failure (reactive), and a phase-end dedicated audit (proactive) finds several more of the same shape that happened to not yet flip a CI run red. N5 has a second, distinct root cause: an architectural design decision (host-daemon singleton resolution intentionally ignores process-scoped env overrides) that is correct for production but was never reconciled with the test-isolation strategy that assumes those overrides work — a **requirements/architecture consistency gap** between `home.rs`'s documented intent and `test_cli_parity.py`'s isolation assumption.

**Pattern recurrence:** N1-N4/N6-N8 are the same defect class as Family C (3 prior instances), now with 7 more instances found by a dedicated sweep — reinforcing that Family C's `new_lint` recommendation (detect wall-clock polling loops and unserialized shared paths in tests) is warranted, not merely a one-off. N5 is a recurrence of an **already-identified, already-deferred** follow-up (noted at AW5-CI-002 closure) — this review is evidence that deferral should not continue past this phase, since the review independently rediscovered it and added new hang-hazard details.

**Classifications:**
- `new_lint` — extend Family C's recommended lint to flag `Instant`/`Duration`-based deadline loops and bare `std::env::temp_dir()`-based shared paths in `#[test]` functions (covers N1-N4, N6-N8 collectively)
- `test_hardening` — redesign N1-N3 to synchronize on the actual completion signal (a test-only `flush()`/`drain()` op) instead of polling; give `test_session()` a per-test `TempDir` (N4); add `readline()`/subprocess timeouts and drain `stderr` in `test_cli_parity.py` (part of N5)
- `architecture_update` — **do not** relax `current_host_runtime_scope`'s production behavior; instead give tests an explicit, narrowly-scoped override path (e.g. an `ATM_HOST_RUNTIME_ROOT` override honored only under a `--peer-wire-security plaintext-test` flag, per fenix's proposal) so parity tests can genuinely isolate from the real host daemon without weakening the production singleton guarantee
- `qa_process_improvement` — the phase-aw AW5-CI-002 closure note deferred this exact issue once already; a deferred finding referenced in a closure note needs a tracked follow-up item with an owner and target phase, not just a prose mention, so it doesn't require an independent review to rediscover it

**Target artifacts:**
- `crates/atm-observability/src/tracing_bridge.rs`, `crates/atm-storage-rusqlite/src/diagnostic_timeline.rs` tests — replace polling with signal-based synchronization
- `crates/atm-graft-python/src/lib.rs::test_session` — per-test `TempDir`
- `crates/atm-graft-python/tests/test_cli_parity.py` + `crates/atm-core/src/home.rs` — add the scoped test-only runtime-root override; add subprocess timeouts and stderr draining
- A tracked follow-up ticket (not just a closure-note mention) for "parity fixture owner.lock isolation" with an explicit owner and target phase

---

### Family O — Architecture/boundary readiness findings: stale rule text, counter-ownership bug, duplicate shapes, missing admission on auxiliary routes (NEW, not in the 70-record ledger)

**Findings (informal ids, pending formal triage):** O1–O6 important, O7–O16 minor
**Source:** fenix's reviewer-C `review-phase-aw-r1-fenix-c-arch` (architecture/boundaries), integrate/phase-aw @ f546c0c5a
**Overall verdict from this reviewer:** no blocking boundary or legacy-daemon violation — legacy `crates/atm-daemon/` diff is empty, no forbidden Cargo edge, 93/93 boundary-enforcement tests pass, ADR-014 respected. This is the one readiness reviewer that confirms a clean bill on the standing legacy-daemon-freeze and boundary-discipline rules; its findings are narrower contract/hygiene gaps, not architecture violations.
**Verification:** quality-mgr independently confirmed O1 and O2 are the same underlying defects already verified as M7 and M6 respectively (level lexical-compare; 5000-vs-1000 cap mismatch) — two independent reviewers converging on identical line numbers is strong corroboration, not a new finding to re-count. quality-mgr independently confirmed O4 (`refresh_persisted_stats` unconditionally `.store()`s over `timeline_dropped_persist_error_total`, silently discarding whatever `flush_locked` had already added for bootstrap-side non-queue-full errors — read directly at `diagnostic_timeline.rs:240-249`). O3 corroborates fenix-A's minor #8 (brittle string-sniffing) but elevates it: the string-match crosses the `atm-storage` → `atm-daemon-bootstrap` crate boundary, not just an internal brittleness. O5 and O6 accepted on citation (specific ADR/rule paragraph and OnceLock call-site references); not independently re-derived given diminishing marginal value at this point in the review.

| id | severity | claim | status |
| --- | --- | --- | --- |
| O1 | important | Same defect as **M7** (lexical `level >=` string compare drops `error` rows under `level=info`/`warn`). | Duplicate of M7 — counted once in Family M, not double-counted here. |
| O2 | important | Same defect as **M6** (route advertises 5000, storage silently clamps to 1000, no signal to caller). | Duplicate of M6 — counted once in Family M, not double-counted here. |
| O3 | important | `queue is full` string-sniffing (`.message().contains(...)`) crosses the `atm-storage` → `atm-daemon-bootstrap` crate boundary to classify drop counters; `DiagnosticTimelineStore::record_batch`'s contract returns an untyped `AtmError` instead of a typed offer enum. | CONFIRMED (elevates fenix-A minor #8 to important — a cross-boundary contract gap, not just brittle matching). |
| O4 | important | `refresh_persisted_stats()` unconditionally `.store()`s the rusqlite-side `persist_error_total` into `timeline_dropped_persist_error_total`, overwriting (not summing with) whatever `flush_locked()` already added there for bootstrap-side `WriterClosed`/`InvalidBatch` drops — those drops become invisible to `/v1/health`. | CONFIRMED by direct read (`diagnostic_timeline.rs:220-249`) — genuine counter-ownership/"honest loss semantics" bug. |
| O5 | important | `RULE-001` (`.claude/agents/arch-qa.md:59-65`) and `ADR-020`'s review conditions still confine the `sc-observability` import exception to a specific legacy-daemon module; `AW1-QA-B2` was closed by *moving* the import to `atm-observability` rather than updating the rule/ADR text to name the new owner, and the new facade (`RetainedLogger`) leaks backend types (`sc_observability_types::LoggingHealthReport`/`LogEvent`/`ServiceName`/`Level`) through its public signatures, violating ADR-020's own opacity condition. | Accepted on specific file/line/paragraph citation; this is the clearest instance this phase of Family D's recurring pattern (a rule enforced at one point silently grows a second, textually-unreconciled exception). |
| O6 | important | `attach_timeline` is not idempotent: a second call would swap the bridge's sink to a new writer and spawn a second flush thread while leaving the first `OnceLock`-held writer/counters live, so the original sink would stop being flushed on cadence. Not reachable today (called once), but nothing guards against a future second call. | Accepted on citation; consistent with fenix-A minor #7's related `OnceLock`/flush-thread observations (same file, same design shape). |
| O7-O16 | minor | Duplicate counter-shape structs (`AtmJsonlObservabilityCounters` vs `JsonlDiagnosticCounters`, field-for-field); duplicate `truncate_utf8`/`truncate_detail` truncation logic; a `lint_rules` id in a boundary TOML that resolves to no real rule; a boundary record's `allowed_dependencies` conflating prod and dev-only edges (masking the exact prod edge AW.3 was meant to keep closed); an untyped `loopback_tcp_get_json` GET-any-path escape hatch plus a non-`cfg(test)` internal constant re-exported as API plus a trait method (`prune`) with no production caller; `/v1/health`/`/v1/diagnostics` bypassing the canonical router's concurrency/load-shed/timeout admission layers (still capability-authenticated, so exposure not a hole); an unused `RuntimeHealth` field with a stale doc/plan contract claim; two thin one-line wrapper functions; a full `atm-observability`/`sc-observability`/`tracing-subscriber` edge pulled into the Python wheel to consume two constants; no ADR recording the new retained-diagnostics pipeline's components. | Accepted on citation — each is a narrow, low-ambiguity hygiene item; not independently re-derived. |

**Reviewer-supplied PREVENTION mapping (adopted directly into this post-mortem's classifications):** fenix's reviewer-C independently produced a finding-family-to-lint mapping that converges strongly with this document's own Families A, B, D, F, and the new M/N — most notably recommending the same `.just` line/file-size lint (Family F), the same boundary-owner rule-text update (Family D / O5), a `lint_boundaries.py` check that a `lint_rules` id must resolve to a registered rule (extends O16's minor), per-section (prod vs dev) manifest allowlists (O7-O16 bundle), a duplicate-struct scanner (Family D / O7), a lint denying `AtmError` string-sniffing outside tests (O3), `.gitattributes eol=lf` for fixture globs (Family C), a reachability lint ensuring every test file is wired into a run target (Family C/N), and a phase-gate doc/status-match check (Family A). This is treated as independent triangulation on this post-mortem's own Decision Rationale, not a separate source of new classifications.

**Root cause:** Same compound cause as Family D (missing boundary enforcement enforced at one point but not mirrored/updated at the rule-text level) for O5, and a fresh instance of the "test proves a weaker property than the AC" gap (Family E) for O4/O6 — no test asserts that a `WriterClosed` drop remains visible in `/v1/health`, nor that a second `attach_timeline` call is safe.

**Classifications:**
- `new_lint` — cross-crate `AtmError` string-sniffing lint (O3); duplicate-struct scanner (O7); `lint_rules` id must resolve to a registered table (part of O16)
- `boundary_update` — per-section (prod/dev) manifest allowlist comparison so `allowed_dependencies` cannot silently admit a dev-only edge into the prod-boundary record (O16 bundle item)
- `architecture_update` — new ADR naming `atm-observability` as sole `sc-observability` owner, superseding the stale ADR-020 module-level exception, and updating `RULE-001` text to match (O5); one ADR documenting the retained-diagnostics pipeline components (O16 bundle item)
- `test_hardening` — typed `DiagnosticRecordError` offer enum instead of untyped-error string matching (O3); a test asserting `WriterClosed` drops surface in `/v1/health` (O4); an idempotency guard + regression test for `attach_timeline` (O6)

**Target artifacts:**
- `crates/atm-storage/src/diagnostics.rs` — typed record-batch error enum
- `crates/atm-daemon-bootstrap/src/diagnostic_timeline.rs` — fix `refresh_persisted_stats` counter ownership; idempotency guard on `attach_timeline`
- `docs/adr/ADR-020*.md`, `.claude/agents/arch-qa.md` RULE-001 text — new/updated ADR naming `atm-observability` as sole owner
- `.just/lint_boundaries.py`, `.just/lint-config.toml` — per-section allowlist comparison; `lint_rules` existence check

---

## Explicit Assessment: AW.2 → AW.3 `must_follow` Sequencing Deviation

Per `docs/plans/phase-aw/phase-aw-plan.md` §4 (execution notes, recorded 2026-09-05):

> AW.3 was cut and reviewed before AW.2 merged, violating the declared `AW.2 → AW.3 must_follow` relation. Merge-forward from `integrate/phase-aw` after AW.2 (PR #1179, #1195) and AW.4 landed substituted for the ordering; AW.3 (PR #1182, #1196, #1198) was re-verified by quality-mgr on the merged base before its final merge.

**Assessment:** This was a real dependency-ordering violation, not a false alarm — AW.3 queries the timeline AW.2 persists, so cutting AW.3 before AW.2 merged means AW.3's dev agent worked against an unmerged interface. However, the substitute control (merge-forward + a full quality-mgr re-verification of AW.3 on the merged base before AW.3's own final merge) is exactly the standing `feedback_merge_forward_asap` discipline, and it demonstrably worked — no AW.3 finding in this ledger traces to an AW.2 interface mismatch that survived to phase-end. The dependency graph declared the risk correctly; the safety net caught the actual deviation.

**Classification:** `merge_forward_process_improvement` — not `no_systemic_followup`, because relying on "the safety net caught it this time" is not itself durable. Concretely: a dispatch-time check should compare the sprint's declared `dependency_relations` (`must_follow`) against the prerequisite PR's actual merge state before a sprint branch is cut, and refuse (or explicitly flag as an accepted risk) a cut that violates a declared `must_follow` edge, rather than discovering the violation after the fact in the plan's own execution notes.

**Target artifact:** `.claude/skills/phase-orchestration/SKILL.md` (or `codex-orchestration/SKILL.md`, whichever coordinates sprint-branch cutting) — add a pre-cut check: for each sprint about to be dispatched, verify every `must_follow` prerequisite named in the phase plan's `dependency_relations` is merged into the integration branch; if not, require an explicit team-lead override note (matching what phase-aw's plan already records after the fact) before dispatch, not only as a retrospective note.

---

## Explicit Assessment: CI-Red-Until-Late Pattern (AW.2 / AW.3)

Rand's own observation: **"I don't think I have ever seen AW.2/AW.3 CI green"** until late in each sprint's QA/fix cycle.

**Evidence from the ledger:** AW.2 has 4 CI findings (AW2-CI-001..004, all blocking) plus 3 more RULE-002/003 or storage-boundary blocking findings; AW.3 has 7 CI findings (AW3-CI-001..007, all blocking) plus 6 more QA-B blocking findings on its own functional deliverable (Family G) and file-size violations (Family F). Between the two sprints, **21 of 33 total blocking findings in the entire phase (64%)** landed on AW.2 or AW.3, and the large majority of those (Families B, C, F) are classes a local build/lint/test run would have caught before any reviewer or CI cycle was spent on them.

**Assessment:** Yes — the dev-QA loop should gate on `just validate` (or equivalent: `cargo build --workspace --tests` + `just lint` + a targeted flaky-test/serialization check) locally before a `qa-pr<N>-r1` dispatch is issued. This is not a new capability request; `rust-qa-agent`'s `phase_end` assignment already requires `artifact_commands: "just validate"` execution proof — the gap is that this proof is currently only collected as *evidence for the reviewer*, not enforced as a *pre-dispatch admission gate* that would stop a QA round from even starting against a broken build.

**Classification:** `qa_process_improvement` (primary) + `new_lint`/tooling (to make the pre-dispatch check itself automatic rather than a checklist item a dev agent could skip).

**Target artifacts:**
- Dev-agent dispatch template (wherever `sc-worktree-create`/dev-dispatch instructions live) — require a local `just validate` pass (or documented failure reason) attached to the first-push notification, before `qa-pr<N>-r1` is sent
- `.claude/agents/quality-mgr.md` — QA intake gate: a dispatch against a branch where `cargo build --workspace --tests` fails is rejected back to the dev agent, not spent on a reviewer round

---

## Integration Review Summary

| Condition | Result |
| --- | --- |
| All sprint branches merged into integrate/phase-aw | YES (AW.1–AW.5, PRs #1176, #1179/#1195, #1182/#1196/#1198, #1181, #1184) |
| Fix-round PR merged | YES (#1200 → integrate/phase-aw, merge commit f546c0c5a) |
| Final QA on integration branch | PASS at f546c0c5a (`qa-pr1199-r1` re-pin) |
| All 70 sprint-scoped triage records fixed | YES (70/70, independently enumerated at f546c0c5a) |
| Waived findings | NONE |
| Deferred findings | NONE |
| **No new blocking findings on a full phase-ending readiness review** | **NO — 4 new blocking (Family M) found by arch-ctm + fenix-a after ledger closure** |
| Zero-regression benchmark evidence | YES (sqlite +25.0%, uds +18.2%, tcp +7.3%, tcp-tls +19.0% vs floor; `durability_after_restart` parity all four targets) — unaffected by Families M/N/O, none of which touch the benchmarked hot paths |
| No legacy synchronous daemon files touched by any phase-aw fix | YES (confirmed by direct diff at every review round, including fenix-c's architecture review) |
| No boundary or Cargo-edge violation | YES (fenix-c: 93/93 boundary tests pass, legacy-daemon diff empty, no forbidden edge) |
| Merge authorization | **Blocked** — Family M's 4 blocking findings must be triaged and fixed (or the phase gate explicitly overridden by Rand) before PR #1199 → develop |

**Integration review verdict:** `integration_review_failed` — the sprint-scoped ledger's own gate passed cleanly, but the required phase-ending readiness review (this post-mortem's own trigger) found new blocking findings not covered by any sprint's acceptance criteria. Per the post-mortem's Required Integration Review conditions, "100% of triaged findings fixed or intentionally deferred" cannot be satisfied while Family M's findings remain untriaged.
**Evidence SHA:** `f546c0c5aadf9f68b6cc1cd2c57bc4d761afcddc`

---

## Systemic Recommendations Summary

| # | Finding family | Classification | Owner | Target artifact |
| --- | --- | --- | --- | --- |
| 1 | Doc/status staleness (A) | `new_lint` | arch-ctm | `.just/run_lint.py` — sprint-doc/plan-table parity check |
| 2 | Doc/status staleness (A) | `qa_process_improvement` | quality-mgr | `.claude/skills/quality-management-gh/SKILL.md` |
| 3 | CI-discovered build/lint breakage (B) | `qa_process_improvement` | quality-mgr | QA intake gate — reject dispatch on broken build/lint |
| 4 | CI-discovered build/lint breakage (B) | `merge_forward_process_improvement` | arch-ctm | Re-run gates after every merge-forward, not only at sprint tip |
| 5 | Test flakiness / platform nondeterminism (C) | `new_lint` + `test_hardening` | arch-ctm | Env-mutation-in-test lint; shared LF-normalize helper |
| 6 | Boundary mirror/duplicate-ownership drift (D) | `new_lint` | arch-ctm | Cross-boundary response-shape duplication detector |
| 7 | Boundary mirror/duplicate-ownership drift (D) | `architecture_update` | arch-ctm | New ADR — one classifier, one error-code variant |
| 8 | Missing test coverage vs AC (E) | `sprint_plan_update` | team-lead | Sprint doc template — `proof_test:` per AC |
| 9 | Missing test coverage vs AC (E) | `qa_process_improvement` | quality-mgr | QA-1 AC-to-test traceability check |
| 10 | Function-length/file-size lint (F) | `new_lint` (escalated repeat of phase-Yb #3) | arch-ctm | Verify RULE-002/003 is a required CI check, not QA-only |
| 11 | AW.3 functional gaps vs AC (G) | `sprint_plan_update` + `qa_process_improvement` | team-lead / quality-mgr | Concrete example queries in AC; call-path tracing in req-qa |
| 12 | Storage single-writer-lane violations (H) | `new_lint` | arch-ctm | Extend writer-lane gate to mutation call sites |
| 13 | Coincidental-parity claims (I) | `qa_process_improvement` + `architecture_update` | quality-mgr / arch-ctm | Verify parity mechanism, not output snapshot; single JSON encoder path |
| 14 | Harness fixture staleness (J) | `test_hardening` | arch-ctm | Staleness assertion in benchmark harness |
| 15 | RBP defensive-coding gaps (L) | `no_systemic_followup` (cadence already decided) + `test_hardening` (sweep for repeatable ones) | quality-mgr | Standalone hardening ticket |
| 16 | AW.2→AW.3 must_follow deviation | `merge_forward_process_improvement` | team-lead | Pre-cut dependency check in orchestration skill |
| 17 | CI-red-until-late (AW.2/AW.3) | `qa_process_improvement` | quality-mgr | Pre-dispatch `just validate` admission gate |
| 18 | Retained-data hygiene bypass (M1, M2) | `boundary_update` + `test_hardening` | arch-ctm | Single mandatory redaction boundary for every retained-sink write path; adversarial secret-in-message test |
| 19 | Quiet-daemon prune never fires / newest-row-deletion bug (M3, M4) | `test_hardening` | arch-ctm | Independent time-based prune trigger; fix DELETE ordering; test asserting surviving-row identity |
| 20 | Retained-log contract scope ambiguity (M1-M4) | `architecture_update` + `qa_process_improvement` | quality-mgr | Amend `docs/atm-daemon/logging.md` to name all retained-sink boundaries; standing phase-end cross-boundary contract trace |
| 21 | Level-filter lexical compare / cap mismatch (M6, M7 = O1, O2) | `new_lint` | arch-ctm | Integer-rank level comparison; shared bound constant across route/storage |
| 22 | Wall-clock-polling test determinism (N1-N4, N6-N8) | `new_lint` + `test_hardening` | arch-ctm | Lint for `Instant`/`Duration` deadline loops and shared temp paths in tests; signal-based test synchronization |
| 23 | Parity-fixture host-isolation gap (N5) | `architecture_update` + `qa_process_improvement` | team-lead | Scoped test-only runtime-root override; convert the existing closure-note mention into a tracked, owned follow-up |
| 24 | Untyped cross-boundary error-string sniffing (O3) | `new_lint` | arch-ctm | Lint denying `AtmError` string-matching outside tests; typed `DiagnosticRecordError` |
| 25 | Counter-ownership overwrite bug (O4) | `test_hardening` | arch-ctm | Fix `refresh_persisted_stats`; test asserting `WriterClosed` visibility in `/v1/health` |
| 26 | Stale ADR/rule text after import relocation (O5) | `architecture_update` | arch-ctm | New/updated ADR naming `atm-observability` as sole `sc-observability` owner; update RULE-001 text |
| 27 | Non-idempotent `attach_timeline` (O6) | `test_hardening` | arch-ctm | Idempotency guard + regression test |
| 28 | Duplicate shapes / dev-prod allowlist conflation / dead surface (O7-O16) | `new_lint` + `boundary_update` | arch-ctm | Duplicate-struct scanner; per-section manifest allowlist comparison; `lint_rules` existence check |

---

## Decision Rationale

Preference order (smallest upstream control):

1. **New lint / static enforcement** (items 1, 3(partially via gate)/10, 5, 6, 12) — mechanically blocks the defect class before QA ever spends a reviewer round on it
2. **Boundary/architecture enforcement** (items 7, 13's architecture half) — structural ownership and single-path rules
3. **Process improvement** (items 2, 3, 4, 8, 9, 11, 13's QA half, 16, 17) — dispatch-gate, checklist, and template changes where full automation isn't yet built
4. **Manual/deferred hardening** (items 14, 15) — narrow, low-frequency, or explicitly already covered by an existing cadence decision

The two largest families by count in the sprint-scoped ledger — missing test coverage vs AC (16 findings, Family E) and boundary mirror/ownership drift (12 findings, Family D) — both already have a concrete, owned target artifact rather than "more QA vigilance." The CI-red-until-late pattern (items 3/17) is the single highest-leverage fix among the sprint-scoped families: it does not eliminate any finding family on its own, but it would have caught 18 of 70 findings (Families B, C, F — 26%) before a reviewer agent or CI minute was ever spent on them, purely by requiring what `rust-qa-agent`'s own `phase_end` template already asks for (`just validate` proof) to gate dispatch, not just to document evidence after the fact.

**Families M, N, and O change this phase's overall verdict, not just its recommendation list.** They are not evidence of sloppier sprint QA — the sprint-scoped reviewers (req-qa, arch-qa, rust-qa-agent, ruthless-boundary-qa) each correctly verified the acceptance criteria and boundary rules they were scoped to check, and fenix-c's own architecture pass confirms zero boundary or legacy-daemon violations. They are evidence of a **structural gap in the QA cadence itself**: nothing in this phase's dev-QA loop was scoped to ask the cross-cutting questions a phase-ending readiness review asks — "does the retained-data contract hold at every sink boundary, not just the one this diff touched?", "is this test asserting the AC, or a weaker proxy for it?", "does rule/ADR text still name the real owner after a fix moved the code?". Per the Decision Rule's own preference order, the fact that three independent reviewers, working from three different angles (data-hygiene, flaky-test audit, architecture/boundaries), each needed a dedicated full-phase pass to surface these — and that fenix-c's own PREVENTION list converges almost line-for-line with Families A/B/D/F's existing recommendations — means **manual reviewer vigilance is currently the only mechanism catching this class**, and per the decision rule that must be flagged as a process gap, not accepted as durable. The concrete correction is item 20/23 above (name every retained-sink boundary in the documented contract, and stop letting a closure-note mention substitute for a tracked, owned follow-up) plus making a full readiness-review-style pass (not just sprint-scoped QA) a standing phase-end gate rather than an ad hoc dispatch that happened to be requested this phase.
