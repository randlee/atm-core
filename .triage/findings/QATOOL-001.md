# QATOOL-001: Schema validator not invoked at write/report/query time

## Pattern
```
oxigraph convert
validate-findings.py
validate-findings.sparql
def check_sc_compose_binding
_phase_sprint
```

## Crates Affected
N/A — this is a `.claude/skills` tooling finding, not a crate/Rust defect. Cross-cutting across all phases (graph-orchestration, triaging-findings, triage-report skills).

## Sprint Origin
PR #639 QA-1 (`fix/qa-triage-foundin-foundat-schema`, reviewed HEAD `c26d44db`), not tied to any phase/sprint. Filed here (legacy cross-cutting tier) rather than under `.triage/phase-*/findings/` because this finding is about the triage tooling itself, not a phase deliverable.

## Status
closed (PR #639 QA-2, commit 572f18a9)

## Description
PR #639 adds `validate-findings.py` / `validate-findings.sparql`, a schema validator that correctly enforces `triage:foundIn`/`triage:foundAt` as required and rejects non-repo-relative paths — confirmed working against both real data and a synthetic bad-record test (missing foundIn/foundAt, absolute host path both correctly flagged as `#error`).

However, the validator is not actually wired into any of the three places it needs to run to be load-bearing:

1. **Write time** — `.claude/agents/qa-triage.md`'s commit step (step 12) only runs `oxigraph convert` (proves the Turtle is parseable, not schema-complete). A finding missing `foundIn`/`foundAt` can be committed today with nothing in the write path catching it.
2. **Report time** — `triage_report.py` does not invoke the validator before generating its DEV/QA/CI/PR/B/I/M table; it silently omits/misattributes findings that fail schema (e.g. missing `foundIn`) rather than erroring.
3. **Query time** — `query_runner.py`-based queries, including `next-dev-task`'s cursor resolution, do not invoke the validator either; the same silent-omission risk applies to dispatch-cursor decisions.

Net effect: a malformed finding can silently fail to be attributed to its sprint by the very tools (`triage_report.py`, `next-dev-task`) team-lead/quality-mgr rely on to know exactly where blockers are and where a dev agent should not be dispatched. A sprint can read as clear while an unattributed blocker sits on it, uncounted.

Also related, lower-severity issues found in the same PR (tracked here for one-stop reference, not separately filed):
- `triaging-findings/SKILL.md` has a duplicated, self-contradictory "3.1 Commit triage artifacts before dispatch" section.
- `sc-compose>=1.2.0` version-gate logic (and its install-message string) is independently duplicated across `preflight.py` and `check_dependencies.py`, with `triage-report/SKILL.md` documenting an unenforced `<1.3` upper bound.
- `triage-record.ttl.j2`'s path-escape guard covers occurrence file paths but not `WorktreeSnapshot`'s own `triage:path`.
- `triage_report.py`'s `_phase_sprint()` hardcodes an AI-specific sprint-naming regex and silently degrades (returns `—`) rather than erroring for a future non-AI phase naming convention.

## Occurrences
| Location | File | Line | Issue | Fixed |
|----------|------|------|-------|-------|
| write path | `.claude/agents/qa-triage.md` | ~170-173 | only oxigraph round-trip, no schema validation | fixed (572f18a9) |
| report path | `.claude/skills/triage-report/scripts/triage_report.py` | n/a | validator never called before report generation | fixed (572f18a9) |
| query path | `.claude/skills/graph-orchestration/scripts/query_runner.py` / `next-dev-task` | n/a | validator never called before cursor/query resolution | fixed (572f18a9) |
| doc contradiction | `.claude/skills/triaging-findings/SKILL.md` | 226-254, 256-279 | duplicate, contradictory section | fixed (572f18a9) |
| version-gate duplication | `.claude/skills/graph-orchestration/scripts/preflight.py`, `.claude/skills/triaging-findings/scripts/check_dependencies.py` | n/a | triplicated sc-compose>=1.2.0 logic | fixed (572f18a9) |
| path guard gap | `.claude/skills/triaging-findings/triage-record.ttl.j2` | ~62, 73-77 | no guard on WorktreeSnapshot path | fixed (572f18a9) |
| phase hardcoding | `.claude/skills/triage-report/scripts/triage_report.py` | 252-256 | `_phase_sprint()` hardcoded AI regex, silent degrade | fixed (572f18a9) |

## Fix
Wire `validate-findings.py`/`.sparql` into all three call sites (qa-triage.md write step, triage_report.py, query_runner.py/next-dev-task) so each fails loudly on a schema violation instead of silently treating an invalid finding as absent. Consolidate the sc-compose version-gate into one shared helper. Deduplicate the SKILL.md section. Add the missing WorktreeSnapshot path guard. Generalize or error-on-mismatch in `_phase_sprint()`.

## Fix History
- 2026-07-25: Filed during PR #639 QA-1 (quality-mgr). Recommended fixes sent directly to Crimson-2f4c (PR author) via ATM; Crimson to push final commit and request QA re-run (QA-2) from team-lead.
- 2026-07-25: Fixed by Crimson-2f4c at commit `572f18a9`. All 7 occurrences closed. Verified independently by quality-mgr via direct code read, contract-test inspection, and live re-execution (synthetic bad-record re-test, `_phase_sprint()` called directly against fresh non-AI synthetic prefixes, PR/CI state check) — not a self-report pass-through.

## QA Round History
- PR#639-QA1: FAIL (req-qa 60% deliverable completion; ruthless-boundary-qa 2 critical/3 important/1 minor on its own scale; arch-qa PASS-with-findings 3 important/1 minor; ground-truth execution agent confirmed validator itself works correctly on real+synthetic input, confirmed no version-comparison bug, confirmed 46/46 relevant tests pass).
- PR#639-QA2 (commit 572f18a9): PASS (req-qa 100% deliverable completion, 0 findings; arch-qa 0 blocking/0 important, merge_ready; ruthless-boundary-qa could not be re-dispatched — session hit 200/200 subagent spawn cap — quality-mgr substituted direct ground-truth re-execution covering the same synthetic-adversarial-input checks). RBQA-F005 (duplicated `known_sprints` logic, query_runner.py vs validate-findings.py) confirmed still open — was never in this PR's committed scope, recommend as a separate future finding, non-blocking.
