---
status: blocked
blocker: The Windows-to-M4 direct cross-host send/ack lane was unavailable on the current VPN/DNS path.
---

# AL.15 — M5/cwin Smoke Closeout

**branch:** `feature/al-15-smoke`
**worktree:** `../atm-core-worktrees/feature/al-15-smoke`
**base:** current `origin/integrate/phase-al`
**owner:** coordinator
**must_follow:** AL.13 and AL.14 report PRs

## Current closeout (2026-08-10)

AL.15 is blocked only on the Windows↔M4 direct transport proof:

| Lane | Status | Evidence/disposition |
|---|---|---|
| M5↔M4 peer preflight, delivery, acknowledgement, and benchmark | complete | AL.13 retained and indexed the live M5↔M4 evidence. |
| Windows local smoke and benchmark | complete | AL.14 retained and indexed the Windows-local smoke and benchmark evidence. |
| Windows↔M4 direct send and acknowledgement | blocked | The available VPN/DNS path did not resolve or route to M4; no delivery or acknowledgement proof exists. |

This is not an abandoned-sprint designation. A combined two-origin PASS
remains unavailable until that one network path is available and the direct
public-CLI rows are rerun.

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
