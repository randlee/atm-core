# Tooling fix: QA PASS requires TTL closure, orchestration state queried by template tags

Branch: `fix/qa-pass-requires-ttl-closure` (from develop 0a1749946). PR target: `develop`.
Owner: arch-ctm. QA: quality-mgr. Coordinator: fenix.

## Problem

`/triage-report` reported AY.3 as live 2/1/4 (B/I/M) while every one of those
findings had been verified fixed in QA rounds R3/R4. Root cause: nothing in
graph-orchestration ties a QA PASS verdict to closure of the sprint's
`.triage/<phase>/findings/*.ttl` records. A PASS is recorded by hand in
`docs/plans/phase-<p>/.audit/qa-evidence-master.json` while the Turtle records
stay `open`, so the report (which correctly reads only unresolved Turtle) shows
blockers that no longer exist.

Second defect: dispatch and verdict state is not queryable. Dispatches carry
`template_sha` but no workflow metadata (the orchestration templates declare no
`metadata.workflow`/`metadata.tags`), and QA verdicts are plain `atm send`
text with no template at all. ADR-046 already provides template-declared
workflow metadata, immutable admission snapshots, and `atm search`
filters (`--workflow-scope-kind/-id`, `--workflow-state`, `--workflow-stage`,
`--effective-tag`, `--template-meta`). Nothing uses them.

Rand's direction: "the queries for /triage-report should be easier, since we
should be able to query based on template tags."

## Deliverables

D1. Workflow metadata on the three orchestration templates, per ADR-046 section 1:
    `.claude/skills/graph-orchestration/dev-task.xml.j2`,
    `.claude/skills/graph-orchestration/dev-fix.xml.j2`,
    `.claude/skills/codex-orchestration/qa-template.xml.j2`.
    Each declares `metadata.type`, `metadata.tags`, and a complete
    `metadata.workflow` (scope kind `sprint`, scope variable = the node id
    variable, state/stage/transition, iteration variable where a round exists).
    One node's dev, fix, qa, and verdict messages must share the same
    `workflow_scope_id`. Bump each template's semver (minor). Templates remain
    verbatim instructions only; no rationale in step text.

D2. New `.claude/skills/codex-orchestration/qa-verdict.xml.j2` that quality-mgr
    sends for every verdict (`workflow.state` distinguishes pass from fail via
    the transition or a required `verdict` variable; stage `qa`). Required vars:
    task_id, sprint, node_id, pr_number, commit_reviewed, round, verdict
    (PASS|FAIL), blocking, important, minor, finding_ids (comma list or empty),
    ttl_closure_commit (sha of the commit that closed/created the records, or
    `none`), report_path. Update `.claude/agents/quality-mgr.md` and
    `.claude/skills/quality-management-gh/SKILL.md` so verdicts are sent only
    through this template (`atm send <coordinator> --template ... --vars ...`);
    a free-text verdict is not a verdict.

D3. Query script `.claude/skills/graph-orchestration/scripts/orchestration_state.py`:
    given `--phase` and optional `--node`, calls `atm search --json` with the
    ADR-046 workflow filters and returns per node the latest dispatch
    (dev/fix/qa) and latest verdict (message id, time, template sha, verdict,
    pr, commit, round). No direct SQLite access; the CLI is the interface.
    `triage_report.py` reads its QA provenance column from this script and
    labels the source; it falls back to `qa-evidence-master.json` only when the
    mail store has no verdict for that node, and says so in `diagnostics`.

D4. Closure gate `.claude/skills/graph-orchestration/scripts/check_qa_closure.py --phase <p> --node <id>`:
    exit 0 only when (a) the latest verdict for the node is PASS (D3),
    (b) `triage_report.py --format json` shows live B/I/M all zero and no
    stale occurrence for that node, and (c) every finding record whose
    `triage:foundIn` is the node has finding-level status `closed` or `fixed`.
    Otherwise exit 1 with a structured JSON listing of each unmet condition and
    the record paths. Wire it in:
    - qa-template.xml.j2: quality-mgr runs the gate before sending PASS; a PASS
      with open records means quality-mgr closes the records it verified (its
      existing closure authority), commits them on the integration branch, and
      cites that commit in `ttl_closure_commit`; if it cannot close them it sends
      FAIL with the reason. A PASS that fails the gate is invalid.
    - graph-orchestration SKILL.md: the coordinator runs the gate before
      recording a PASS, advancing the cursor, or merging a sprint PR.
    - dev-task/dev-fix completion steps stay unchanged.

D5. Tests under `.claude/skills/graph-orchestration/tests/` (pytest, no live
    mail store, no live GitHub): orchestration_state.py against canned
    `atm search --json` output; check_qa_closure.py against fixture Turtle
    (open, closed, fixed_partial with stale occurrence) and canned report JSON;
    `sc-compose render --strict` of every touched template with sample vars.
    The PR description includes `atm send --dry-run` output for each template
    (no real sends) and one real `atm search` invocation showing the filters
    resolve on atm 1.5.5.

## Acceptance criteria

AC1. `python3 .claude/skills/graph-orchestration/scripts/check_qa_closure.py --phase ay --node AY3`
     run against a copy of integrate/phase-ay's current `.triage` exits 1 and
     lists AY3-QA-002 and the AY2-QA-001 stale occurrence by path.
AC2. Same command against fixture data with all records closed and a PASS
     verdict exits 0.
AC3. `atm send --dry-run --template <each template> --vars <sample>` succeeds
     for dev-task, dev-fix, qa-template, qa-verdict with the daemon at 1.5.5.
AC4. `atm search --json --workflow-scope-kind sprint --workflow-scope-id <node> --workflow-stage qa`
     returns the qa-verdict dry-run/test messages once the templates carry
     metadata (document the exact command in the skill README).
AC5. `triage_report.py --phase ay --format json` still exits 0 on
     integrate/phase-ay data and its QA column names its source (mail store or
     evidence master) per row.
AC6. `cargo` untouched: no Rust changes, no daemon changes, no `.triage/` or
     `qa-evidence-master.json` edits on any branch.
AC7. pytest for the new tests passes; `sc-compose render --strict` passes for
     every template; `python3 .just/run_lint.py` targets that cover skills
     (if any) pass.

## Non-goals

- No Rust or storage changes; ADR-046 machinery is used as shipped.
- No backfill of historical messages; old plain-text verdicts stay untagged
  (the evidence-master fallback covers them).
- No change to triage-record.ttl.j2 or the qa-triage agent.

## Required validation

```
python3 -m pytest .claude/skills/graph-orchestration/tests -q
sc-compose render --strict --root . --file <template> --var-file <sample>   # each template
atm send --dry-run --template <template> --vars <sample> <recipient>           # each template
python3 .claude/skills/triage-report/scripts/triage_report.py --phase ay --format json --integration-root ../integrate/phase-ay
```
