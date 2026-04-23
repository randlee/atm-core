---
name: quality-management-gh
version: 1.0.0
description: Reusable QA orchestration skill for GitHub PRs. Use for multi-pass QA, CI monitoring, and template-driven findings and final quality reports.
---

# Quality Management (GitHub)

This skill defines a reusable quality-management workflow for teams that run QA
across one or more passes before merge.

## Scope

Use this skill when you need to:
- run QA in multiple passes (`IN-FLIGHT`, `FAIL`, `PASS`)
- monitor CI progression for a PR
- publish structured findings to PR plus ATM
- publish a final QA closeout report on PASS

This skill is intentionally generic. Team-specific teammate names, branch
policy, and background-agent ownership stay in the repo’s `quality-mgr` agent
prompt.

## Required QA Status Contract

Every QA update, both ATM and PR, must include:
- sprint or task identifier
- branch, commit, PR number
- verdict (`PASS | FAIL | IN-FLIGHT`)
- finding counts by severity (`blocking`, `important`, `minor`)
- blocking ids with concise summaries
- next required action plus owner
- merge readiness (`ready | not ready`) plus reason

Use fenced JSON for machine-readable status payloads:

```json
{
  "sprint": "M.1",
  "task": "mailbox-locking",
  "branch": "feature/pM-s1-mailbox-locking",
  "commit": "abc1234",
  "pr": 123,
  "verdict": "FAIL",
  "findings": {
    "blocking": 1,
    "important": 2,
    "minor": 0
  },
  "blocking_ids": ["QA-001"],
  "next_action": "Fix lock acquisition rollback semantics",
  "owner": "arch-ctm",
  "merge_readiness": "not ready",
  "merge_reason": "Blocking findings remain"
}
```

## QA Lifecycle (Multi-Pass)

1. Initial pass: usually `FAIL` with findings.
2. Fix passes: `IN-FLIGHT` or `FAIL` while fixes are in progress.
3. Final pass: `PASS` with final quality report and merge recommendation.

Do not treat QA as single-shot.

## CI Monitoring

Preferred repo-specific flow:
- use `atm gh monitor status` to verify monitor health when available
- use `atm gh monitor pr <PR> --start-timeout 120` to start or attach a PR monitor when available
- use `atm gh pr report <PR> --json` for one-shot structured status when available

Fallback when repo-specific `atm gh` tooling is unavailable or not yet wired:
- `gh pr checks <PR> --watch`
- `gh pr view <PR> --json mergeStateStatus,reviewDecision`

If monitoring cannot start, include the failure in QA status and proceed with
one-shot PR report data.

## Findings Report to PR (Blocking)

Template:
- `.claude/skills/quality-management-gh/findings-report.md.j2`

Recommended flow:
1. Gather findings from QA agents.
2. Render markdown from the template with required variables.
3. Post to the PR as a blocking review or status comment.

Suggested commands:
- blocking review:
  `sc-compose render --root .claude/skills/quality-management-gh --file findings-report.md.j2 --var-file <vars.json> | gh pr review <PR> --request-changes --body-file -`
- in-flight update:
  `sc-compose render --root .claude/skills/quality-management-gh --file findings-report.md.j2 --var-file <vars.json> | gh pr comment <PR> --body-file -`

Fallback when render fails:
- post plain markdown preserving the same machine-status fields

`<vars.json>` must be a flat JSON map of strings for `sc-compose`.

## Final Quality Report to PR (Closeout)

Template:
- `.claude/skills/quality-management-gh/quality-report.md.j2`

Recommended flow:
1. Confirm final QA pass and summarize validation scope.
2. Render markdown from the template with required variables.
3. Post as final closeout review or comment.

Suggested command:
- `sc-compose render --root .claude/skills/quality-management-gh --file quality-report.md.j2 --var-file <vars.json> | gh pr review <PR> --approve --body-file -`

Use the final template only for `PASS` closeout.

## PR Update Conventions

- First QA pass posts detailed findings with `FAIL` and should use
  `--request-changes`.
- Fix-pass updates revise status and open findings.
- Final pass posts `PASS` closeout with residual risk and readiness and should
  use `--approve`.
- Rendered reports must include a fenced JSON block for machine parsing.

## ATM Coordination Protocol

For each task:
1. immediate acknowledgement
2. execute QA work
3. send completion or status summary
4. receiver acknowledgement

No silent processing.
