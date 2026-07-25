# Phase-AI audit investigation log

Concise chronology of the AICH-S1…AICH-S10 investigation. Detailed command
output and issue analysis are in [graph-orchestration-script-audit.md](graph-orchestration-script-audit.md).
Audit guidance and scope are in [README.md](README.md).

| UTC time | Task / finding | Result | Evidence / reference |
|---|---|---|---|
| 2026-07-25 04:15 | Established lower time bound | First AICH-S1 assignment in `.sprints/AICH/events.ttl`; use this as the default git/ATM query lower bound. | [README.md](README.md) — Time window |
| 2026-07-25 16:25 | Started audit session | Created the `audit/phase-ai` worktree from `integrate/phase-AI`; restricted scope to the supplied AICH sprint table. | [README.md](README.md) — Scope and baseline |
| 2026-07-25 16:30 | Compared graph-orchestration refs | Graph skill/scripts are absent on `integrate/phase-AI`; present on `develop`/`origin/develop`. `origin/develop` also has the ignore list. | [graph-orchestration-script-audit.md](graph-orchestration-script-audit.md) — Refs compared |
| 2026-07-25 16:32 | Ran graph cursor against AICH | Cursor returned `TRAVERSAL / AICH-S10`; 37 malformed legacy-file warnings. Ignore list suppresses noise without changing the cursor. | [graph-orchestration-script-audit.md](graph-orchestration-script-audit.md) — Graph-orchestration on AICH data |
| 2026-07-25 16:34 | Checked graph structure and completion queries | Structure query returned 0 violations; cursor selected S10; S8 and S10 lacked valid completion; open-findings query returned 0 because AICH findings do not use the expected fields. | [graph-orchestration-script-audit.md](graph-orchestration-script-audit.md) — Direct query results |
| 2026-07-25 16:36 | Ran triaging-findings carry-forward tests | Three tests passed on both `develop` and `integrate/phase-AI`; scoped carry-forward found one `AI22-BLOCK-004` occurrence on `integrate/phase-AI`. | [graph-orchestration-script-audit.md](graph-orchestration-script-audit.md) — Triaging-findings carry-forward |
| 2026-07-25 16:40 | Audited AICH finding metadata | 63 AI21–AI30 finding records contain no `triage:foundIn`; only AI21-BLOCK-001 has `triage:foundAt`. This makes graph invalidation and cleanup results incomplete. | [graph-orchestration-script-audit.md](graph-orchestration-script-audit.md) — Issues found, item 1 |
| 2026-07-25 16:48 | Added raw findings validator | Added `validate-findings.sparql` and `validate-findings.py` on the development line. `#error` covers missing `foundIn`/`foundAt`; `#warning` covers missing identity/routing/description fields. | Development-ref paths: `.claude/skills/graph-orchestration/scripts/validate-findings.py` and `SKILL.md`; detailed result in [graph-orchestration-script-audit.md](graph-orchestration-script-audit.md) |
| 2026-07-25 16:50 | Ran validator for AICH scope | 169 files scanned; 63 AI21–AI30 findings selected; 125 errors, 0 warnings. Output is intentionally truncatable with `--max-results`; the first 12 lines are summarized in the detailed report. | [graph-orchestration-script-audit.md](graph-orchestration-script-audit.md) — AICH findings validation |
| 2026-07-25 16:52 | Reconstructed provenance from git history | Commit subjects/bodies identify likely sprint and QA-pass origins and preserve branch/head snapshots. They do not establish the actual QA discovery timestamp or authoritative `foundIn`; those require the planned QA evidence. | [graph-orchestration-script-audit.md](graph-orchestration-script-audit.md) — Provenance investigation |
| 2026-07-25 16:56 | Recorded evidence gap and next input | QA evidence will be used to populate authoritative sprint assignment and observation timestamps rather than inferring them from current branch names. | User-provided QA-evidence plan; [README.md](README.md) — Recommended inputs |
| 2026-07-25 17:08 | Queried QA assignment/result mailboxes | Read-only ATM queries as `quality-mgr` (assignments) and `team-lead` (results), bounded at the AICH work-start time. Shared ATM paths were checked; local scratchpad copies were located for verdict artifacts. | [qa-evidence-master.json](qa-evidence-master.json) — message IDs, timestamps, and paths |
| 2026-07-25 17:12 | Built QA evidence projections | Created the authoritative JSON master index and PST CSV projection. The CSV has one row per located QA run with separate blocker/important/minor columns; unresolved assignment/result gaps remain explicit. | [qa-evidence-master.json](qa-evidence-master.json); [qa-assignment-results.csv](qa-assignment-results.csv) |
| 2026-07-25 17:13 | Parallel verification dispatched | Background agents are independently checking graph/triage scripts and validating the JSON/CSV projection while the high-level audit remains in this log. | Agent reports pending; script verification remains separate from evidence-table review |
| 2026-07-25 17:19 | Completed parallel script verification | Background verifier fixed `--validate-only` argument handling, hardened assignee/triage error paths, and added tests. Graph/triage suite: 43 passed; Python compile checks passed. Changes committed on `develop` as `621fd911`. | `.claude/skills/graph-orchestration/`; `scripts/triage_carry_forward.py`; verifier report |
| 2026-07-25 17:20 | Hardened query-runner CLI error boundary | Follow-up verifier change converts malformed graph/SPARQL exceptions to one-line `ERROR` output and adds regression tests. Full graph/triage suite reached 45 passed; Python compile checks passed. Follow-up committed on `develop` as `2ace0c66`. | `.claude/skills/graph-orchestration/scripts/query_runner.py`; `test_queries.py` |

## Investigation rule

Do not rewrite prior entries when new evidence changes an interpretation. Add a
new row with the evidence timestamp, explain the correction, and link the
source artifact. Branch names describe current location; commit messages and
QA artifacts are the provenance evidence for this audit.
