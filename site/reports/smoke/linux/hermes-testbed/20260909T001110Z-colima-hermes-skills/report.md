# Colima integration run 10 — ATM 1.5.11 (2026-09-09T00:11Z)

Verdict: **FAIL** (test.sh gate), skills **7/7 PASS**.

| item | value |
| --- | --- |
| ATM under test | 1.5.11, tag `prerelease/v1.5.11` @ 1768a2e75 (develop, PR #1349) |
| Fixture | randlee/atm-hermes-testbed main 3bc3246 (`./test.sh`, `run.sh --gateway --no-peer`) |
| Roster shape under test | `tester` on the herdr backend with `--session default` (the rand-m4 shape from atm-core #1342) |
| Skill reports | 7/7 received, 7/7 PASS (setup-environment ×2, smoke ×2, hermes-ready, nudge-roundtrip ×2) |
| herdr doctor | every endpoint `unexpected_response` / `internal_failure` — "Herdr socket server status must use the ping probe" |

## What the run proves

- The #1342/#1343 fix holds: a `--session default` roster row is nudged over the herdr socket (`default` = the root `herdr.sock`); all four tester sentences produced START lines and reports, and both nudge-roundtrip legs passed.
- The #1348 doctor detail fix surfaces the real error text instead of the generic catch-all.

## Why the verdict is FAIL

`test.sh` now requires `state.kind == "ok"` on every herdr doctor endpoint (testbed #8, added after run 9 was graded PASS with a red doctor state, atm-core #1347). The doctor probe on the socket transport is dead-wired: `doctor_probe.rs` sends `HerdrOp::StatusServer`, which the socket encoder rejects; the intended ping path (`transport_socket.rs::server_info`) has no callers. Filed as atm-core **#1351**. The transport itself works, as the seven reports show. The `herdr transport` header row in `colima-hermes-skills.json` is graded by transport name (#1347) and therefore reads PASS; it is left unedited.

Files beside this report are the unedited run outputs (`result.txt`, `report-1..7.txt`, `herdr-doctor.json`) plus the rendered smoke set.
