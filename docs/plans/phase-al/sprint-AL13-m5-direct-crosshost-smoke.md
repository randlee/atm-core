# AL.13 — M5 Direct Cross-Host Hardware Smoke

**branch:** `feature/al-13-smoke`
**worktree:** `../atm-core-worktrees/feature/al-13-smoke`
**recommended_agent:** M5 hardware operator, with an M4/cwin operator
available as the already-running peer.
**must_follow:** AL.9's accepted runtime composition and the `develop`
smoke-skill/report-index contract. Before a run, the coordinator records one
exact tested commit and requires both hosts to select binaries built from that
same commit.
**unblocks:** AL.15 evidence review.  A failure blocks AL.14's symmetric run
only when it identifies a shared precondition (for example, a version mismatch
or unavailable peer); otherwise AL.14 may still establish the other direction.
**parallel_safe:** local preparation may run with AL.14. The two hosts must not
run cross-host sends simultaneously against the same disposable identities.

## Tested-candidate manifest and scope gate

Before M5 builds or switches binaries, the coordinator records this immutable
three-SHA manifest in the AL.13 PR description.  It is the one source for the
run; a branch name is never evidence.

| Field | Required value and check |
|---|---|
| `tested_sha` | The exact commit built and selected on both hosts. `git rev-parse HEAD` on M5 must equal it before the first smoke command. |
| `runtime_candidate_sha` | The exact accepted HTTP-runtime candidate being tested. `git merge-base --is-ancestor <runtime_candidate_sha> <tested_sha>` must exit zero. |
| `report_index_sha` | `faf0c24b2743274590de4607bfc07654bff63709` (PR #788). `git merge-base --is-ancestor <report_index_sha> <tested_sha>` must exit zero. |
| `home_branch` | Exactly `feature/al-13-smoke`; `git branch --show-current` must report it before evidence is committed. |

The M5 operator copies this manifest unchanged to the cwin operator before
AL.14 starts. A different SHA, version, runtime candidate, or report-index
ancestor is a blocked run, not an equivalent test.

The only valid live feature names are `localhost`, `local-ip`,
`peer-preflight`, `crosshost-send`, and `crosshost-ack`. The run is out of
scope if an artifact or PR status report names `crosshost`,
`crosshost-curl-plain`, `crosshost-curl-tls`, a raw socket probe, or an
operator-issued `atm send`/`atm read` outside the repository runner. The
runner-generated message body and ID are the only payload evidence; no
operator-supplied or transformed body is acceptable.

For every code-bearing AL.13 fix, review the diff from `tested_sha` before
rerunning. It fails this sprint's scope gate if it changes
`crates/atm-daemon/**`, adds a TLS/certificate/peer-wire-security setting, or
adds resend/replay/retry/heartbeat/cursor/scheduler/batch behavior. The
reviewer records that result in the PR. A needed change in any of those areas
is a separate decision, not a hardware-smoke fix.

## Purpose

Prove minimal direct delivery between the M5 and its configured peer using the
same public CLI path used by a normal `atm send`. This is a hardware-evidence
sprint, not a transport implementation sprint. It proves the current runtime
as installed on real hosts; it does not add a listener, a peer protocol, TLS,
curl probes, retry, replay, batching, or a nudge-specific path.

The peer's SSH alias and the two test identities are supplied by the operators
from their existing ATM configuration. They are not committed in this plan,
source tree, shell history, or report metadata beyond the sanitized evidence
already written by the smoke runner.

## Fixed operating rules

- The coordinator creates the tested-candidate manifest above before either
  operator builds. Neither host may silently test an older runtime branch that
  lacks the report contract.
- Work from the M5 home sprint branch `feature/al-13-smoke`; the resulting PR
  contains only the M5 evidence, an explicit status summary, and any narrowly
  necessary fix. Do not put M5 evidence on an unrelated branch.
- Use the repository's `/smoke-test` skill and the `just smoke` commands below.
  Do not create a second shell script, raw-socket probe, curl-based proof, or
  a manual inbox mutation.
- Use `/daemon-switch` to select the matched `atm` and daemon binaries as one
  computer-wide pair. Run
  `python3 .claude/skills/daemon-switch/scripts/daemon-switch.py status --doctor`
  before the switch and retain its output with the post-switch `atm doctor
  --json` output. There must be exactly one managed daemon before and after
  the run.
- The direct proof is plaintext as selected by the currently tested runtime.
  This sprint must not configure, require, or claim TLS/mTLS. The
  `crosshost-curl-plain` and `crosshost-curl-tls` diagnostic features are out
  of scope and do not substitute for an `atm send` proof.
- Direct failure is final for this sprint: do not add or enable resend,
  replay, heartbeat, cursor, queue, scheduler, or a mutated message body.

## Deliverables

1. Use `/sc-git-worktree` to select or create M5's home worktree
   `feature/al-13-smoke` from the named testing ref. Do not run from a random
   checkout, `develop`, or a previous evidence directory.
2. Record the tested-candidate manifest, client/daemon version,
   operating-system version, M5 hostname, peer SSH alias, and the exact
   selected CLI/daemon pair in the M5 PR description. Redact addresses,
   credentials, certificates, and private configuration values.
3. Build/select that same release pair on M5. Run `atm doctor --json` and stop
   if its client and daemon versions differ from the recorded tested version or
   readiness is not `ready`.
4. Establish the local ladder, in order:

   ```bash
   just smoke localhost
   just smoke local-ip
   ```

   Each command must pass before the next begins. A local failure is a local
   runtime defect or setup defect, not a reason to attempt cross-host traffic.
5. With the peer daemon already healthy and SSH-reachable, set only the remote
   recipient context required by the runner, then run the direct peer ladder:

   ```bash
   export ATM_SMOKE_REMOTE_IDENTITY='<configured-peer-agent>'
   export ATM_SMOKE_REMOTE_TEAM='<configured-peer-team>'
   just smoke peer-preflight <peer-ssh-alias>
   just smoke crosshost-send <peer-ssh-alias>
   just smoke crosshost-ack <peer-ssh-alias>
   ```

   `peer-preflight` must pass first. `crosshost-send` proves exact message ID
   and body visibility in both M5→peer and peer→M5 directions. `crosshost-ack`
   adds the requires-ack and acknowledgement-reply round trip in both
   directions. The runner invokes only public `atm` commands on the peer; it
   must not start, stop, configure, or repair the remote daemon.
6. Retain every generated self-contained run directory under
   `site/reports/smoke/<platform>/<host>/<run>/`. The runner updates the
   generated `site/reports/index.html`; verify the master page links each M5
   run and each page opens its XHTML evidence panels. Commit the retained
   evidence only to the M5 home sprint branch.
7. Open or update the M5 PR with a status report: commands attempted in order,
   PASS/FAIL result for every command, tested SHA/version, report links,
   observed failure text (if any), and the next blocked action. If a narrowly
   scoped code/configuration defect is found, fix it on the same home branch,
   rerun from the first affected ladder stage, and explain the exact change in
   the PR. Escalate ambiguous architecture decisions to Rand rather than
   inventing a fallback path.

## Acceptance criteria

- Both M5 local stages pass using one matched CLI/daemon pair.
- `peer-preflight`, `crosshost-send`, and `crosshost-ack` pass in that order
  against a healthy configured peer; the retained evidence proves both
  directions, exact message IDs/bodies, and acknowledgement linkage.
- The output shows no retry/replay/reconciliation action and no second
  transport route, raw socket, curl proof, TLS claim, payload mutation, or
  legacy-daemon change; the tested-candidate manifest and scope gate pass.
- Every evidence run is linked from `site/reports/index.html`, preserves its
  platform and host path, and is accompanied by an M5 PR status summary.
- A failure is recorded as a failure with its first failing stage and exact
  output; later stages are marked blocked rather than silently retried.

## Required validation

- `just test` before hardware execution when code changes are present; no
  code-only PR may claim a hardware result.
- `atm doctor --json` on M5 and the peer before cross-host execution.
- The full progressive command sequence above, with generated report pages
  inspected through the master reports index.
- Diff review of `tested_sha..HEAD`, recorded in the PR, confirming no
  `crates/atm-daemon/**` change, no added TLS/certificate/peer-wire-security
  setting, and no added resend/replay/retry/heartbeat/cursor/scheduler/batch
  behavior.
- Review of the M5 PR by the cwin operator for peer availability and by the
  coordinator for version/SHA parity.

## Non-closure

AL.13 does not prove Windows-specific behavior by itself, does not test a
different client transport, and does not authorize a recovery feature. AL.15
owns combined evidence acceptance only after the cwin symmetric run is
available.
