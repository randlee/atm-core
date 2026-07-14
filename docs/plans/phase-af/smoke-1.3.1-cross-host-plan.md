---
title: Smoke 1.3.1 — Cross-host release-candidate plan
status: complete
branch: smoke-test/1.3.1-cross-host
worktree: /Users/randlee/Documents/github/atm-core-worktrees/smoke-test/1.3.1-cross-host
---

# Smoke 1.3.1 — Cross-host release-candidate plan

## Scope split

This sprint has two explicit lanes.

- macOS lane: executed directly in this worktree by `arch-ctm` against
  `develop` at `98a4e66c`
- Windows lane: checklist only, to be executed separately by a Windows Codex
  agent coordinated directly by the user

No Windows pass/fail claim is made in this document.

## Candidate under test

- branch: `smoke-test/1.3.1-cross-host`
- target branch: `develop`
- candidate commit: `98a4e66c`
- workspace version: `1.3.1`
- publication state: not yet tagged or published

## Authoritative references

- `docs/plans/phase-af/readiness.md`
- `docs/plans/phase-af/af-1-host-singleton.md`
- `docs/plans/phase-af/af-2-observability-release-gates.md`
- `docs/plans/phase-af/af-3-native-send-input-integrity.md`
- `docs/plans/phase-AB/cross-host-smoke-checklist.md`
- `scripts/smoke/run.py`
- `scripts/smoke/run_thorough.py`
- `scripts/smoke/run_thorough_shared_host.py`
- `scripts/smoke/phase_ad_suite.py`

## Assignment mismatch noted at start

The dispatch instructed this task to treat
`docs/plans/phase-af/smoke-1.3.1-cross-host-plan.md` as the authoritative
source before coding, but that file did not exist in the assigned worktree at
task start. This sprint document was therefore bootstrapped from the dispatch
itself and becomes the authoritative source going forward.

## macOS lane

### Goal

Revalidate the accepted AF-1, AF-2, and AF-3 release-critical behavior on the
1.3.1 candidate in this macOS worktree, using the existing smoke scripts plus
the accepted-line evidence already recorded in `readiness.md`.

### Commands to execute

1. `python3 scripts/smoke/run.py fast --write-artifacts`
2. `python3 scripts/smoke/run.py normal --write-artifacts`
3. `python3 scripts/smoke/run.py thorough --write-artifacts`
4. `python3 scripts/smoke/run_thorough_shared_host.py`

### Pass criteria

- fast lane passes
- normal lane passes
- thorough lane passes
- shared-host lane passes on an isolated macOS OS-user with no pre-existing
  ambient `atm-daemon`

### Failure criteria

- any smoke row regression
- any shared-host singleton failure
- any retained AF-3 input integrity mismatch
- inability to execute the shared-host lane because the host is not isolated

### Executed macOS result

The lane was executed directly in this worktree.

- `fast`: PASS
- `normal`: PASS
- `thorough`: FAIL
- direct `run_thorough_shared_host.py`: FAIL

The failure was not a code-surface regression in the tested rows that ran. It
was an environment blocker in the shared-host release lane:

- ambient daemon detected: pid `87512`
- ambient daemon binary:
  `/Users/randlee/.local/atm/1.3.0/bin/atm-daemon`
- runner failure contract:
  `shared-host smoke requires an isolated OS user with no existing atm-daemon; refusing to attach to or terminate an ambient daemon`

That blocker is consistent with the AF-1 contract. The script is behaving as
designed by refusing to produce a false singleton proof on a non-isolated host.

### Interpretation

- AF-2/AF-3 adjacent rows covered by `fast`, `normal`, and the non-shared-host
  portion of `thorough` stayed green on `98a4e66c`
- the release-critical AF-1/AF-2/AF-3 shared-host proof could not be freshly
  re-executed on this host because the host was already in active ATM use
- the previously accepted AF evidence recorded in
  `docs/plans/phase-af/readiness.md` remains the last clean shared-host proof
  line until this sprint is rerun on an isolated macOS OS-user

## Windows lane

The Windows lane is intentionally checklist-only in this sprint. Its durable
execution handoff is published in:

- `docs/plans/phase-af/smoke-1.3.1-windows-checklist.md`

That checklist is the artifact the user’s Windows Codex agent should follow.

## Deliverables produced by this sprint

- this plan:
  `docs/plans/phase-af/smoke-1.3.1-cross-host-plan.md`
- Windows checklist:
  `docs/plans/phase-af/smoke-1.3.1-windows-checklist.md`
- macOS execution report:
  `reports/smoke/2026-07-14-21-13-57-smoke-1.3.1.md`
- machine-readable macOS execution payload:
  `reports/smoke/2026-07-14-21-13-57-smoke-1.3.1.json`

## Closeout state

This sprint document is complete because the requested plan, execution attempt,
repo-published evidence, and Windows handoff checklist were all produced.

The macOS release-candidate smoke verdict itself is not green. It remains
blocked on rerunning the shared-host lane under an isolated macOS OS-user with
no ambient daemon.
