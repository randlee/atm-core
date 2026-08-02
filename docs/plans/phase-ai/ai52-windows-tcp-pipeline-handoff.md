# AI.52 Windows TCP benchmark handoff

## Purpose

`feature/pAI-s52-windows-transport-benchmark` is the only Windows benchmark
branch. It contains bounded eight-frame TCP dispatch and the benchmark
runner. Do not use `fix/ai52-windows-tcp-pipeline`; it is an unverified,
superseded stacked experiment.

HTTP framing is unchanged: shared `HttpFrameReader` uses `memchr`'s optimized
delimiter finder.

## Cwin procedure

1. In `F:\github\atm-core`, use `/sc-git-checkout` for
   `feature/pAI-s52-windows-transport-benchmark`, then pull its current head.
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
6. Set `ATM_CAPACITY_HOST_LABEL=windows-x64-01`, then run the complete
   release-daemon TCP matrix at frames per connection `1`, `2`, `8`, `16`, and
   `64`. Run every profile without waiting for a new assignment. Preserve each
   run, including failures, through `just benchmark-report`; run
   `just reports-index --check` before committing generated artifacts.

## Result to report

For each profile report accepted/responses, errors, p50 admissions/s, bytes/s,
and the committed `site/reports/send-message-benchmark/` artifact. A failed
or sub-threshold run is diagnostic work, not closure: Cwin must remove
straightforward defects, rerun that profile, and still complete the matrix.
Only a limit requiring deliberate redesign is a blocker. Commit/push all code
and evidence to this branch, then send one concise completion report to
team-lead.
