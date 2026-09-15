# Colima integration run 11 — ATM 1.5.14 (2026-09-09T06:24Z)

Verdict: **PASS** (test.sh gate), skills **7/7 PASS**.

| item | value |
| --- | --- |
| ATM under test | 1.5.14, tag `prerelease/v1.5.14` @ b65558165 (develop lineage, PR #1372); prerelease-archive run 34317899892, ci run 34317921603 |
| Fixture | randlee/atm-hermes-testbed main 3bc3246 (`./test.sh`, `run.sh --gateway --no-peer`) |
| Roster shape under test | `tester` on the herdr backend with `--session default` (the rand-m4 shape from atm-core #1342) |
| Skill reports | 7/7 received, 7/7 PASS (setup-environment ×2, smoke ×2, hermes-ready, nudge-roundtrip ×2) |
| herdr doctor | every endpoint `{'kind': 'ok', 'version': '0.8.2', 'protocol': 20}` on the socket transport |
| Versions in the fixture | Hermes Agent v0.21.0 (2026.8.31) · upstream d45230aa; atm-daemon 1.5.14; herdr 0.8.2 |

## What the run proves

- **atm-core #1351 is closed by this run.** Run 10 failed the `test.sh` gate because the herdr doctor probe was dead-wired: every endpoint reported `unexpected_response` / `internal_failure` while the transport itself worked. On 1.5.14 every endpoint reports `state.kind == "ok"` with the version and protocol filled in, so the ping probe is now reaching the socket server. This is the first colima run to pass the doctor gate testbed #8 added.
- The #1342/#1343 nudge fix continues to hold on a `--session default` roster row: all four tester sentences produced START lines and reports, and both `atm-nudge-roundtrip` legs passed, one over the native tool path and one over the CLI.
- The candidate carries the merges of atm-core **#1363** (Herdr doctor target resolution, four QA rounds, including the drain-and-join fix for the claim-release shutdown gap) and **#1367** (prerelease publish-kit adoption, QA PASS 7/7). Neither introduced a regression in any of the five skills.
- `atm doctor` inside the fixture reports `healthy` with two info findings and zero warnings or errors.

## Release-pipeline result recorded alongside the run

1.5.14 is the first candidate whose `prerelease-archive.yml` run succeeded end to end, including the `release` job, and therefore the first GitHub prerelease Release produced from develop lineage: six assets, five archives plus `checksums.txt`, non-draft. The two preceding candidates failed that job with `fatal: not a git repository` because the job ran `gh release create` with no `actions/checkout` and no explicit repository; #1367 fixed it. `prerelease/v1.5.13`'s archives exist only as Actions artifacts, which is why the integration candidate moved to 1.5.14.

## Timing

Inputs resolved 06:24:05Z, fixture up 06:24:37Z, first skill sentence 06:24:56Z, verdict 06:30:04Z — six minutes end to end on a warm build cache.

Files beside this report are the unedited run outputs (`result.txt`, `report-1..7.txt`, `herdr-doctor.json`) plus the rendered smoke set.
