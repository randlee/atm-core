# AL.15 — M5/cwin Smoke Closeout

**branch:** `feature/al-15-smoke`
**worktree:** `../atm-core-worktrees/feature/al-15-smoke`
**base:** current `origin/integrate/phase-al`
**owner:** coordinator
**must_follow:** AL.13 and AL.14 report PRs

## Goal

Create or refresh `feature/al-15-smoke` from current
`origin/integrate/phase-al` with `/sc-git-worktree`. Review the M5 and cwin
`/smoke-test` reports as one direct cross-host result. AL.15 does not add a
new smoke runner or rerun a host lane.

## Required review

Confirm that both host PRs contain reports for:

| Row | M5 | cwin |
|---|---|---|
| Runtime health | required | required |
| Localhost | required | required |
| Local IP | required | required |
| Peer readiness | required | required |
| Direct send, both directions | required | required |
| Requires-ack/reply, both directions | required | required |
| Benchmark | required | required |

Use the master `site/reports/index.html` navigation to inspect each retained
smoke and benchmark report. The two hosts must identify the same tested runtime
version before a combined PASS is possible.

## Report and PR

Open a PR from `feature/al-15-smoke` to `integrate/phase-al` with one closeout
report that links the M5 and cwin run reports and records one result:

- **PASS:** all required rows pass on both origins.
- **BLOCKED/FAIL:** name the first failing host and row, link its report, and
  identify the next owner.

## Acceptance

- The closeout PR links report artifacts from both host lanes through the
  master reports index.
- It reports a complete two-host PASS or a specific BLOCKED/FAIL result.
