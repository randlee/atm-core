---
title: Smoke 1.3.1 — Cross-host release-candidate plan
status: complete
branch: smoke-test/1.3.1-cross-host
worktree: /Users/randlee/Documents/github/atm-core-worktrees/smoke-test/1.3.1-cross-host
---

# Smoke 1.3.1 — Cross-host release-candidate plan

## Scope

This branch validates the 1.3.1 release candidate across the currently
available same-host surfaces on macOS and Windows.

- macOS lane: executed directly in this worktree by `arch-ctm`
- Windows lane: executed by the separate Windows agent and published back to
  this branch
- cross-host behavior: intentionally deferred; this document makes no
  cross-machine pass claim

## Candidate under test

- branch: `smoke-test/1.3.1-cross-host`
- workspace version: `1.3.1`
- runtime-hardening baseline: `8a371683` (`fix: harden graft smoke runtime readiness`)
- current validated smoke head: `b610431f`

`8a371683` is the commit that introduced the graft receiver readiness hardening
that unblocked the same-host graft lane. The current head `b610431f` closes the
QA follow-up findings against that baseline by tightening runtime behavior,
correcting smoke-script validation, and updating the evidence set.

## Authoritative references

- `docs/plans/phase-af/readiness.md`
- `scripts/smoke/run.py`
- `scripts/smoke/run_thorough_shared_host.py`
- `scripts/smoke/run_graft_same_host.py`
- `reports/smoke/smoke-fast.md`
- `reports/smoke/smoke.md`
- `reports/smoke/smoke-thorough.md`
- `reports/smoke/2026-07-15-00-13-36-shared-host-direct.md`
- `reports/smoke/2026-07-15-00-13-37-graft-same-host-direct.md`
- `reports/smoke/2026-07-14-23-44-37-smoke-1.3.1-windows-rerun.md`

## QA1 fix-round content

This fix round closed the local findings from `SMOKE-1.3.1-QA-1`:

- updated this sprint doc so it matches the post-fix branch state
- produced durable documentation for a direct passing
  `run_thorough_shared_host.py` execution
- rewrote `ReceiverReadyLatch` timeout/recovery text for production operators
- changed the graft receiver loop to isolate per-connection failures instead of
  terminating the listener on any single bad connection
- ensured the session snapshot transitions from `Listening` to `Closed` on
  clean shutdown and to `Degraded` when the receiver thread terminates
  unexpectedly
- aligned the runtime test helper with the widened receiver-ready deadline
- scoped the graft smoke daemon-ownership assertion to the current fixture
  session
- consolidated duplicated daemon lifecycle helpers into
  `scripts/smoke/daemon_lifecycle.py`

`ARCH-001` was not fixed by code on this branch. It was superseded by a fresh
Windows rerun already published to the branch with a real commit SHA in
`reports/smoke/2026-07-14-23-44-37-smoke-1.3.1-windows-rerun.md`.

## macOS lane

### Commands

1. `python3 scripts/smoke/run.py fast --write-artifacts`
2. `python3 scripts/smoke/run.py normal --write-artifacts`
3. `python3 scripts/smoke/run.py thorough --write-artifacts`
4. `python3 scripts/smoke/run_thorough_shared_host.py`
5. `python3 scripts/smoke/run_graft_same_host.py`
6. `just test`

### Result

All required macOS same-host checks passed at branch head `b610431f`.

| Check | Result | Evidence |
| --- | --- | --- |
| Fast smoke | `PASS` | `reports/smoke/smoke-fast.md` |
| Normal smoke | `PASS` | `reports/smoke/smoke.md` |
| Thorough smoke | `PASS` | `reports/smoke/smoke-thorough.md` |
| Direct shared-host lane | `PASS` | `reports/smoke/2026-07-15-00-13-36-shared-host-direct.md` |
| Direct graft same-host lane | `PASS` | `reports/smoke/2026-07-15-00-13-37-graft-same-host-direct.md` |
| Full repo validation | `PASS` | local `just test` run at `b610431f` |

### Direct shared-host rerun evidence

The direct rerun that QA requested completed successfully and is documented
separately instead of being inferred from the embedded AD18 row:

- command: `python3 scripts/smoke/run_thorough_shared_host.py`
- result: `PASS`
- durable report:
  `reports/smoke/2026-07-15-00-13-36-shared-host-direct.md`

This closes the earlier evidentiary gap where only the embedded suite row had
been published.

### Graft lane interpretation

The graft readiness fix landed in `8a371683`. This QA1 round keeps that
baseline and closes the follow-up issues around operator-facing errors,
receiver-loop resilience, readiness-deadline parity, and smoke validation
discipline. The branch-head thorough smoke remains green, including `GRAFT-001`
in `reports/smoke/smoke-thorough.md`.

## Windows lane

Windows reran the same-host smoke scope after the graft runtime fix and
published new evidence back to this branch.

| Check | Result | Evidence |
| --- | --- | --- |
| Windows rerun | `PASS` | `reports/smoke/2026-07-14-23-44-37-smoke-1.3.1-windows-rerun.md` |

That rerun uses a real branch SHA:

- binary SHA: `8a371683c21a2106b84ba77484d6a14882f652a2`

The Windows document explicitly states that cross-host behavior was not
exercised.

## Deliverables produced

- authoritative sprint document:
  `docs/plans/phase-af/smoke-1.3.1-cross-host-plan.md`
- macOS smoke reports:
  - `reports/smoke/smoke-fast.md`
  - `reports/smoke/smoke.md`
  - `reports/smoke/smoke-thorough.md`
- direct macOS lane reports:
  - `reports/smoke/2026-07-15-00-13-36-shared-host-direct.md`
  - `reports/smoke/2026-07-15-00-13-37-graft-same-host-direct.md`
- Windows rerun report:
  - `reports/smoke/2026-07-14-23-44-37-smoke-1.3.1-windows-rerun.md`

## Closeout state

This sprint is complete for the currently scoped same-host smoke surfaces.

- macOS same-host smoke: green
- Windows same-host smoke: green
- direct shared-host rerun evidence: present
- direct graft same-host evidence: present
- cross-host behavior: not yet in scope for this release document
