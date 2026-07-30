# AI.33 fastpc4 capacity execution handoff

This is the current operational handoff for cwin's AI.33 validation work on
fastpc4. It supersedes any earlier handoff statement that treated the machine's
existing ATM state as a reason to stop the capacity exercise.

## Authority and objective

- fastpc4 is a validation host; it is not carrying a protected production ATM
  service or database.
- cwin is authorized to stop, replace, configure, and rebuild the fastpc4 ATM
  daemon and its database as required by this exercise.
- cwin owns root cause, source fixes for defects encountered, and commits/pushes
  of those fixes and evidence to this branch. Do not wait for another agent to
  authorize an obvious correctness repair.
- The objective is a truthful baseline for the public Windows loopback admission
  path: after obvious functional defects are fixed, measure where the current
  implementation lands against 1,000 samples per second. This is a baseline,
  not permission to hide a failure with retries, longer deadlines, extra
  workers, or weaker assertions.

## Exact environment

- Worktree:
  `F:\\github\\atm-core-worktrees\\feature\\pAI-s33-admission-capacity-smoke`
- Branch:
  `feature/pAI-s33-admission-capacity-smoke`
- Before each run, pull/rebase this branch, record `git rev-parse HEAD`, and
  build the CLI and daemon from that exact worktree.
- The runner-owned daemon and database may replace any prior fastpc4 ATM state.
  Ensure exactly one matching daemon is running for each live test.

## Required loop

1. Run `just test`, then `just smoke localhost`. Fix any genuine code defect
   revealed by either command; add a deterministic regression test when the
   defect is not already covered. Commit/push the fix and its evidence.
2. Run:

   ```powershell
   $env:ATM_CAPACITY_ISOLATED_OS_USER = '1'
   python scripts/smoke/run_admission_capacity.py
   ```

   The runner's disposable SQLite database and release-built daemon are the
   test state. Do not substitute an ambient daemon or a hand-written request
   loop.
3. On any runner startup, admission, response, or one-second interval failure:
   collect the JSON artifact, daemon stdout/stderr, retained logs, listener and
   process details, and the exact command. Root-cause the failure before
   optimizing. If an obvious defect is found, fix it on this branch, test it,
   commit/push, and repeat from step 1.
4. Once obvious correctness defects are gone, run the full twenty-interval
   baseline (ten accepting-peer and ten unavailable-peer intervals). Report
   accepted writes, responses, elapsed time, failures, and the slowest interval
   for every sample. A result below 1,000 accepted writes and 1,000 responses
   within one second is valuable baseline evidence, not a reason to stop.
5. Repeat the loop until either all twenty intervals pass or the remaining
   limit has a documented root cause and reproducible evidence. Do not change
   the gate to make the result pass.

## Guardrails

- No automatic retry of a side-effecting send. A reset after a partial write
  might otherwise duplicate an immutable message.
- No timeout increase, queue increase, or worker-count increase merely to mask
  a baseline failure.
- Keep every live run tied to the recorded branch commit and matching CLI and
  daemon binaries.
- Generated databases, logs, private keys, and certificates remain out of git;
  commit only sanitized reports and references to the generated artifacts.

## Handoff record

For every run, append a concise dated entry with commit, commands, result,
artifact paths, and the next concrete action. Commit/push each entry so the
Mac-side reviewers can inspect it without accessing fastpc4 directly.
