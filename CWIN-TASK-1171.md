# cwin task: #1171 Windows benchmark gap (post-1.5.0 release)

Branch: `evidence/readiness-1.5.0-windows` (based on `develop` @ or after `5b1baacef`, v1.5.0 back-merged)
Host: FastPC4
Issue: https://github.com/randlee/atm-core/issues/1171

## Context

The 2026-08-01 windows-x64-01 TCP send-family floor (8,793.91 msg/s, f16/64
profile) was seeded from a run that later turned out to key on
`host_label+target` only, which mis-gated cwin's own 8-frame readiness runs
(tracked separately as RRG-BENCH-FLOOR-PROFILE-KEY-001, already fixed/merged
in PR #1158 — no action needed on that part). What remains open for #1171 is
explaining a residual ~9.8% throughput gap on real TCP send campaigns, plus
reproducing a query-path FTS saturation, and capturing a lint TypeError that
does not reproduce on macOS.

**Do not edit any floor value.** Floors move only via a quality-mgr-approved
reseed, and never downward. This task is evidence-gathering only.

## Isolation (no new OS account required)

Use a dedicated `ATM_HOME` instead of a dedicated OS account:

1. Create a fresh directory, e.g. `C:\atm-bench\home-1.5.0`, and set
   `ATM_HOME` to it for the entire campaign shell.
2. Build (or use) the candidate `atm`/`atm-daemon` from this worktree's
   commit (develop-derived, v1.5.0-line).
3. Start your own `atm-daemon` from that build, with the loopback TCP
   interface bound to a port no other daemon is using — the harness already
   accepts `ATM_CAPACITY_DIRECT_PEER_PORT` for this.
4. Before every campaign:
   - `tasklist` — confirm no other `atm-daemon.exe` is running.
   - `atm doctor --json` — confirm it reports the expected `ATM_HOME` path
     and port.
   - Confirm the interactive `windows-x64-01` daemon is stopped for the
     duration of the campaign.
5. Run the whole campaign shell **non-interactively via a Scheduled Task**
   (`schtasks`, "run whether user is logged on or not", no window) — this is
   the Windows analogue of the macOS `m5-atmbench` isolated non-interactive
   account, and is the isolation mechanism we're trusting for evidence
   provenance here. No desktop session or focus changes should touch the
   campaign while it runs.

### Provenance — record all of this in every report

- Host, account, `ATM_HOME` path, daemon port
- Candidate commit (the exact `develop`/branch SHA you built from)
- `ATM_CAPACITY_HOST_LABEL=windows-x64-01-isolated` (so the reports index
  separates this from the earlier ad hoc windows-x64-01 runs)
- The Scheduled Task name

No environment dumps, no config file contents — just the fields above plus
the benchmark/tooling output itself.

## Steps

1. **TCP send-family throughput** — run the f16/64 profile (matching the
   floor's seed shape), three campaigns, TCP send only. Profile the TCP send
   path during one of the three campaigns (ETW, or the harness-side timing
   breakdown — whichever isolates client vs. daemon vs. socket time on this
   host). Goal: explain the residual ~9.8% gap versus the 2026-08-01
   8,793.91 msg/s PASS. Report whether the gap looks like candidate code,
   account/host state, or harness shape. **No floor edits.**

2. **Query FTS saturation reproduction** — reproduce the fixed 32-way fanout
   that returned `ATM_DAEMON_CONNECTION_SATURATED` for 7 of 236 requests.
   Capture the daemon's saturation counters and the effective reader-lane
   settings from `atm doctor --json` (the `reader_lanes` block: pool_size,
   queue_depth). Report whether the Windows lane sizing explains the
   saturation rate.

3. **Lint aggregate TypeError (issue #1171 item 5) — evidence only, no fix
   expected.** This does not reproduce on macOS (cipher ran the full
   aggregate plus reversed/randomized boundary suites clean, no shared state
   found). On this isolated `ATM_HOME` setup, run a fresh `just lint` and
   capture:
   - the exact traceback
   - the Python version in use
   - the test execution order for that run

   Attach this to issue #1171 as a comment. Do not attempt a fix — root
   cause will come from this Windows-side evidence, handled separately.

## Deliverable

1. Commit all evidence (benchmark reports, saturation-repro output, lint
   traceback capture) to this branch (`evidence/readiness-1.5.0-windows`).
2. Open a PR from this branch to `develop`.
3. **In the PR description, add a report section** covering, for
   team-lead/fenix to read:
   - Provenance fields (see above) for every campaign run
   - TCP send-family gap: measured numbers, profiling findings, and your
     assessment of the cause (candidate code / host-state / harness shape)
   - FTS saturation: counters + reader-lane settings + your assessment
   - Lint TypeError: traceback + Python version + test order, attached to
     issue #1171 (link the comment from the PR description too)
   - Confirm reports-index is green (`just` recipe you used, and its result)
4. Do not merge the PR yourself — team-lead/fenix will route it through the
   standard QA + merge-approval flow once it's up.
