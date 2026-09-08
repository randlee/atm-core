# Colima integration run 9: atm 1.5.9 (integrate/phase-ay) with Hermes, herdr socket transport

- verdict: **PASS** (7/7 skill reports, every step PASS, no intervention)
- run: 2026-09-08 16:23:12Z start, 16:36:21Z verdict (13 minutes including the testbed image layer rebuild)
- command: `./test.sh` in atm-hermes-testbed (branch `feat/atm-test-skills` at 6c81af1), no arguments
- files beside this report are the run's outputs, unedited: `result.txt`, `report-1.txt` … `report-7.txt` (the seven
  skill reports as received in the fixture's oversight inbox), `herdr-doctor.json` (`atm doctor --json` `.herdr` inside
  the fixture after bringup), `index.html` and `smoke.envelope.json` (written by test.sh for the report index)

## Inputs (all from the network, pinned)

| component | source | pin |
| --- | --- | --- |
| atm-daemon + atm CLI 1.5.9 | prerelease-archive.yml run 34249771093, artifact `aarch64-unknown-linux-gnu` | tag `prerelease/v1.5.9` = 0c2d38368 on integrate/phase-ay |
| hermes_atm + atm_graft 1.5.9 wheels | ci.yml run 34249772325 (PR #1308), artifact `hermes-atm-wheels-linux-aarch64` | same commit |
| herdr | GitHub release `herdrdev/herdr` v0.8.2, sha256-checked | build.sh |
| Hermes | fork randlee/hermes-agent, `main` = d45230aac (Hermes Agent v0.21.0, 2026.8.31) | build context |

## The seven reports

| # | skill | agent | tools | result |
| --- | --- | --- | --- | --- |
| 1 | atm-setup-environment | tester@testbed (Claude Code, herdr backend) | cli | PASS 5/5 |
| 2 | atm-setup-environment | hermes@testbed (gateway) | native+cli | PASS 6/6 |
| 3 | atm-smoke | hermes@testbed | native+cli | PASS 10/10 |
| 4 | atm-smoke | tester@testbed | cli | PASS 8/8 |
| 5 | atm-hermes-ready | tester@testbed | cli | PASS 4/4 |
| 6 | atm-nudge-roundtrip | hermes@testbed | native | PASS 5/5 |
| 7 | atm-nudge-roundtrip | tester@testbed | cli | PASS 2/2 |

Every sentence produced its `ATM TEST START` line within a minute; the one `WAIT` line (tester smoke, step 8) was
the 60 s tick while the gateway answered.

## herdr transport: socket (native IPC), proven

The fixture daemon's home config carries `[herdr] transport = "socket"` (written by bringup.sh before the daemon
starts; 1.5.9's default is also socket). `herdr-doctor.json` shows `transport: socket`, endpoint
`$HOME/.config/herdr/herdr.sock`, breaker closed. Every nudge to the herdr-backed `tester` delivered over that path
(reports 1, 4, 5, 7 exist only because the tester was nudged).

Finding, not a run failure: the doctor's herdr **status query** over the socket returns
`unexpected_response / internal_failure: "Herdr status query did not return a supported response"` and lists no
members, while delivery works. Filed as atm-core #1335 (Phase AY).

## Context

Runs 3–8 (atm 1.5.8, 2026-09-08) hardened the fixture to zero interventions; their FAIL/cause/fix table is in
`docs/plans/hermes-integration-tests/reports/hermes-skill-tests-1.5.8-hermes-testbed.md` on atm-core branch
`feat/atm-test-skills`. Run 9 is the first run on integrate/phase-ay and the first over the socket transport.
Not covered: host→container sends over the peer link (`--no-peer`; blocked by atm-core #1309), Windows and x86_64.

Reproduce: `./test.sh` from a checkout of atm-hermes-testbed with `env/allowlist.env` present; it picks the latest
`prerelease/vX.Y.Z` tag on its own and writes this bundle under `.cache/results/<UTC>/site/`.
