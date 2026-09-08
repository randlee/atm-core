# Post-mortem: HERMES-SKILL-TESTS, ATM 1.5.8, fixture: hermes-testbed

Run 8 of 2026-09-08, 06:57:07–07:02:06 UTC: one command (`./test.sh`, randlee/atm-hermes-testbed
PR #5 at 7278a19), no in-container patching, no intervention. `test.sh` resolved the latest
prerelease tag, downloaded the ATM archive and the `hermes-atm-wheels-linux-aarch64` wheels, rebuilt
the testbed layer (skills are baked in), started the fixture (`run.sh --gateway --no-peer`) and sent
the seven sentences of `SMOKE-TEST-RUNBOOK.md` §5. Oversight: `oversight@testbed` inside the fixture
(reports are read there by `test.sh`; nothing crosses to the host). Tester: Claude Code
(`tester@testbed`, haiku, driven by ATM nudges through herdr). Hermes: the fork gateway on the
`api_server` platform (`hermes@testbed`), driven by ATM nudges exactly like the tester (#1307,
landed in 1.5.8 by PR #1327) — no headless CLI session any more.

Inputs: ATM 1.5.8 (`prerelease/v1.5.8` at d43629171; prerelease-archive and ci.yml artifacts of that
commit), hermes fork randlee/hermes-agent at d45230aac (Hermes Agent v0.21.0), herdr 0.8.2.

Wall clock: 5 minutes end to end with the base image cached (inputs 2 s, build 10 s, fixture start
19 s, first sentence 06:57:39, last report 07:02:06). Every skill sent its START line within 30 s of
its sentence; no WAIT line was needed (no wait exceeded 60 s).

## Reports

| # | skill | agent | tools | result | skill elapsed |
| --- | --- | --- | --- | --- | --- |
| 1 | atm-setup-environment | tester | cli | PASS 5/5 (step 5 cross-host SKIP: no peer named) | 1 s |
| 2 | atm-setup-environment | hermes | native+cli | PASS 6/6 (step 5 SKIP, same) | 30 s |
| 3 | atm-smoke | tester | cli | PASS 9/9 | 5 s |
| 4 | atm-smoke | hermes | native+cli | PASS 9/9 (partner ack in 28 s) | 45 s |
| 5 | atm-hermes-ready | tester | cli | PASS 4/4 (pong in 10 s) | 10 s |
| 6 | atm-nudge-roundtrip (responder) | hermes | native | PASS 5/5 | 20 s |
| 7 | atm-nudge-roundtrip (tester) | tester | cli | PASS 3/3 (ack in 10 s) | 10 s |

Verdict line of `test.sh`: `PASS  skills reported 7/7, PASS 7/7 (7 report messages)`, exit 0.
Evidence in the testbed worktree: `.cache/results/20260908T065708Z/` (report-1..7.txt, result.txt,
run.log, build.log, gateway-logs/, tester/transcript.jsonl).

What 1.5.8 proves that 1.5.7 could not: the gateway receives and acts on ATM nudges (report 2, 4 and
6 are the gateway's own work; the 1.5.7 run needed a headless `hermes chat` for each), the
hermes-ready pong arrives (1.5.7: FAIL by design, #1307), and the roundtrip ack takes 10 s instead
of the 1.5.7 run's 6 minutes.

## FAIL lines and their causes

None in run 8. Runs 3–7 the same day each ended on one FAIL; each was root-caused from the saved
transcripts and gateway logs, fixed between runs and never recurred. None was an ATM or Hermes
defect.

| run | FAIL | cause | fix (testbed PR #5) |
| --- | --- | --- | --- |
| 3 | smoke #4 hermes: tester's message answered before the headless session saw it | two actors under one identity (gateway + headless CLI) | fc6f21e: `h()` sends a nudge to the gateway, headless path dropped from `test.sh` and runbook §5 |
| 4 | tester wait steps reported "no message" while the row existed | tester wrapped `atm list --json` in a `python3 <<EOF` heredoc, which replaced the pipe as stdin | 7687ee4: every wait step is one fixed `atm list --from/--contains/--since` command; REPORT.md forbids heredoc parsers |
| 5 | hermes-ready #5: pong not matched | tester pinged with the bare word "ready", the gateway replied "ack" | d994e20: exact ping line, pong = any reply since `--since "$T"` |
| 6 | verdict counted 7 messages but two were the same (skill, agent) | duplicate report collapsed a phase | 360ecbe: gate counts distinct (skill, agent) slots |
| 7 | smoke #4: ack reply run-id ambiguous; a troubleshoot report counted as a slot | run-4 wording named the wrong run-id; verdict counted non-test skills | 7278a19: step 7 acks with the partner's run-id, step 8 filters on it; only the four test skills' slots count |

## Interventions by the oversight agent

None during run 8. Between runs, only the fixes above (skills and `test.sh`), each committed and
pushed before the next run so the next run could not hit the same thing twice.

## Runbook and script lines changed since the 1.5.7 report

- `SMOKE-TEST-RUNBOOK.md` §5: `H` is the gateway driven by nudge; the headless recipe is gone (run 3
  proved one identity must have one actor). The "Rehearsed" line should now read: run 8, 2026-09-08,
  seven sentences, seven reports, 5 minutes with the image cached.
- `test.sh` (new since 1.5.7, commits 89f3c78…7278a19): one command from a clean checkout — resolves
  the latest prerelease tag, looks the ci.yml run up by workflow and commit (not by the run's overall
  conclusion), builds, starts, sends the seven sentences, polls `oversight@testbed`, prints the verdict
  with every failed step and its cause, saves everything under `.cache/results/<UTC>/`.
- `bringup.sh`: installs the hermes-atm hook with `--platform api_server` when the installer supports
  it (e5dc3c3); the gateway is started explicitly and waited for.
- Skills: `atm-smoke` step 1 records `T=$(date -u +%Y-%m-%dT%H:%M:%SZ)`; step 7 acks with the
  partner's run-id; step 8 waits with `atm list --unread --from <partner> --contains "atm-smoke ack"
  --since "$T" --json`. `atm-hermes-ready` sends an exact ping line and accepts any reply since the
  ping. `REPORT.md`: no heredoc parsers, one fixed list command per wait step.

## Product observations

- 1.5.8 on the fixture: no defect found by these seven skills. `atm list --from/--contains/--since
  --json` (1.5.8) is what made fixed wait commands possible; the tester never had to parse anything.
- Gateway log noise, not defects (recorded by solar for the next canonical seam patch):
  `inject_internal_message: visible notice was not delivered: API server uses HTTP request/response,
  not send()` and `Fallback send also failed`. The nudge is still processed.
- Cross-host (`--peer`) leg not exercised in run 8 (`--no-peer`; step 5 SKIP on both sides). It was
  proven in the 1.5.7 run; #1309 still blocks host → container sends over the link.

## Recommended changes

- Fixture: none required; the definition of done (a cold haiku agent runs `./test.sh` from the repo
  alone) is met on this host. PR #5 is ready for review.
- Next: run the same command on the `--peer` fixture once #1309 lands; add the first non-Hermes
  daemon integration case as one more sentence + report (the spin-up is now cheap, per Rand's ruling
  that all daemon integration tests run in colima).
