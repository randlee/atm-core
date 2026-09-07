# Hermes graft Colima integration runbook

Status: in progress for `HERMES-GRAFT-COLIMA-R1-1788742538`.

This runbook coordinates a full frozen-delta review of the Hermes agent fork
and an isolated Colima integration run against an atm-core release candidate.
It is written so a lower-cost coordinator can repeat the process without
depending on chat history.

## Safety invariants

- Never review a moving fork target. P1 starts only after Loki supplies the
  agreed completion signal, base SHA, and immutable head SHA.
- Never publish atm-core or its packages. Report integration readiness to
  Fenix; Rand owns the publish decision.
- Never patch atm-core product code in this workflow. Report the failing test,
  root cause, and minimal product-fix scope to Fenix for separate dispatch.
- Do not use `sudo`. Do not initiate ssh from rand-m4. Do not control a Hermes
  agent through tmux. Fixture daemons remain loopback-only except for an
  explicitly approved isolated cross-host leg.
- Never persist message bodies, live participant addresses, tokens, secret
  values, or capability values in docs, JSON results, commits, or ATM reports.
- Treat QEMU wall-clock timings as diagnostics only, never benchmark evidence.
- A rerun does not erase a failure. Root-cause every failure and intermittent
  result; flaky tolerance is zero.

## Roles and addressing

| Role | Responsibility |
| --- | --- |
| `loki@hermes` | Freeze and identify the fork range; own the testbed; execute or co-execute Hermes-side steps; validate results. |
| `solar@atm-dev` | Review the full fork delta; coordinate the matrix; maintain this runbook; report phase status to Fenix. |
| `fenix@atm-dev` | Coordinate ATM-side decisions and QA; dispatch separate atm-core fixes; forward only genuine waiver/publish decisions to Rand. |
| Rand | Waive blocking review findings and decide whether to publish. |

Use `loki@hermes` exactly for the local Hermes-team contact. For cross-host ATM
routing, use the host-qualified form `<agent>@<team>.<host>` supplied for the
isolated fixture. Do not substitute an unqualified name or `localhost` for a
cross-host trust identity.

## P0 — freeze contract and matrix handshake

Send one message to `loki@hermes` requesting all fields below. Do not infer a
missing value from an earlier testbed cycle.

```text
fork_repo:
fork_branch:
base_sha:
frozen_head_sha:
completion_signal:
testbed_repo_sha:
image_name_and_tag:
container_name:
atm_version:
atm_tarball_source_and_builder:
hermes_atm_wheel_source_and_builder:
atm_graft_wheel_source_and_builder:
tiers:
cross_host_in_scope:
fork_validation_commands:
```

The completion signal is valid only when it includes the exact branch, base,
head, and the statement that the range will not move during review. Save only
these coordination facts; do not copy message content or credentials into the
repository.

P0 exit gate:

- every field above is resolved;
- the target is annotated tag `prerelease/v1.5.3` at the agreed `develop`
  commit;
- artifact provenance identifies who built each input and from which commit;
- Loki and Solar agree on one tier list and the cross-host disposition.

### P0 record for the current cycle

- Initial Hermes review head was `3ffa69d9070400fb0528f8dc75afa8edfa2691ff`;
  the accepted post-fix range is
  `693641aa8b4359c602283bdbbc14041e03bc47bc..d45230aac599d3db6a24404eba4fa2f04e030004`.
- atm-core tag: `prerelease/v1.5.3` at
  `9654b75f1710d7155fb3a6584ee709314c1642d5`.
- Archive run `34074022098` passed; wheel run `34074006538` is bound to
  the same commit.
- Native aarch64 ATM archive SHA-256:
  `ee00a73aee785f47c125d4b31ac50056c39f7012ae76e76da3b9d7dd81ed7905`.
- `hermes-atm` wheel SHA-256:
  `2892e8490053b7158e9c87bb72838955390200e5247d940d971bc79c267a7318`.
- aarch64 `atm-graft` wheel SHA-256:
  `880eef4e90e04a16bf5bfa0e8b8a2d047b59f76183076a4b7336a99cfd0f1203`.
- Tagged source and built wheel METADATA both admit `atm-graft` 1.5.3.
- P2 executes from testbed main
  `b468321`; the peer authority, 4 CPU / 4 GiB Colima
  allocation, `suite/v2`, no-`sudo` marker protocol, and primary matrix are
  frozen.

## P1 — full review of the frozen Hermes fork

### Prepare without mutating the target

Use a dedicated read-only review checkout. Replace placeholders only with the
values Loki acknowledged in P0.

```sh
cd /Users/randlee/Documents/github/hermes-agent-randlee
git fetch --all --prune
git cat-file -e <base-sha>^{commit}
git cat-file -e <frozen-head-sha>^{commit}
test "$(git rev-parse origin/<fork-branch>)" = "<frozen-head-sha>"
git merge-base --is-ancestor <base-sha> <frozen-head-sha>
git diff --name-status <base-sha>..<frozen-head-sha>
git log --reverse --oneline <base-sha>..<frozen-head-sha>
```

Record the changed-file inventory before reviewing. Review every changed file
and every commit in the range. Re-run the head-equality check immediately
before publishing the report; if it changed, stop and obtain a new freeze
signal.

### Review focus

- atm-graft receiver registration, refresh, lookup, and restart recovery;
- queue versus steer kind preservation and nudge-template rendering;
- task-state/ack transitions introduced by atm-core Phase AX;
- host-qualified addressing and cross-host trust boundaries;
- capability/token handling, log redaction, and isolation walls;
- shutdown, cancellation, retry, deadline, and orphan-process behavior;
- packaging/version compatibility for atm `prerelease/v1.5.3`, `hermes-atm`, and
  `atm_graft`;
- test coverage for each new branch and failure mode.

Run exactly the fork validation commands Loki provides. Do not silently replace
or omit a failing command.

Current-cycle review is recorded on Hermes PR #21: initial findings comment
`issuecomment-5563939096` and passing delta-review comment
`issuecomment-5564128952`. HGF-001 through HGF-004 are fixed and independently
verified at `5f2793add0f949bbc9b231e0e18a17bfb39b2f93`: 38 tests, Ruff, diff check,
and the production-shaped steer probe pass. HGF-005 is fixed at `650bc7f2d5`.
The final `origin/main` and `origin/atm/stack` both point to `d45230aac5`; the
full five-file range is clean, P1 is PASS, and P2 is released.

### Review report shape

Post the report on Loki's named Hermes fork issue or PR:

```text
SUMMARY
<scope, base..head, overall verdict>

FINDINGS
HGA-001 — Blocking|Important|Minor — path:line
Finding: <observable defect or risk>
Recommendation: <concrete correction>

RISKS
- graft receiver compatibility:
- nudge templates and queue/steer kinds:
- task-state behavior:
- cross-host trust:

TEST-GAPS
- <missing deterministic or integration coverage>
```

P1 exit gate: no unresolved blocking finding, unless Rand explicitly waives it
in a durable decision referenced by the report.

## P2 — isolated Colima integration

### Provenance and isolation preflight

Work from `/Users/randlee/Documents/github/atm-hermes-testbed`. Confirm the
testbed checkout is at Loki's acknowledged SHA and clean before execution.

```sh
cd /Users/randlee/Documents/github/atm-hermes-testbed
git fetch --all --prune
test "$(git rev-parse HEAD)" = "<testbed-sha>"
test -z "$(git status --porcelain)"
colima status
docker info
test -f env/allowlist.env
test -z "$(git ls-files env/allowlist.env)"
shasum -a 256 <atm-tarball>
find <wheels-dir> -maxdepth 1 -type f -name '*.whl' -print
python3 /Users/randlee/Documents/github/atm-core/.just/check_version_sync.py
rg -n 'atm-graft' /Users/randlee/Documents/github/atm-core/crates/hermes-atm/pyproject.toml
unzip -p <hermes-atm-wheel> '*.dist-info/METADATA' | rg '^Requires-Dist: atm-graft'
```

Do not print the allowlist contents. Record only artifact filenames, source
commits/run IDs, SHA-256 digests, and image digest. Every integration cycle
gets its own unused patch version and tag. Before a version is selected, run:

```sh
git -C /Users/randlee/Documents/github/atm-core fetch --tags origin
git -C /Users/randlee/Documents/github/atm-core tag -l 'prerelease/*'
```

Integration/readiness builds use `prerelease/vX.Y.Z`; `vX.Y.Z` is reserved for
production releases. Never test an untagged version. This cycle's target is
`prerelease/v1.5.3`; pin its exact tag SHA and artifacts from team-lead's
dispatch before building. Both the source constraint and the built
`hermes-atm` wheel METADATA must admit the same tagged `atm_graft` workspace
version. A mismatch stops the run before image construction.

PR #1274 (`chore/release-1.5.3`) introduces the permanent same-tag wheel-pair
control in `.just/check_version_sync.py`. Its fix commit `11be31b93` changes the
`hermes-atm` constraint to `atm-graft>=1.5,<1.6`; `993acd0a7` bumps the workspace
and Python packages to 1.5.3. PR #1274 merged to `develop` at `9654b75f1`, and
the annotated `prerelease/v1.5.3` tag resolves to
`9654b75f1710d7155fb3a6584ee709314c1642d5`. Archive run `34074022098` is the
only artifact source for this cycle. Wait for that run to complete and verify
the tagged source and built wheel METADATA before image construction.

### Build and boot

Use the platform, image/tag, and artifact locations agreed in P0. The approved
Colima allocation is 4 CPU and 4 GiB. Do not reuse an unproven wheelhouse or a
mutable image tag.

```sh
TESTBED_PLATFORM=<platform> \
ATM_TARBALL=<atm-tarball> \
WHEELS_DIR=<wheels-dir> \
./build.sh all

TESTBED_PLATFORM=<platform> ./run.sh
docker exec <container> atm doctor
```

Current-cycle build record:

- Discarded image digest prefix `670edd12` is invalid evidence. A first-match
  glob selected a stale 1.4.6 archive despite the explicit 1.5.3 override.
- Testbed commit `b468321` makes override selection exact and purges stale
  tarballs. The rebuilt container reports ATM, `hermes-atm`, and `atm-graft`
  1.5.3 and passes `atm doctor`.
- The only valid image is `loki/hermes-testbed:testbed` at
  `sha256:5d46dfbc0f90460306673bf624013f11c4402ce7b48f91fd23f87eb1fbe8b9ee`.
  Its baked artifact digests match the P0 record.

If the harness attempts `sudo`, stop. The no-`sudo` implementation landed by
`fb9d63c`, building on `41fc546`. Daemon lifecycle control stays outside the
unprivileged agent, and the outer coordinator invokes root-only,
non-self-elevating helpers through root-default
`docker exec <container> <helper>`. The image contains no `sudo` package or
sudoers entries. Clear all coordination markers before every run. The fixture
agent performs and observes only ATM operations while UTC ready, trigger, and
done markers coordinate the out-of-band helper.

For AT4, the coordinator arms the restart helper, the fixture agent writes the
ready marker, and the helper restarts the daemon while the agent session remains
live. For AT8 Phase B, calibrate `--after` from a measured send RTT, clamp the
result to 300--1500 ms, and record it in `at8-armed`. The agent writes the
trigger immediately before sending; the helper then applies SIGSTOP/SIGCONT.
The daemon log must prove persistence happened before SIGSTOP. Otherwise record
`FAIL: freeze preceded persist`; never tune and rerun until green. AT8 step 1
must capture the same structural no-`sudo` evidence as AT4.

The authoritative operating procedure remains the testbed repository's
`SMOKE-TEST-RUNBOOK.md`; this readiness record intentionally does not duplicate
its mutable command sequence. The approved prompt changes and their CATALOG
changelog ship as `suite/v2` for the `prerelease/v1.5.3` rerun.

### Tier matrix

| Tier | Required flow | Command | Expected durable result |
| --- | --- | --- | --- |
| A | mailbox semantics | `docker exec <container> /opt/testbed/test-graph.sh` | `tier-a.json`, all named rows PASS |
| B | daemon → graft receiver → Hermes injection seam and envelope fidelity | same matrix command | `tier-b.json`, all named rows PASS |
| C | retained tmux fixture surface only; never manipulate a Hermes agent through tmux | same matrix command | `tier-c.json`, all named rows PASS |
| D | Herdr fixture surface | same matrix command | `tier-d.json`, all named rows PASS |
| D7 | ATM → Herdr nudge routing contract | same matrix command | D7 row PASS; no `ATM_HERDR_UNAVAILABLE` |
| Restart | daemon/receiver both orders plus crash-within-window recovery | use the P0-agreed restart command | separate restart JSON; zero manual profile repair |
| E | live graft-Hermes transcript | `docker exec <container> /opt/testbed/harness/run-prompts.sh E0` or the P0-agreed successor | prompt JSON with every step PASS |
| Cross-host | container ↔ Mac both directions | only when P0 says in scope | dedicated JSON with host-qualified routing and both directions PASS |

The matrix command runs A–D together; treat each emitted JSON file as its own
tier result. Validate each file as JSON and inspect every row. A process exit 0
does not override a failed or skipped required row.

The current testbed catalog describes E1 as E0's acceptance shape but has no
separate E1 prompt file. P0 must decide whether E0 is the Tier E live transcript
or Loki will add a distinct E1 artifact before the run.

### Result handling

Copy results out of the container into a new versioned directory without
overwriting prior evidence:

```sh
mkdir -p results-run-v<atm-version>-<utc-stamp>
docker cp <container>:/opt/testbed/results/. \
  results-run-v<atm-version>-<utc-stamp>/
python3 -m json.tool results-run-v<atm-version>-<utc-stamp>/tier-a.json >/dev/null
python3 -m json.tool results-run-v<atm-version>-<utc-stamp>/tier-b.json >/dev/null
python3 -m json.tool results-run-v<atm-version>-<utc-stamp>/tier-c.json >/dev/null
python3 -m json.tool results-run-v<atm-version>-<utc-stamp>/tier-d.json >/dev/null
```

Add the restart, Tier E, and optional cross-host JSON checks using their
P0-agreed filenames. Commit per-tier JSON and artifact provenance in the
testbed repository. Do not commit the environment allowlist or raw transcripts
containing prohibited data.

Every FAIL record must include:

```json
{
  "tier": "<id>",
  "verdict": "fail",
  "root_cause": "<sanitized technical cause>",
  "reproducible": true,
  "fix_owner": "loki|fenix-dispatch|external",
  "rerun_disposition": "not_run|confirm_fix_only"
}
```

## Cross-host rules

Run this leg only if P0 explicitly includes it.

- No ssh may originate from rand-m4.
- The peer authority and advertised host are `atm-hermes-testbed.local`, never
  `localhost`; `setup-mtls.sh` uses it as the certificate CN/SAN and
  `setup-peer.sh` advertises it.
- The host trust entry maps that authority to the container fingerprint and
  published port 43102. The host `/etc/hosts` entry resolves it to the Colima
  VM IP.
- Use `<agent>@<team>.<host>` for every cross-host ATM recipient.
- Fixture identities use the `fx-` prefix and never reuse live fleet names.
- Trust configuration is bootstrap-cached; an authorized owner must restart
  the affected daemon after a pin change before diagnosing the route.
- Record sanitized IDs and verdicts only, never message bodies, fingerprints,
  certificates, addresses, or capability values.

### Fixture roster teardown

Roster removal is caller-team scoped. A cross-team caller must be rejected;
that rejection is the expected authorization boundary, not a reason to weaken
the command. Run teardown as the fixture member itself or as another member of
the fixture team:

```sh
ATM_IDENTITY=<fixture-member> ATM_TEAM=<fixture-team> \
  atm teams remove-member <fixture-team> <fixture-member>
```

Expected result: the command succeeds once, and a sanitized roster/list check
shows the fixture member absent. With a caller from another team, expect an
error stating that the caller team does not match the target team; change the
caller identity, not the authorization policy.

## P3 — issue log

Every issue becomes a row before work continues.

| ID | Phase | Symptom | Cause | Recovery / durable rule | Status |
| --- | --- | --- | --- | --- | --- |
| HGC-001 | setup | `sc-git-worktree` was not on PATH | The repository exposes it as a command/skill backed by a Python helper, not a shell binary | Use the repository worktree-create delegate/helper; preserve tracking and protected-branch checks | resolved |
| HGC-002 | setup | New docs branch initially pointed at `98661ea18`, while fetched `origin/develop` was `89bd7d256` | The create helper fetched remotes but based the worktree on stale local `develop` | Before editing, compare `HEAD` to `origin/develop` and fast-forward the new feature branch | resolved |
| HGC-003 | P0 | Existing testbed docs describe older ATM/testbed cycles | README/runbook history predates the current fork update | Treat the frozen P0 record above as the cycle authority; never reuse old version/run IDs | resolved |
| HGC-004 | P0/P2 | Existing prompt harness documents privileged restart helpers, while this task forbids `sudo` | Safety contract changed for this run | Use P2 testbed SHA `8661a3e77` (doc-only after `e8e500a`; no-`sudo` changes complete by `fb9d63c`, initial implementation `41fc546`): root-only, non-self-elevating helpers run through outer `docker exec`; the fixture agent uses cleared UTC markers and never invokes sudo. Fenix approved the corresponding AT4/AT8 prompt design | resolved; suite/v2 rerun active |
| HGC-005 | P0/P2 | Tier E catalog names E1 but only an E0 prompt file exists | E1 is described as E0's acceptance shape, not an independent artifact | Use E0 as the required Tier E live graft-Hermes transcript for this matrix | resolved; execution pending |
| HGC-006 | P0 | Tagged `hermes-atm` and `atm_graft` 1.5.x wheels cannot co-install | `crates/hermes-atm/pyproject.toml` requires `atm-graft>=1.4,<1.5` at v1.5.0, `prerelease/v1.5.1`, and `origin/develop@89bd7d256` | PR #1274 fixes the range to `>=1.5,<1.6` and adds `.just/check_version_sync.py` as the permanent guard. Tag `prerelease/v1.5.3` is pinned to `9654b75f1710d7155fb3a6584ee709314c1642d5`; source and built wheel METADATA both admit `atm-graft` 1.5.3 | resolved |
| HGC-007 | P0 | An ATM coordination send returned `ATM_DAEMON_MAY_HAVE_EXECUTED` | The client could not prove whether the daemon committed the send | Check durable message/log state before retrying; resend only when the write is confirmed absent, preventing duplicate coordination messages | resolved by Loki |
| HGC-008 | P0 | A coordination message delivered only a local file reference instead of its intended body | Bare-file-reference delivery path did not carry the usable coordination text | Sender resends the full inline text and marks the file-reference message superseded; receiver acknowledges but does not open an unexpected external path | resolved by Loki/Solar |
| HGC-009 | P0/P2 | A fixed AT8 Phase-B delay can freeze before persistence or become timing-dependent | Host and VM scheduling make the send boundary variable | Calibrate from measured send RTT, clamp `--after` to 300--1500 ms, record it in `at8-armed`, and require log proof that persistence preceded SIGSTOP; otherwise fail without tuning/retry | approved; suite/v2 rerun pending |
| HGC-010 | P0/P2 | Loopback or an ambiguous peer name would invalidate cross-host trust evidence | The transport must distinguish the container peer from host-local routing | Use `atm-hermes-testbed.local` for CN/SAN and advertised host, map host trust to the container fingerprint and port 43102, and resolve the authority to the Colima VM IP | resolved by Rand ruling; execution pending |
| HGC-011 | P0 | Native sends against a host 1.4.13 daemon intermittently returned `ATM_DAEMON_MAY_HAVE_EXECUTED` while durable truth showed no write | Client could not confirm whether an older daemon committed the request | Treat as non-blocking diagnostic evidence only; verify durable truth before one retry and do not remodel or patch the frozen legacy daemon | observed; no product action |
| HGC-012 / HGF-001 | P1 | Real `mode="steer"` falls back to queue | Fork defect in `randlee/hermes-agent`: production stores the direct agent in `SessionState.turn.agent`, but the fork seam unwraps only a tuple; fork tests manufacture the obsolete tuple shape | Fork commit `5f2793add0` accepts direct-agent state, preserves sentinel/legacy behavior, and uses production-shaped tests. No atm-core product change | resolved in final fork tip `d45230aac5` |
| HGC-013 / HGF-002 | P1 | A hung visible notice prevents the main internal event from routing | Fork defect in `randlee/hermes-agent`: the soft-fail notice send has no deadline | Fork commit `5f2793add0` adds a 10-second bound and proves a hung notice warns while the event routes exactly once. No atm-core product change | resolved in final fork tip `d45230aac5` |
| HGC-014 / HGF-003 | P1 | The documented frozen test procedure fails with `No module named pytest` | Fork documentation defect in `randlee/hermes-agent`: it installs with `--no-dev`, while pytest is in the `dev` extra | Fork commit `5f2793add0` installs locked `messaging` and `dev` extras; the corrected 38-test command passes. No atm-core product change | resolved |
| HGC-015 / HGF-004 | P1 | Startup-hook test cannot detect loss of `gateway_runner` at the real emit site | Fork test gap in `randlee/hermes-agent`: it manually calls an `AsyncMock` with the expected payload | Fork commit `5f2793add0` drives the production startup mixin and asserts the emitted runner. No atm-core product change | resolved |
| HGC-016 / HGF-005 | P1 | Final full-range `git diff --check` reported five whitespace errors | Fork test formatting predated the delta-only review, so the first fix round did not touch it | Fork commit `650bc7f2d5` removes only the five trailing spaces; PR #23 merged, both fork pointers advanced to `d45230aac5`, 38 tests/Ruff/full-range diff-check pass. No atm-core product change | resolved |
| HGC-017 | P0/P2 | A stale host-side fixture roster row remained from the prior AT3 cycle | Cross-host fixture cleanup had awaited an ownership ruling | Rand authorized removal; Loki removed it and verified the host held-state list is empty before the new run | resolved |
| HGC-018 | P2 | Cross-team fixture-member removal is rejected | ATM roster mutation is caller-team scoped | Run `atm teams remove-member` under the fixture member identity or another member of the fixture team, then verify absence with a sanitized roster/list check. This is an ATM authorization rule, not a Hermes fork defect | resolved rule; apply at teardown |
| HGC-019 | P2 | First image reported ATM 1.4.6 despite a 1.5.3 override | Testbed `build.sh` selected the alphabetically first stale `atm_*` archive instead of the explicit override | Discard image `670edd12`; testbed commit `b468321` pins override artifacts exactly and purges stale archives. Accept only the rebuilt image whose in-container version triple and baked digests match P0 | resolved before test evidence |

## Stop/escalate decision table

| Condition | Action | Escalate to |
| --- | --- | --- |
| Fork head differs from frozen SHA | Stop review; request a new immutable range | Loki, then Fenix in P1 report |
| Blocking P1 finding remains | Do not start or pass P2 | Loki for fix; Rand via Fenix only for waiver |
| atm-core product change appears necessary | Capture minimal repro and affected path; make no code edit here | Fenix for separate `arch-ctm` dispatch |
| Testbed/Hermes harness bug | Fix on Loki-owned fork/testbed branch with provenance and rerun only the affected confirmation | Loki; report result to Fenix |
| Required tier fails or flakes | Record FAIL and root cause; never rerun-until-green | Loki + Fenix |
| Artifact provenance is incomplete or versions differ | Stop before image build | Loki + Fenix |
| Any command requires sudo | Stop; obtain a no-sudo procedure or revised ruling | Loki + Fenix |
| Cross-host scope or authorized host operator is missing | Mark the leg blocked/out of scope exactly as P0 decides; do not improvise | Fenix |
| All required tiers pass with no unresolved blocking review findings | Report integration PASS; do not publish | Fenix; Rand decides publish |

## Phase report template

Send exactly one ATM message to Fenix after each phase:

````text
<one-sentence phase summary>

```json
{"task_id":"HERMES-GRAFT-COLIMA-R1-1788742538","phase":"P0|P1|P2|P3","status":"pass|fail|blocked|in_progress","artifacts":["<sanitized durable reference>"],"blockers":["<empty or actionable blocker>"]}
```
````

After P2 and P3 pass, send a final readiness message with phase `FINAL`. State
only whether integration unblocks Rand's publish decision; never initiate the
publish.
