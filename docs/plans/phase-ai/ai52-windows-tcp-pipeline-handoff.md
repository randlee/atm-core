# AI.52 Windows TCP pipeline handoff

## Purpose

This fix makes Windows loopback TCP use the same bounded eight-frame
keep-alive dispatch path as Unix. It does not change HTTP framing: the shared
`HttpFrameReader` continues to use `memchr`'s optimized delimiter finder.

## Cwin procedure

1. In `F:\github\atm-core`, use `/sc-git-checkout` to update the AI.52
   worktree and merge or cherry-pick the Windows TCP pipeline fix commit.
2. Run `just test` first. Do not benchmark a failing tree.
3. If `just test` fails, Cwin is authorized to root-cause and fix the failure
   on the AI.52 branch: add a focused regression test, run `just test` again,
   and commit/push the fix. Do not wait for another assignment.
4. Under the dedicated `atm` Windows account, stop/reset the designated
   disposable benchmark daemon and database as needed. Do not use a shared or
   production ATM home/database.
5. Run `just smoke`, `just smoke localhost`, `just smoke local-ip`, and
   `atm doctor --json`. Root-cause, fix, and re-run any failed smoke stage
   before benchmarking.
6. Set `ATM_CAPACITY_HOST_LABEL=windows-x64-01`, then run the release-daemon
   TCP benchmark at frames per connection `1`, `2`, `8`, `16`, and `64`.
   Preserve every run, including failures, through `just benchmark-report`;
   run `just reports-index --check` before committing generated artifacts.

## Result to report

For each profile report accepted/responses, errors, p50 admissions/s, bytes/s,
and the committed `site/reports/send-message-benchmark/` artifact. A failed
or sub-threshold run is diagnostic work, not closure: Cwin must remove
straightforward defects and rerun until the remaining limit requires a
deliberate redesign.
