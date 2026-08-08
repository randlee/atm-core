# AL.12 — cwin Windows Hardware Smoke

## Operator contract

1. Use `/sc-git-worktree` to create or update cwin's home sprint worktree for
   `feature/pal-s12-cwin-hardware-smoke-dispatch`. Do not use a docs-only plan
   checkout or an ad-hoc clone.
2. From that worktree, run only `/smoke-test <options>`. The skill selects the
   canonical `just smoke` ladder and writes the self-contained evidence under
   `site/reports/smoke/`; do not invoke an internal Python runner or add a
   second harness.
3. After the run, open one PR from
   `feature/pal-s12-cwin-hardware-smoke-dispatch` to `integrate/phase-al` with
   the status report. Its description must state the tested source SHA,
   host/OS, commands/results, `atm doctor --json` outcome, and the exact
   `site/reports/smoke/` directory printed by `/smoke-test`. Add PR comments
   as stages complete or a failure is investigated, so the record explains
   what was run, what passed, what failed, and any observed problem.

The smoke skill owns the test options, peer discovery, and report schema. A
failure is reported as-is in that PR; do not replace it with an ad-hoc
PowerShell/Python harness, hard-code a host or IP, introduce TLS, or add replay
behavior. cwin is authorized to diagnose and fix a straightforward issue in
this home sprint branch, rerun the affected smoke stage, commit/push the repair,
and update the same PR. For a difficult design or product decision, ask Rand
directly for advice after pushing the smallest reproducible WIP/fix and its
evidence to this branch; then request my technical review of that exact commit.
Do not relay code or logs through a long message chain and do not invent a
workaround.
