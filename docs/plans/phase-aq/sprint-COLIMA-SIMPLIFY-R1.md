---
id: COLIMA-SIMPLIFY-R1
phase: AQ
sprint: COLIMA-SIMPLIFY-R1
title: Colima integration test simplification
status: complete
branch: plan/colima-simplify
worktree: /Users/randlee/Documents/github/atm-core-worktrees/plan/colima-simplify
target: develop
pr_target: develop
integration_branch: develop
execution_track: docs
parallel_with: [HERMES-PATCH-MODEL-R1]
stack_parent: none
one_command: "./run.sh --atm-ref prerelease/vX.Y.Z --hermes-ref <sha>"
target_minutes: 30
dependency_relations:
  - prerequisite: COLIMA-SIMPLIFY-R1
    dependent: HERMES-PATCH-MODEL-R1
    relation: parallel_safe
    rationale: This plan changes only testbed operation; the canonical Hermes patch design is a separate track.
---

# COLIMA-SIMPLIFY-R1 — Colima integration test simplification

## Goal

Replace the multi-party release exercise with one unattended command in
`randlee/atm-hermes-testbed`:

```sh
./run.sh --atm-ref prerelease/vX.Y.Z --hermes-ref <sha>
```

The command resolves immutable inputs, builds or reuses a native arm64 image,
starts a disposable container, runs deterministic tiers A–D and the release
prompt suite, queries the isolated ATM SQLite database, and writes exactly one
top-level `result.json` plus an evidence directory. It prints `PASS` or `FAIL`
and exits 0 or 1. The command owns setup, bounded waits, cleanup, and evidence
collection. There are zero ATM messages between invocation and verdict.

Any agent with Colima—including a low-cost or local coordinator—or a scheduled
job can run the command. No person is part of the execution path. A skipped,
missing, malformed, or timed-out default fixture makes the harness verdict
`FAIL`; it can never make a release look green.

## Hard Dependencies

- None. This planning sprint is independent of the v1.5.6 build and rollout.
  Implementation requires a machine with Colima, immutable ATM/Hermes refs,
  and a configured fast/local prompt backend. A provider-backed backend needs
  its harness-scoped credential; the local backend does not. It never requires
  a live ATM team or a second host.

## Dependency Relations

`HERMES-PATCH-MODEL-R1` is `parallel_safe`: it owns whether a named Hermes
release needs the canonical minimal patch or can use a public seam, while this
plan owns only the testbed runner. The future testbed PR relations are defined
under “Future testbed implementation sprints.”

## Exact Targets

- Add this authoritative sprint document.
- Add the linked sprint entry to `docs/project-plan.md`.
- Change no source, harness, runtime, requirement, ADR, or testbed file in this
  planning PR.

## Deliverables

- [x] This authoritative plan with one-command contract, measured baseline,
  fixture dispositions, provenance, roles, documentation cleanup, and small
  implementation sprints.
- [x] `docs/project-plan.md` entry linking this plan.

Every listed deliverable lands complete in this documentation sprint. Testbed
implementation is explicitly assigned to later, separately documented sprints.

## Required Work

### Measured baseline and budget

The 2026-09-07 v1.5.3 evidence is under
`atm-hermes-testbed/results-run-v153` on testbed evidence commit `e5cb983`.
Its final report recorded a 13m59s command envelope (04:37:15–04:51:14 UTC).
The individual evidence files account for the work as follows:

| Stage | Measured baseline | Evidence and interpretation | Target/fix |
| --- | ---: | --- | --- |
| Startup/preflight | 0m37s | Outer start to `tier-a.json.started_at` (04:37:52) | ≤2m; bound every probe and fail once |
| A–D matrix | 0m59.970s | Sum of `duration_ms` in `tier-{a,b,c,d}.json`: 3.116s, 45.054s, 2.468s, 9.332s | ≤3m; retain deterministic scripts |
| Prompt execution | 10m16s observed span | `prompt-AT1.json.started_at` 04:39:00 through `prompt-AT8.json.finished_at` 04:49:16; measured test costs include AT1 4m09s and AT2 2m20s | ≤15m; use fast/local agents, bounded child processes, and deterministic fixtures |
| Evidence/verdict tail | 1m58s | Last prompt timestamp to outer completion | ≤2m; aggregate locally once |
| Image build/download | **Not measured** | `asset-provenance.txt` identifies artifacts and image digest but records no stage start/end; its 02:34:58 timestamp is not a build duration | Instrument in `result.json`; warm cache ≤3m, cold path ≤10m |

The JSON-visible interval from `tier-a.json.started_at` (04:37:52) to
`prompt-AT8.json.finished_at` (04:49:16) is 11m24s. The difference from the
13m59s outer envelope is command startup and final aggregation. The A–D files
show 27/27 deterministic checks passed. The prompt files show AT1 failed and
AT3/AT4/AT8 skipped, so the correct overall verdict was `FAIL` even though no
ATM product defect was demonstrated.

The required cold command-to-verdict target is **≤30 minutes**; the warm-cache
goal is **≤20 minutes**. The separate hard safety ceiling is 60 minutes, after
which the harness terminates its children, writes `FAIL`, and cleans up. The
implementation records elapsed milliseconds for
resolution, download, image build/cache lookup, startup, matrix, prompts, DB
queries, evidence write, and total. A target miss is reported as a diagnostic
`target_met` value and optimization issue; exceeding 30 minutes never changes
the product `PASS`/`FAIL` verdict. The 60-minute ceiling is a stuck-harness
failure, not a performance assertion.

Concrete execution choices:

- Native `linux/arm64` is the release path on Apple Silicon. The evidence
  already records `host.emulation = native`, and CI publishes the aarch64 ATM
  archive and manylinux wheel. `amd64`/qemu is an opt-in diagnostic only; its
  wall clock is recorded and never compared with a product threshold.
- Cache a content-addressed Hermes base image by named Hermes release SHA and
  build only the thin testbed layer when its inputs change. A cache miss is
  automatic and remains inside the 30-minute cold budget.
- Resolve the ATM tag to one commit, download its archive and wheels from the
  matching successful CI runs, verify published and locally computed hashes,
  then install those prebuilt artifacts. Do not compile ATM inside the
  container. The current evidence lacks download/build timings, so the first
  wrapper PR adds them before further optimization.

### Product assertions and fixture disposition

Assertions are limited to public exit codes, sanitized `atm doctor --json`,
and read-only queries of durable rows belonging to this run's generated
fixture IDs. `docs/requirements.md` §§7.12–7.14 govern the oracle: JSON message
IDs must agree; read acceptance is not immediate durable visibility; callers
poll `atm list --json` to a bounded deadline; timeouts fail explicitly; and
file/template preflight must fail before partial persistence. SQLite checks
confirm durable message/state rows and exactly-once outcomes. They never print
message bodies, `graft_receiver_endpoints` values, tokens, chat IDs, or
Telegram IDs. Internal log serialization is diagnostic only.

| Yesterday's drift or missing fixture | Disposition in the default gate |
| --- | --- |
| E0, AT0, AT2, and AT5–AT7 passed without identified drift | **Retain as required.** Run them through the same bounded child interface and product-observable oracle; any skip, timeout, malformed report, or nonzero child exit fails the harness. |
| AT1 second team/member was never registered | **Fix.** The runner creates both isolated rosters, proves them through the CLI, and removes them during automatic cleanup. |
| AT1 required an exact phrase in help text | **Drop.** Assert the documented exit/result behavior; retain help text only as an artifact for humans. |
| AT3 peer was absent | **Move to diagnostics-only.** Cross-host trust mutates a second machine and cannot satisfy the one-host unattended contract. It is not invoked or skipped by the default command. |
| AT4 marker directory/lifecycle hook was absent | **Fix.** Create and permission the marker directory before the agent starts; the runner owns the restart hook and bounded synchronization. |
| AT8 marker/calibration setup was absent | **Fix the fixture, narrow the oracle.** The runner installs and validates its fault-injection helper before agent launch; the gate asserts exit/result truth and exactly-one durable DB row. RTT and delay remain diagnostics. |
| B2a/B2b/B2c copied nudge templates drifted | **Fix.** Generate expected envelopes from the product-owned renderer/schema and assert semantic fields, not a second handwritten template. |
| D7 parsed a private daemon log shape | **Fix.** Correlate the CLI message ID with the durable DB row and recipient state; keep logs only in the evidence directory. |
| A1 assumed synchronous read-state visibility | **Fix.** Assert `mutation_applied` acceptance, then bounded-poll the list/DB projection as required by §§7.12–7.13. |

Default-gate fixtures declare `required: true`: all 27 A–D rows plus E0, AT0,
AT1, AT2, AT4, AT5, AT6, AT7, and AT8. The manifest, rather than a handwritten
expected count in the aggregator, is authoritative. The aggregator fails
closed if any required fixture is absent or returns `skip`. AT3 and any qemu
probe declare `classification: diagnostic`; optional diagnostics cannot
contribute a passing count.

### Provenance without coordination

The runner records what it actually used: requested ATM tag, resolved ATM SHA,
Hermes release/SHA, testbed SHA and dirty-state refusal, CI run IDs, archive and
wheel SHA-256 values, base/testbed image digests, architecture/emulation, suite
manifest hash, timestamps, stage durations, and the exact command arguments.
Mutable local checkouts are never a provenance source, and evidence from an
earlier container or result directory is rejected.

There is no daily fork rebase. Hermes integration uses either zero fork patch
through a public plugin/API seam or one minimal canonical patch for each named
Hermes release. The selected Hermes target is resolved once by the command and
recorded, not stabilized through messages. `HERMES-PATCH-MODEL-R1` owns that
design; this sprint neither redesigns nor modifies the fork. Fenix and the
original designer agree the patch is as small as possible; Loki maintains the
accepted patch but does not redesign it. ATM defects become ordinary issues or
PRs owned by arch-ctm. Rand alone makes publication decisions.

The prompt child receives a harness-scoped secret and maps it to the provider's
well-known variable only in that child process. No secret is written into an
image, file, result, log, or container-wide environment.

### Procedure and roles

1. Any runner with Colima invokes the one command with immutable ATM and Hermes
   refs. It does not send progress messages or request decisions.
2. The command returns `PASS`/0 or `FAIL`/1 and leaves `result.json` plus its
   referenced evidence directory. Cleanup runs on both paths.
3. The runner posts `result.json` unchanged to the release PR or issue and
   sends one ATM line to the requester containing the verdict and durable URL.
4. On `FAIL`, automation opens or updates one ordinary issue with the JSON and
   evidence attached. Diagnosis happens after the verdict; the failed command
   is not resumed. Product fixes and harness fixes use normal PRs, then a new
   clean invocation produces a new result directory.

The command must be suitable for a scheduled job. It takes no coordinator
identity, has no interactive prompts, enforces the 60-minute safety timeout,
and leaves no fixture teams, containers, ports, or host trust changes behind.

### Documentation replacement

- Close or supersede atm-core PR #1276 rather than merging its
  `docs/runbooks/hermes-graft-colima-integration.md`; if the file lands first,
  delete it when this runner ships. Remove its P0–P3 sequencing, handoffs,
  intermediate status reporting, and mutable commands.
- Replace testbed `SMOKE-TEST-RUNBOOK.md` with the four-step procedure above
  plus the one-command options/result schema. Delete its step-by-step build,
  boot, per-test execution, evidence assembly, co-sign, and host-coordination
  instructions.
- Delete testbed `COLIMA-VM-INTERNAL-PLAN.md`. Preserve only still-valid
  isolation and named-peer facts in the diagnostics documentation; remove its
  checklist, ownership, sequencing, held host state, and approval history.
- Rewrite the testbed README's obsolete phases, amd64 default, manual
  allowlist, and owner/status narrative around the one-command contract.

### Future testbed implementation sprints

Each row becomes its own mandatory-frontmatter sprint file when dispatched.
Each PR targets the then-current testbed `main`, has no dependency on another
open PR, and leaves existing entry points usable until replaced. The first PR
is intentionally a thin wrapper so unattended use arrives before internal
cleanup. No stack is required for these sequentially rebased, disjoint PRs; if
a later split introduces a linear open-PR dependency, manage that chain with
the `/gh-stack` skill.

| Sprint/PR | Owned paths and independently mergeable scope | Required validation |
| --- | --- | --- |
| CS.1 single-command entry point | `run.sh`, new `harness/aggregate-result.*`, and runner tests: accept refs, call today's scripts, enforce child/60-minute timeouts, emit the complete aggregate schema below, and exit 0/1. | Shell tests for args, timeout, skip→fail, child-exit propagation, fresh result directory. |
| CS.2 immutable assets and cache | `build.sh`, asset resolver/cache helper, and its tests: resolve tag/SHA and matching CI artifacts, verify hashes, use native arm64, and key base/thin-layer caches by content. | Cold and warm dry fixtures; cache-key and provenance mismatch tests. |
| CS.3 deterministic product oracle | `testbed/test-tier-{a,b,d}.py`, shared result/DB-query helper, and tests: repair B2a–c, D7, and A1 without changing the aggregate schema. | Fixture tests plus A–D matrix on a disposable container. |
| CS.4 self-contained prompt fixtures | `harness/run-prompts.sh`, prompt manifests/hooks, and tests: repair AT1/AT4/AT8, isolate child secrets, default to a fast/local backend, classify AT3 diagnostic, and fail every required skip. | Offline fake-agent suite and one real-provider prompt run. |
| CS.5 unattended operation and docs | Scheduled workflow/poster, cleanup/residue tests, `README.md`, concise runbook, and deletion of the ceremonial documents named above. | Cron-like no-TTY run; forced failure proves one JSON, issue payload, and clean teardown. |

CS.1 may merge first without waiting for any other row. CS.2–CS.4 touch
separate asset, oracle, and prompt-fixture surfaces and can proceed in
parallel after CS.1 establishes the aggregate schema. CS.5 follows the stable
CLI/schema but can prepare its docs and automation in parallel.

## Explicit Code Samples

The future runner's stable entry point and aggregate result shape are:

```sh
./run.sh --atm-ref prerelease/vX.Y.Z --hermes-ref <sha>
```

```json
{
  "schema": "colima-result-1",
  "verdict": "pass",
  "exit_code": 0,
  "started_at": "<UTC timestamp>",
  "finished_at": "<UTC timestamp>",
  "duration_ms": 839000,
  "target_minutes": 30,
  "target_met": true,
  "safety_timeout_minutes": 60,
  "provenance": {
    "atm_ref": "prerelease/vX.Y.Z",
    "atm_sha": "<sha>",
    "hermes_sha": "<sha>",
    "testbed_sha": "<sha>",
    "suite_manifest_sha256": "<sha256>",
    "ci_run_ids": [0],
    "archive_sha256": "<sha256>",
    "wheel_sha256": {"hermes_atm": "<sha256>", "atm_graft": "<sha256>"},
    "image_digest": "sha256:<digest>",
    "platform": "linux/arm64",
    "emulation": "native"
  },
  "stages": [{"name": "matrix", "duration_ms": 59970}],
  "fixtures": {
    "manifest_sha256": "<sha256>",
    "counts": {"required": 36, "pass": 36, "fail": 0, "skip": 0},
    "results": [
      {"id": "A1", "classification": "required", "verdict": "pass", "evidence": "evidence/<run-id>/A1.json"}
    ]
  },
  "evidence_dir": "evidence/<run-id>"
}
```

Every field shown is required. Each manifest fixture has one `results` entry;
the abbreviated array above illustrates its fixed row shape. Failed rows add a
sanitized `reason`; no result contains a secret or receiver endpoint. Stdout
ends in exactly one verdict line; `result.json` is the sole machine-readable
summary and links subordinate evidence artifacts.

## This Sprint Does Not Close

- It does not change atm-core code, the legacy synchronous daemon, Hermes
  agent code, the canonical Hermes patch, or release policy.
- It does not make the cross-host AT3 diagnostic a release gate.
- It does not claim that the unmeasured image-build duration already meets a
  target; CS.1 measures it and CS.2 optimizes it.
- It does not implement the future testbed PRs listed above.

## Acceptance Criteria

- [x] The default operation is one command, one aggregate JSON, one terminal
  verdict, and no execution-time ATM traffic.
- [x] The 30-minute budget and all available/missing baseline measurements are
  explicit and tied to the v1.5.3 evidence.
- [x] Every known failed or drifted fixture has a fix/drop/diagnostic
  disposition, and required skips fail closed.
- [x] Provenance is derived from resolved artifacts and runtime observations,
  never mutable checkout state or messaging.
- [x] The operator procedure fits on one screen and has no human gate.
- [x] At 2,500 words or fewer (five 500-word rendered pages), this plan stays
  within the six-page limit.
- [x] This PR changes documentation only.

## Required Validation

```sh
git diff --check origin/develop...HEAD
rg -n '^---$|^id: COLIMA-SIMPLIFY-R1$|^status: complete$|^branch: plan/colima-simplify$|^worktree: |^target: develop$|^one_command: |^target_minutes: 30$' \
  docs/plans/phase-aq/sprint-COLIMA-SIMPLIFY-R1.md
rg -n 'COLIMA-SIMPLIFY-R1' docs/project-plan.md \
  docs/plans/phase-aq/sprint-COLIMA-SIMPLIFY-R1.md
test "$(wc -w < docs/plans/phase-aq/sprint-COLIMA-SIMPLIFY-R1.md)" -le 2500
git diff --name-only origin/develop...HEAD | grep -Ev '^(docs/|$)' && exit 1 || true
```
