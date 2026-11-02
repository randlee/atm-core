# HANDOFF — phase-bo, 2026-11-01 22:50 (Claude session with Rand)

## State
- The three lite hooks are done and verified: `design/lite-code/atm-bd-task-{assign,start}-hook.py`, `atm-bd-task-hook.py`
  (49/47/75 lines; 171 total). pyright strict 0 errors against the bo-9 stub; gate deterministic checks pass on all three;
  47 tests pass (`bo-10/tests/lite`, one per Transitions row and per Errors row of `hooks/*.md`).
- `bo-10/` is the PR-ready package for atm-bd-orchestration (see `bo-10/README.md`). Only the Jev half of the gate is
  unrun here (needs TYPESAFE_API_KEY on Rand's machine).
- `design/_superseded/` holds the restart-1 hook, cron, shared/ and state-transitions.md. The old close hook is the
  gate's "stack pushed into close" fixture. Delete after bo-10 merges.
- `anchors.json`: ANC-077 (total lines ≤ 600), ANC-078 (hooks import facts, not decisions), ANC-079 (every bug fix is
  classified before it is written) added as **proposed**. Rand locks.
- `sprint-bo-10-lite-hooks.md` links to the hook files instead of embedding them.
- Uncommitted: everything above. `git status` in the worktree shows it. Commit as Rand (ANC-071 covers hooks/ and intent.md;
  anchors.json edits are proposals until locked).

## Next (one message to fenix; see bo-10/HANDOFF-stop-order.md for the full text)
1. Run the gate's Jev half on the three hooks with the key; commit the runs to anchor-runs/hooks/.
2. Open the bo-10 PR against atm-bd-orchestration from `bo-10/`.
3. Add nothing to anchors.json beyond ANC-077..079; Rand locks them.
4. bo-15 and ANC-075/076 work waits until bo-14 has run one live lite phase.

## Not lite, but raised tonight
- beads-cache: validate every `cache.write` against the central schema and reject mismatches. Agents writing custom
  beads that don't match schema is the root failure the hooks were compensating for; the cache is the one place to stop it.
- atm-bd-orchestration's 50 open bugs: triage each with the ANC-079 question (template / decision row / spec / hook)
  rather than fixing in Python; most are decision-layer bugs (e.g. notifications re-firing every 10 minutes).
