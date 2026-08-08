# AL.14 — cwin Cross-Host Smoke

**branch:** `feature/al-14-smoke`
**worktree:** `../atm-core-worktrees/feature/al-14-smoke`
**base:** current `origin/integrate/phase-al`
**owner:** cwin operator
**peer:** M5 operator
**unblocks:** AL.15

## Goal

Create or refresh `feature/al-14-smoke` from current
`origin/integrate/phase-al` with `/sc-git-worktree`, then run the repository
smoke harness from cwin against the current ATM runtime and M5 peer. This is
the Windows-originating evidence lane. Use the existing `/smoke-test` skill
and its `just smoke` commands; do not create another runner.

## Required tests

Run these rows in order using the cwin home worktree. `/smoke-test` owns the
matched CLI/daemon selection and readiness prerequisite:

| Row | Required proof |
|---|---|
| Runtime health | `atm doctor --json` reports the selected pair ready. |
| Localhost | `just smoke localhost` passes. |
| Local IP | `just smoke local-ip` passes. |
| Peer readiness | The skill's peer-preflight against M5 passes. |
| Direct send | The skill's cross-host send row proves cwin→M5 and M5→cwin delivery. |
| Acknowledgement | The skill's cross-host acknowledgement row proves both requires-ack/reply directions. |
| Benchmark | After the smoke ladder, `just benchmark` and `just benchmark-report` complete and retain the standard benchmark report. |

Use the existing configured peer identity and endpoint. Do not hard-code host
addresses in the repository. The benchmark runner owns an isolated daemon and
database; do not run it beside the live daemon selected for smoke.

## Report and PR

Follow `/smoke-test` so every smoke run produces its self-contained report
under `site/reports/smoke/` and is registered in `site/reports/index.html`.
`just benchmark-report` retains the benchmark artifact through the same master
reports navigation. Open a PR from `feature/al-14-smoke` to
`integrate/phase-al` containing the retained artifacts and a short status
report with the tested commit, Windows platform, command-row results, report
links, and the first failure if any.

Fix a defect on this home branch when it is safe and in scope, then rerun from
the first affected row. Ask Rand before changing transport behavior.

## Acceptance

- Every required row has a retained PASS report, or the PR records its first
  failing row and blocks the later rows.
- The cwin smoke and benchmark reports are reachable from
  `site/reports/index.html`.
- The PR truthfully reports the cwin result; it does not claim M5 completion.
