---
title: Windows benchmark performance investigation and remediation
issue: 1030
branch: plan/1030-windows-perf-investigation
dev_branch: fix/1030-windows-perf (to be created off develop when dev work starts)
status: pending-quality-review
owner: cwin (Windows dev, fastpc4)
target_host: windows-x64-01 (fastpc4)
reference_host: m5-atmbench (isolated M5 macOS benchmark account)
---

# GH #1030 — Windows benchmark performance: investigation & remediation plan

This plan is written to be **self-contained for cwin on fastpc4**. Every
hypothesis below is grounded in code or committed evidence, with exact
`file:line` pointers into this branch (based on `develop` @ `938767c72`).
No information outside this document and the repo itself should be needed.

## 1. The actual problem (it is bigger than the issue title says)

GH #1030 frames this as "Windows trails the M5 Mac baseline; ~80–85% parity
expected by design." The committed evidence says the real gap is **2.2–2.9x**,
not 15–20%:

| Target | M5 `m5-atmbench` accepted p50 (msg/s) | Windows `windows-x64-01` measured 2026-08-26 (f8, official) | Ratio |
|---|---:|---:|---:|
| sqlite | ~42,500 (floor 35,000) | 16,022.88 | ~0.38 |
| tcp | ~17,600 (floor 17,000) | 6,012.47 | ~0.34 |
| tcp-tls | ~14,600 (floor 13,500) | 5,614.01 | ~0.38 |

Sources: `site/reports/send-message-benchmark/baselines.json`,
`site/reports/send-message-benchmark/20260826T164714Z-windows-x64-01.campaign.json`,
`docs/plans/phase-ao2/evidence/ao2-windows-official-20260826.md`,
M5 campaigns `20260826T204109Z`/`20260826T204324Z`/`20260826T002410Z-m5-atmbench.campaign.json`.

Three load-bearing observations:

1. **The gap exists with no network at all.** The `sqlite` target is a
   standalone storage-admission binary (`atm-daemon-benchmark`,
   `crates/atm-daemon-bootstrap/src/bin/atm-daemon-benchmark.rs`) — no
   sockets, no TLS, no HTTP — and it is already at ~38% of the M5 value.
   Whatever costs Windows here sits **under** tcp and tcp-tls too, because
   every transport target funnels into the same SQLite writer.
   **Rand's direction (2026-08-29): close the sqlite gap first.** WPERF.3
   (transport) is gated on WPERF.2 (sqlite) below.
2. **This 32–38% ratio is not new.** A prior analysis measured Windows at
   ~32–34% of macOS TCP at every frame depth with no root cause found:
   `docs/plans/phase-ai/ai52-windows-mac-tcp-performance.md`. The "~85%
   historical" figure describes fastpc4's general hardware relationship to
   the M5, **not** anything these benchmarks have ever measured. Treat 85%
   as the target, not as a regression-from state.
3. **Windows also regressed against itself.** On 2026-08-01 fastpc4 hit tcp
   p50 8,794 (f16) / 8,298 (f8) at revisions `5d32095…`/`44233a2…`; every
   campaign since 2026-08-21 (revisions `240b2af…`, `78ec600…`, `a944207…`,
   and the 2026-08-26 officials at `098bd7a…`/`c387a45…`) sits at ~6,000–6,900.
   All revisions are recorded in
   `site/reports/send-message-benchmark/historical-record.json` — a
   revision-window comparison can separate "code change" from "host change."
   Same-day variance is also on record: 2026-08-01 tcp f8 was 4,281 at 20:26
   and 8,298 at 21:12; f64 collapses to 1,003–4,018 in most campaigns.

## 2. What the benchmark actually measures (read this before profiling)

The tcp/tcp-tls benchmark does **not** run `atm send` and does **not**
exercise AO2.14 peer pooling. It is a Python HTTP/1.1 client
(`scripts/smoke/run_admission_capacity.py`) speaking directly to the
daemon's direct-peer listener; recipients carry no host, so
`dispatch_resolved_peer_write` returns immediately
(`crates/atm-http-runtime/src/storage_and_nudge_router.rs:355-357`).
Measured path: Python socket → Tokio/Axum direct-peer listener
(`crates/atm-http-runtime/src/lib.rs:556-579`, `http1_server.rs:32-142`) →
`StorageAndNudgeRouter::commit_write` → SQLite writer thread → HTTP 201.
The daemon under test is the shipped release Tokio+Axum `atm-daemon`
(never the frozen legacy sync daemon — do not touch that code; see
`AGENTS.md` hard rule).

Harness facts that shape every number:

- **Batch-and-wait, not pipelining.** `MAX_IN_FLIGHT_REQUESTS = 8`
  (`run_admission_capacity.py:99-102`); `submit_connection` (:1437-1456)
  sends one 8-frame batch then blocks draining all 8 responses. At the
  official f8 profile every connection is exactly one batch, so
  per-connection throughput = 8 / (batch RTT) — latency-bound by design.
- **Connection churn is inside the timer.** 125 fresh connections per
  1,000-message interval (f8), each with TCP connect (+ full mTLS handshake
  on tcp-tls) and `Connection: close` (:1408-1432). ~10 intervals × 3
  targets per campaign.
- **The headline p50 is a median of interval rates**, not per-message
  latency (`benchmark_schema.py:516-529`; floor check
  `run_admission_capacity.py:311-315`). Interval count varies with host
  speed (`run_profile` :1615-1642 loops until 20 s), so slower hosts get
  fewer samples.
- **Platform-divergent client concurrency.** `import resource` fails on
  Windows (:79-82), so `admission_connection_worker_limit` (:1466-1484)
  returns the raw 512 workers unclamped while macOS clamps to
  `RLIMIT_NOFILE - 64`. Inert at f8 today (125 connections both ways) but
  a comparability trap for any profile change.

## 3. Ranked hypotheses for cwin

Each has a cheap **kill-test** — run it before investing in the fix.
Diagnostic runs are fine on any account; **official evidence only from the
dedicated benchmark account (WPERF.1), and never elevate the account,
change power policy, add AV exclusions, or use WSL to improve an official
number** (AO2.8 rule, `sprint-AO2-8-windows-tcp-benchmark-parity.md:132-145`).

### H1 — SQLite commit durability: `synchronous=FULL` + `FlushFileBuffers` (primary; explains the platform-wide level shift)

- `PRAGMA synchronous` is never set anywhere in the repo, so SQLite runs at
  its compiled default **FULL**; under WAL that fsyncs the WAL on every
  commit. WAL enablement: `crates/atm-storage-rusqlite/src/shared_db.rs:594-622`.
- On macOS, SQLite's `fsync()` is not a media flush (F_FULLFSYNC off by
  default); on Windows the VFS issues `FlushFileBuffers` — a true write
  barrier, 10–100x slower and far more variable. Same code, wildly
  different per-commit cost. This matches a large gap on the transport-free
  `sqlite` target.
- Group commit amortizes fsyncs: single writer thread, one Immediate
  transaction per batch (`crates/atm-storage-rusqlite/src/writer/mod.rs:475-495,580,631`).
  Fewer messages per batch ⇒ more fsyncs per message (see H2).
- **Kill-test:** instrument or trace commits-per-second vs messages-per-
  second on the `sqlite` target (batch-size telemetry around
  `collect_batch`, writer/mod.rs:520-573). Then run a **diagnostic-only**
  A/B with `PRAGMA synchronous=NORMAL` on the writer connection
  (`open_writer_connection_for_target`, shared_db.rs:643-654). If Windows
  sqlite p50 jumps toward ~36k, H1 is confirmed.
- **Constraint:** the product guarantees reply-after-commit durability
  (AO2.8 preserves "reply-after-commit durability", and the harness
  verifies exact counts after a daemon restart —
  `durability_after_restart` in every result JSON). Under WAL,
  `synchronous=NORMAL` moves durability to checkpoint boundaries — that is
  a **product decision, not a dev decision**. If H1 confirms, bring the
  measured numbers + a durability analysis to Rand/team-lead before
  changing the shipped default. Acceptable engineering alternatives to
  evaluate: larger/adaptive commit batching (H2), WAL autocheckpoint
  tuning, or platform-conditional durability with an explicit
  documented contract.
- Also verify build parity: rusqlite uses `features = ["bundled"]` on Unix
  vs `["bundled-windows","modern_sqlite","hooks"]` on Windows
  (`crates/atm-storage-rusqlite/Cargo.toml:28,31`) — confirm both produce
  the same SQLite version/compile flags (`PRAGMA compile_options`).

### H2 — 1 ms batch window vs Windows ~15.6 ms timer resolution (primary; explains the extreme variance)

- `BATCH_TIME_BUDGET = Duration::from_millis(1)`
  (`crates/atm-storage-rusqlite/src/writer/mod.rs:26`), enforced via
  `tokio::time::timeout_at` (:520-573). Windows' default system timer
  granularity is ~15.6 ms unless something in-process calls
  `timeBeginPeriod`. If the effective window rounds up to ~15.6 ms —
  or oscillates depending on what else on the host has raised the timer
  resolution — batch sizes (and thus fsyncs/message and admission
  latency) swing run to run and even interval to interval.
- This is the same seam AO2.6 fixed once already
  (`docs/plans/phase-ao2/sprint-AO2-6-admission-writer-batching-regression.md:19-45`).
- It plausibly also explains the **run-to-run variance** item in GH #1030:
  another interactive app raising/releasing the global timer resolution
  changes the daemon's effective batch window. (Note: this is a different
  mechanism from the client-side harness-pipelining suspicion tracked for
  AO2 — related symptom, distinct root; check both, conflate neither.)
- **Kill-test:** log actual batch sizes + inter-commit intervals on
  Windows; compare against macOS. Then A/B with the timer raised
  (`timeBeginPeriod(1)` in a diagnostic build, or run a known
  timer-raising process alongside). If batch sizes stabilize and
  throughput jumps, H2 is confirmed. Fix candidates: count-based batch
  triggering (drain-to-N) rather than pure time-based; Tokio's
  `Builder::event_interval`/coarse-timer awareness; or an explicit
  high-resolution waitable timer on Windows.

### H3 — Harness connection churn + TIME_WAIT/ephemeral-port pressure (transport targets; also a variance source)

- 125 short-lived loopback connections per interval, `Connection: close`,
  no SO_REUSEADDR/linger tuning on either side
  (`run_admission_capacity.py:1424-1432`). Windows default: 4-minute
  TIME_WAIT, dynamic port range 49152–65535. Sequential targets in one
  campaign compound the pressure.
- **Kill-test:** watch `netstat -ano | findstr TIME_WAIT` count during a
  run; correlate interval throughput vs accumulated TIME_WAIT sockets. A
  diagnostic harness variant with connection reuse (raise
  frames_per_connection) isolates churn cost — history already hints at
  this: tcp f16 (8,794) beat f8 on 2026-08-01.
- **Constraint:** the harness is a shared measurement contract — any change
  to its connection model changes the numbers for macOS too and
  invalidates existing floors. Harness changes land in a separate PR,
  flagged to team-lead, with re-baselining implications stated (see §6
  and the AO2 harness-pipelining suspicion, which is deferred but
  related evidence: same batch-and-wait loop, `run_admission_capacity.py:1437-1456`).

### H4 — tcp-tls asymmetries: no TLS session resumption + per-connection router rebuild

- Server config has no ticketer/session storage
  (`crates/peer-tls/src/lib.rs:184-200`), so every connection pays a full
  ECDHE + ECDSA + pinned-client-cert-verify handshake
  (`crates/atm-storage/src/tls.rs`, `PinnedClientVerifier`). Crypto is
  rustls+ring on both platforms (`Cargo.toml:46`), so raw crypto speed is
  comparable — the cost is per-connection count, which H3 multiplies.
- The authenticated path rebuilds `canonical_api_router(...)` + a fresh
  `ConcurrencyLimitLayer(128)` **per accepted connection**
  (`crates/atm-http-runtime/src/http1_server.rs:110-119`); the plaintext
  path builds its router once at startup
  (`crates/atm-http-runtime/src/runtime_setup.rs:17-25`).
- **Kill-test:** on Windows, tcp-tls (5,614) is already ~93% of tcp
  (6,012) — so TLS is *not* the dominant Windows cost today; fix H1/H2
  first. These items matter for the final push to 12.4k and are clean,
  bounded wins: add a rustls ticketer; hoist router construction out of
  the accept loop.

### H5 — Nagle server-side (small, cheap, do it while there)

- `set_nodelay` appears nowhere in the Rust codebase; the benchmark client
  sets TCP_NODELAY client-side only (`run_admission_capacity.py:1408`).
  Server 201-responses are small writes; Windows delayed-ACK/Nagle
  interaction differs from macOS.
- **Kill-test/fix:** `stream.set_nodelay(true)` on accepted sockets in the
  direct-peer accept paths (`crates/atm-http-runtime/src/lib.rs:556-579`,
  `http1_server.rs`); measure before/after on tcp f8.

### H6 — Host environment: Defender/AV, power plan, background load

- AO2.8 already defined the evidence contract: a typed `WindowsHostFacts`
  object (power plan, Defender state, exclusions absent, virtualization/WSL
  state, standard token) — `sprint-AO2-8-windows-tcp-benchmark-parity.md:132-138`.
- Defender real-time scanning of the SQLite db/WAL files could contribute
  to both the sqlite gap and variance.
- **Kill-test:** diagnostic-only run with Defender real-time protection
  temporarily off (never for official evidence). Record host facts for
  every diagnostic so environment deltas are attributable. The Aug-1-peak
  vs Aug-21+-plateau regression (§1.3) may be host-side: check Windows
  Update history, Defender platform updates, and power-plan changes on
  fastpc4 around 2026-08-01→2026-08-21, alongside a code-revision bisect.

## 4. Sprint plan

Ordering is deliberate: isolation first (evidence validity), then sqlite
(Rand's directive — it underlies the whole chain), then transport, then
re-baseline. WPERF.2 gates WPERF.3.

### WPERF.1 — Dedicated non-interactive Windows benchmark account

fastpc4 analogue of `m5-atmbench`. Official evidence to date came from the
operator's interactive account (`C:\Users\rand.lee\.atm`) — the exact gap
m5-atmbench closed on macOS. `bootstrap_benchmark_account()`'s safety check
is presence-of-durable-state, not identity
(`scripts/smoke/benchmark_account.py:297-347`), so isolation must come from
the OS account itself.

Tasks:
1. Create a local, non-admin, non-interactive account (suggested name
   `atmbench`) on fastpc4; enable SSH or scheduled-task access (no
   interactive desktop use). The account id is recorded via SID
   (`benchmark_account.py:62-78`) — no repo change needed.
2. Clone the repo under that account; provision the daemon-switch selector
   symlinks (`C:\atm-active\atm.exe`, `atm-daemon.exe`) per
   `.claude/skills/benchmark-run/SKILL.md:141-160` (needs Developer Mode
   or one elevated shell — one-time).
3. `just benchmark-bootstrap` from the new account (refuses pre-existing
   `.atm/db` state — expected clean).
4. One full campaign (`$env:ATM_CAPACITY_HOST_LABEL = 'windows-x64-01';
   just benchmark`) to prove the pipeline end-to-end; publish it whatever
   the numbers are (`just benchmark-publish`; evidence is evidence).
Acceptance: published campaign whose account home is the dedicated
account's; `benchmark-account.json` manifest recorded under its SID;
host-facts captured (H6).

### WPERF.2 — SQLite admission gap (root-cause and fix) — **gate for WPERF.3**

Work H1 + H2 (+ H6 diagnostics) using the isolated `sqlite` target binary
(`atm-daemon-benchmark` — storage only, the cleanest instrument).
Method: follow the AO2.8 remediation loop
(`sprint-AO2-8-windows-tcp-benchmark-parity.md:46-73`): quantify (batch
telemetry, commits/sec, profiler trace when wall-clock can't distinguish),
smallest justified production correction, regression test, full rerun.
Escalation rule: any durability-contract change (e.g. `synchronous`)
goes to Rand/team-lead with data before landing (H1 constraint).
Acceptance: Windows sqlite p50 ≥ **~36,100 msg/s** (85% of M5 accepted
median), or a profiler-backed attribution showing the residual is inherent
platform cost, escalated for an explicit product decision. No fix may
regress M5 numbers (standing rule).

### WPERF.3 — tcp / tcp-tls gap (after WPERF.2 gate)

Re-measure both transports against the corrected storage baseline — H1/H2
fixes may close much of this for free. Then work H5 (nodelay), H4
(ticketer, router hoist), and H3 diagnostics in that order (cheapest
first). Harness-model changes (H3 fix) are a separate coordinated PR, not
part of this sprint's product changes.
Acceptance: Windows tcp p50 ≥ **~15,000**, tcp-tls ≥ **~12,400 msg/s**
(85% of M5 accepted medians), same attribution/escalation fallback as
WPERF.2, no M5 regression.

### WPERF.4 — Variance root-cause + harness comparability

1. With H2's timer findings in hand, characterize run-to-run variance on
   the dedicated account: ≥5 consecutive campaigns, report per-target
   p50 spread. Target: consecutive-run spread within the 5% tolerance
   band used by the acceptance floors.
2. Fix the harness comparability traps regardless of variance outcome:
   clamp Windows client workers like macOS
   (`admission_connection_worker_limit`,
   `run_admission_capacity.py:1466-1484`), and surface interval-count in
   the report so variable sample sizes are visible.
3. Record explicitly whether the client-side batch-and-wait suspicion
   (AO2-tracked, `run_admission_capacity.py:99-102,1437-1456`) shares
   evidence with the Windows variance — flag, don't assume.

### WPERF.5 — Re-baseline Windows floors (3-clean-run standard)

Current `windows-x64-01` floors are all "historical migration seed;
pending quality review" (`baselines.json`). After WPERF.2–.4 land:
three clean, published official campaigns for the same contract
(`benchmark-run/SKILL.md:86-92`), then update `baselines.json` floors with
rationale. Raising floors is the normal ratchet; any lowering requires the
exact `"Rand via D3 ratchet exception"` approval string
(`benchmark_schema.py:167-190`). Close out GH #1030 and supply the
readiness disposition for `ATM-QA2-004`
(`docs/plans/phase-ao2/readiness.md:39-56`).

## 5. Acceptance targets (Rand, 2026-08-29: target ~85% of Mac numbers)

Method per AO2.8: expected Windows p50 = matching M5 accepted median ×
0.85; closure floor = expected × 0.95 (effective 80.75%). Using the three
accepted m5-atmbench campaigns (20260826T002410Z / 204109Z / 204324Z):

| Target | M5 median p50 | Windows target (×0.85) | Closure floor (×0.95) | Today | Needed gain |
|---|---:|---:|---:|---:|---:|
| sqlite | ~42,400 | ~36,000 | ~34,200 | 16,023 | ~2.2x |
| tcp | ~17,600 | ~14,900 | ~14,200 | 6,012 | ~2.5x |
| tcp-tls | ~14,600 | ~12,400 | ~11,800 | 5,614 | ~2.2x |

(uds: not applicable on Windows — three-target matrix,
`benchmark_schema.py:290-295`.)

## 6. Rules and constraints (binding for all sprints)

1. **Never patch the legacy synchronous daemon** — all daemon work targets
   the Tokio+Axum `atm-http-runtime` path (AGENTS.md hard rule; the
   benchmark already exercises only that path, §2).
2. **No threshold-lowering to force a pass** (Rand, 2026-08-26). Floors
   move on evidence via the ratchet rules only.
3. **Durability semantics are a product contract.** Reply-after-commit and
   the restart-durability check stay intact; `synchronous`/checkpoint
   changes require explicit sign-off (H1).
4. **Official evidence rules:** dedicated account only; no elevation,
   power-plan change, AV exclusion, or WSL to improve a number; publish
   every measured campaign including FAILs; campaigns are immutable
   (`benchmark-run/SKILL.md` §4–6).
5. **Harness changes are shared-contract changes:** separate PR,
   team-lead visibility, macOS impact and re-baselining stated up front.
6. **No M5 regression:** no fix may regress the achieved M5 numbers.
7. Dev work happens on a new `fix/1030-windows-perf` worktree branched
   from `develop`; PRs target `develop`; merge-commit only. This plan
   branch carries no implementation.
8. **PR report:** the dev PR must carry a complete report describing every
   change, root causes found (hypothesis → kill-test result → fix), and
   before/after benchmark evidence links, so the team can review without
   side-channel context.

## 7. Command appendix (fastpc4, PowerShell)

```powershell
# Preflight (dedicated account; see benchmark-run SKILL §1)
git status --short
python .claude/skills/daemon-switch/scripts/daemon-switch.py status --doctor
atm doctor --json
if (-not (Test-Path (Join-Path ($env:ATM_HOME ?? "$HOME\.atm") "benchmark-account.json"))) { just benchmark-bootstrap }

# Select the branch-built pair (never replace installed executables)
python .claude/skills/daemon-switch/scripts/daemon-switch.py switch `
  --cli-link C:\atm-active\atm.exe --daemon-link C:\atm-active\atm-daemon.exe `
  --cli target\release\atm.exe --daemon target\release\atm-daemon.exe --yes `
  --service atm-daemon

# Run the full three-target matrix (f8) and review
$env:ATM_CAPACITY_HOST_LABEL = 'windows-x64-01'
just benchmark
just benchmark-show

# Publish (measured campaigns always publish, PASS or FAIL)
just benchmark-publish
git commit -m "evidence(benchmark): <campaign-id> windows-x64-01"
git push

# Restore the installed release pair afterwards
python .claude/skills/daemon-switch/scripts/daemon-switch.py restore --yes --service atm-daemon
atm doctor --json

# Diagnostics
netstat -ano | findstr TIME_WAIT           # H3 churn watch
powercfg /getactivescheme                  # H6 host facts
Get-MpComputerStatus | Select RealTimeProtectionEnabled   # H6
```

## 8. QA history

| Round | Reviewer | Result | Notes |
|---|---|---|---|
| — | quality-mgr | pending | Initial review of this plan doc |
