# AL.14 — cwin Cross-Host Smoke

**branch:** `feature/al-14-smoke`
**worktree:** `../atm-core-worktrees/feature/al-14-smoke`
**base:** current `origin/integrate/phase-al`
**owner:** cwin operator
**peer:** M4 operator (Rand's Mac)
**unblocks:** AL.15

## Goal

Create or refresh `feature/al-14-smoke` from current
`origin/integrate/phase-al` with `/sc-git-worktree`, then run the repository
smoke harness from cwin against the current ATM runtime and the M4 peer. This
is the Windows-originating evidence lane. Use the existing `/smoke-test` skill
and its `just smoke` commands; do not create another runner.

The cwin host has the M4 endpoint in its peer database but cannot SSH to M4.
That is not a transport failure. The existing `peer-preflight`,
`crosshost-send`, and `crosshost-ack` feature runners use SSH to operate the
remote machine, so they are deliberately **not** evidence for this lane. Do
not configure SSH, add a runner, or commit an IP address to work around that
limitation. The direct public `atm` commands below test the actual ATM HTTP
transport using the existing durable M4 peer record.

## Final-candidate round: run from this same home worktree

The current retained reports are diagnostic evidence, not closure for a newer
integration candidate. CWin performs every next round from **this same**
`feature/al-14-smoke` worktree; do not create a second smoke runner or a new
test branch.

Before a final round:

1. Fetch `origin`, then non-destructively merge the current
   `origin/integrate/phase-al` into this branch. Record both the frozen
   integration parent SHA and the resulting local candidate SHA before
   building. Do not silently test an older branch merely because the existing
   daemon remains healthy.
2. Use `/smoke-test` / daemon-switch to select the matching release CLI and
   replacement HTTP runtime as one pair, then verify `atm doctor --json`.
   Never select a legacy daemon or mix binaries from different revisions.
3. Run the required rows below in order. The final benchmark is a fresh six
   profile TCP campaign (`F1`, `F2`, `F4`, `F8`, `F16`, `F64`) through the
   existing `just benchmark` and `just benchmark-report` path.
4. Commit/push the generated reports and a PR status update identifying the
   frozen candidate, the six profile outcomes, direct SQLite admission metric,
   durable restart result, and any M4-dependent row outcome.

The generated benchmark landing page must report the **current campaign**
status separately from retained historical status. It must not erase, relabel,
or hand-edit historical failures; conversely, an older failed panel must not
make a clean current six-profile campaign appear to have failed.

If a reproducible Windows, shared-runtime, smoke-harness, or report-generator
defect is found, CWin is authorized to fix it in this home worktree, add a
focused regression test, and rerun from the first affected row. Preserve that
commit for diagnosis, but do not merge the mixed evidence branch as product
code: identify the fix commit for extraction into a fresh
`/sc-git-worktree` from `origin/integrate/phase-al`, followed by its own
quality-reviewed PR. Keep reports and plan/evidence commits on this home
branch.

## Required tests

Run these rows in order using the cwin home worktree. `/smoke-test` owns the
matched CLI/daemon selection and readiness prerequisite:

| Row | Required proof |
|---|---|
| Runtime health | `atm doctor --json` reports the selected pair ready. |
| Localhost | `just smoke localhost` passes. |
| Local IP | `just smoke local-ip` passes. |
| Direct send | cwin and M4 each send one unique ordinary message through their configured peer record; the other operator reads the exact message ID and text with public `atm read`. |
| Acknowledgement | cwin and M4 each send one unique `--requires-ack` message; the receiver reads and acknowledges it; the sender reads the acknowledgement reply and verifies it names the original message ID. |
| Benchmark | After local runtime health succeeds, run `just benchmark` and `just benchmark-report` against the matched candidate. Retain the standard report for every Windows TCP profile (`F1`, `F2`, `F4`, `F8`, `F16`, `F64`). This row is independent of M4 reachability. |

Use the existing configured M4 peer identity and endpoint. Do not hard-code
host addresses in the repository: the M4 IP is operator input already held in
the cwin peer database. The equivalent target spellings
`<m4-agent>@<team>.<m4-host>` and `<m4-agent>@<team> --host <m4-host>` are
wire-equivalent; use the form already configured for the durable peer record.
The benchmark runner owns an isolated daemon and database; do not run it
beside the live daemon selected for smoke.

### Windows benchmark parity gate

This is a localhost TCP performance proof, not a synthetic SQLite-only probe.
Use the exact `origin/integrate/phase-al` SHA that built both the selected CLI
and benchmark daemon. Record the TCP F1 p50 throughput, p50/p99 latency, and
the durable accepted count after the controlled restart for every profile.

Compare CWin's TCP F1 p50 with an M5 TCP F1 run built from that **same
candidate SHA**. CWin passes the parity gate at 85% or more of the matched M5
TCP F1 rate. Do not use an older report as a release gate: historical
`develop` evidence is useful context, but it predates the current runtime and
benchmark path. The current M5 reference panel is the only comparable
baseline.

If CWin is below the 85% gate, retain the failed benchmark panel and collect
read-only evidence before changing code: selected executable paths and SHA,
daemon ownership/doctor output, process CPU and memory, listener endpoint,
and applicable Defender Firewall/exclusion state. Do not disable Defender,
alter VPN routes, or lower the threshold. CWin is authorized to diagnose and
fix a reproducible, in-scope Windows or shared runtime defect on this home
branch, add regression coverage, rerun the affected benchmark profiles, and
commit the fix. Extract any such product-code fix into the separate reviewed
PR described above before it reaches integration. Transport-policy,
security-policy, or benchmark-threshold changes require Rand's decision first.

For the two direct-peer rows, record a UTC token in each body (for example,
`al14-cwin-to-m4-send-<UTC>` and `al14-m4-to-cwin-send-<UTC>`), run
`atm send ... --json`, and retain each returned message ID. For each
acknowledgement direction, add `--requires-ack`; the receiver runs
`atm read --team <team> --message-id <id> --json`, then
`atm ack --team <team> <id> <reply-body> --json`. The sender runs
`atm read --team <team> --message-id <reply-id> --json` and confirms both the
exact reply body and `acknowledgesMessageId == <id>`. Coordinate the M4 read,
send, and ack over ATM/team communication, not SSH. M4 retains its matching
transcript; the cwin report cites every message ID and its M4 counterpart.

If direct delivery fails, retain the client error and stop only the remaining
M4-dependent direct-peer row. Still run the independent benchmark row after
the local runtime rows pass. Do not describe SSH unavailability as a network
test failure.

## Report and PR

Follow `/smoke-test` so every `just smoke` run produces its self-contained
report under `site/reports/smoke/` and is registered in `site/reports/index.html`.
`just benchmark-report` retains the benchmark artifact through the same master
reports navigation. Add a cwin status report to the PR for the direct public
CLI rows: tested commit, Windows platform, target peer alias (never its raw
IP), commands, message IDs, exact-body verification, M4 counterpart, and the
first failure if any. Open a PR from `feature/al-14-smoke` to
`integrate/phase-al` containing the retained artifacts and status report.

Fix a reproducible defect on this home branch when it is safe and in scope,
then rerun from the first affected row. Include the commit SHA, diagnosis, and
before/after benchmark panels in the PR. Ask Rand before changing transport,
security, or benchmark-policy behavior. A code fix remains an evidence-branch
commit until it is cleanly extracted to a fresh integration-based QA PR; do not
merge it incidentally with the retained smoke artifacts.

## Acceptance

- Local rows have retained `/smoke-test` PASS reports, or the PR records their
  first failure.
- Direct-peer rows have cwin and M4 correlated public-CLI transcripts proving
  the exact message IDs, bodies, and acknowledgement relationship; or the PR
  records the actual first transport failure. SSH availability is not a row.
- The independent benchmark row is retained whenever the local runtime rows
  pass, even when an M4-dependent row fails.
- Windows TCP F1 reaches at least 85% of the same-candidate M5 TCP F1 p50, or
  the PR retains the failing panels and the required read-only diagnosis with
  no threshold/security workaround.
- The cwin smoke and benchmark reports are reachable from
  `site/reports/index.html`.
- The PR truthfully reports the cwin result; it does not claim M4 completion
  without M4's correlated read/ack evidence.
