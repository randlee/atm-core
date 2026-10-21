# Sprint bl-4 — QA reviewer data: `metadata.reviewers` on the QA bead and on finding beads

closure_type: boundary; target_boundary: QaReviewers; branch: sprint/bl-4-qa-reviewer-data; PR target: integrate/phase-bl; wave: 2; external_ref: #347; recommended_agent: arch-ctm; recommended_model: sonnet; must_follow: bl-1 (versioned reviewer metadata fixture); parallel_safe: bl-2, bl-3, bl-5, bl-6, bl-7, bl-8, bl-10, bl-13, bl-14.

Paths under `plugins/atm-bd-orchestration/skills/atm-bd-orchestration/`.
Line numbers at v0.18.4 (86853d5).

## Governing text (verbatim, Rand on #347)

> "on qa page, we should probably create 2 horizontal panels (top/bottom).  top would be ~60% and would have most recent qa run on top.  when selecting a row of a qa run, the lower pane would have a vertical split each qa subagent w/ status (pass/fail Nerd icon), elapsed time of subagent, if subagent was selected,  finding text for all findings would show up in lower right panel which rendered it nicely."

> "actually, I think quality-mgr files the findings after triage as beads."

> "so multiple sub-agents may both contribute the a single finding.  i.e. there may be 3 subagent fail for 2 findings."

Narrowed scope (2026-10-21 review of #347): the Sanity tab is delivered; the
remainder is the Quality tab. This sprint is the producer half; bl-7 is the
console half and consumes the shapes below.

## Relations

bl-1 supplies the fixture/codec boundary; every other wave-2 sprint is
parallel-safe. bl-7 is the sole Quality presentation consumer of
`metadata.reviewers` and names that artifact in its relation record.

## Scope boundary

In: quality-mgr times each reviewer spawn and writes `metadata.reviewers[]`
on the QA bead at close; finding beads gain `metadata.reviewers` (filing
reviewer first) at triage merge. Out: console (bl-7); the qa-log row change
(dropped by the review: the console derives elapsed from bead timestamps).
No new plugin file.

## Owned paths

- `roles/quality-mgr.md`
- `templates/qa-template.xml.j2`, `templates/finding-bead.json.j2`
- `examples/qa-template-vars.json`, `examples/qa-template-fix-round-vars.json`,
  `examples/finding-bead-vars.json`
- `scripts/tests/test_qa_report_contract.py` (all new tests go here)

`test_templates.py` and `test_handoff_contracts.py` are bl-5's. Changes here
are additive (a new metadata key, a new instruction sentence), so their
existing assertions must stay green; the full plugin suite is the check. If
an assertion in those files must change, this sprint stacks after bl-5 and
the plan's wave table is updated.

## Public contract

QA bead, written by quality-mgr at close:

```json
"metadata": {"reviewers": [
  {"reviewer": "req-qa", "status": "PASS|FAIL", "kind": "verdict|API_FAILURE",
   "elapsed_s": 412.3, "error": null | {"code": "...", "message": "..."},
   "findings": ["<finding bead id>", ...]}]}
```

Finding bead (`finding-bead.json.j2`; `reviewer` at `:16,60` stays the filing
reviewer, which alone verifies the fix):

```json
"metadata": {"reviewer": "req-qa", "reviewers": ["req-qa", "arch-qa"]}
```

## Authoritative deliverables

1. `qa-template.xml.j2` (steps b and d, where reviewers are spawned and
   collected) and `roles/quality-mgr.md`: record a start and end time around
   each reviewer spawn and keep `elapsed_s`, `status`, `kind` and the error
   per reviewer.
2. Step g (triage, where finding beads are filed): when duplicates merge,
   the finding bead's `metadata.reviewers` lists every contributing reviewer,
   filing reviewer first; `finding-bead.json.j2` takes a `reviewers` var.
3. Step i (close): `bd update <qa bead> --metadata` writes
   `metadata.reviewers[]` with each reviewer's finding bead ids; a reviewer
   whose findings did not survive triage has `findings: []`.

## Authoritative acceptance criteria (gh-347, deterministic)

- [ ] `test_finding_bead_reviewers_filing_first`: rendering `finding-bead.json.j2` with `reviewer: "req-qa"`, `reviewers: ["req-qa","arch-qa"]` gives that list and the unchanged `reviewer`.
- [ ] `test_qa_close_step_writes_reviewers`: rendered step i contains the `metadata.reviewers` write with the fields `reviewer, status, kind, elapsed_s, error, findings`.
- [ ] `test_reviewer_timer_in_spawn_steps`: rendered steps b/d instruct recording start/end per reviewer spawn.
- [ ] `test_three_reviewers_two_findings_shape`: an example vars file with three failing reviewers over two finding ids renders three `reviewers` entries pointing at two ids.

## Authoritative validation

```bash
cd plugins/atm-bd-orchestration && python3 tests/gen_manifest.py --check && python3 -m pytest -q -rs
```
