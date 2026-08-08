# AL.14 — cwin Direct Cross-Host Hardware Smoke

**branch:** `feature/al-14-smoke`
**worktree:** `../atm-core-worktrees/feature/al-14-smoke`
**recommended_agent:** cwin hardware operator, with the M5 operator available
as the already-running peer.
**must_follow:** AL.13's recorded tested commit and smoke-runner contract. The
same SHA/version is required on both hosts; AL.14 never proves a mixed build.
**unblocks:** AL.15 combined hardware-evidence review.
**parallel_safe:** local setup may run in parallel with AL.13. Run the direct
cross-host ladder only in a scheduled window, not concurrently with AL.13's
peer-send ladder.

## Tested-candidate manifest and scope gate

AL.14 consumes the immutable three-SHA manifest first recorded by AL.13. It
must reproduce the following checks in the cwin PR before it builds or runs:

| Field | Required value and check |
|---|---|
| `tested_sha` | The exact commit built and selected on both hosts. `git rev-parse HEAD` on cwin must equal it before the first smoke command. |
| `runtime_candidate_sha` | The AL.13 runtime candidate. `git merge-base --is-ancestor <runtime_candidate_sha> <tested_sha>` must exit zero. |
| `report_index_sha` | `faf0c24b2743274590de4607bfc07654bff63709` (PR #788). `git merge-base --is-ancestor <report_index_sha> <tested_sha>` must exit zero. |
| `home_branch` | Exactly `feature/al-14-smoke`; `git branch --show-current` must report it before evidence is committed. |

The only valid live feature names are `localhost`, `local-ip`,
`peer-preflight`, `crosshost-send`, and `crosshost-ack`. The run is invalid if
an artifact or PR status report names `crosshost`, `crosshost-curl-plain`,
`crosshost-curl-tls`, a raw socket probe, or an operator-issued `atm send`/
`atm read` outside the repository runner. The runner-generated body and ID are
the only valid payload evidence.

For every code-bearing AL.14 fix, review the diff from `tested_sha` before
rerunning. It fails this sprint's scope gate if it changes
`crates/atm-daemon/**`, adds a TLS/certificate/peer-wire-security setting, or
adds resend/replay/retry/heartbeat/cursor/scheduler/batch behavior. Record the
review in the PR. Any required change in those areas needs a separate decision
and cannot be smuggled into this Windows evidence sprint.

## Purpose

Provide the Windows-originating half of the minimal direct M5↔cwin proof. The
test is deliberately the same public `atm` send/read/ack behavior as AL.13,
not a Windows-specific protocol or a different server path. It confirms that
the configured direct peer endpoint is reachable from Windows and that the
same request body/result semantics hold in both directions.

## Fixed operating rules

- Use AL.13's tested-candidate manifest exactly. Record its immutable SHA in
  every artifact; a moving branch name or a pre-index smoke checkout is not
  valid evidence.
- Work from cwin's home sprint branch `feature/al-14-smoke`; open a PR with
  evidence and status rather than depositing output in another host's branch.
- Use `/smoke-test` and `just smoke` only. PowerShell wrappers may set the two
  documented environment variables, but must not replace the runner, issue
  raw HTTP, or manipulate SQLite/inboxes directly.
- Select one matched Windows CLI/daemon pair with `/daemon-switch`. Use its
  Windows selector-symlink workflow; never overwrite installed executables or
  run a second daemon. Retain the `daemon-switch.py status --doctor` output
  before selection and confirm the selected pair with `atm doctor --json`.
- This proof exercises direct plaintext delivery selected by the tested
  runtime. Do not add TLS/mTLS configuration, curl diagnostics, certificates,
  or an alternate listener. Those are separate, non-required work.
- Do not introduce or enable replay, retry, recovery timers, cursors, batches,
  or post-send background delivery. An unavailable peer is a typed direct-send
  result, not a reason to change the test payload or delivery path.

## Deliverables

1. Use `/sc-git-worktree` to select or create cwin's home worktree
   `feature/al-14-smoke` from the named testing ref. Do not run the candidate
   from an arbitrary checkout or a stale local clone.
2. Record the tested-candidate manifest, client/daemon version, Windows
   version, sanitized hostname, M5 SSH alias, and the selected pair in the
   cwin PR. Confirm it exactly matches AL.13's recorded manifest/version
   before continuing.
3. Build/select the pair, then run `atm doctor --json`. Stop for any unhealthy
   daemon, version mismatch, or competing service.
4. Run the local Windows ladder in order:

   ```powershell
   just smoke localhost
   just smoke local-ip
   ```

   The Windows local-IP row is mandatory; it proves the runner uses the
   Windows TCP adapter before a peer test is attempted.
5. During the agreed M5 availability window, configure only the existing
   recipient context and execute the same direct peer ladder from cwin:

   ```powershell
   $env:ATM_SMOKE_REMOTE_IDENTITY = '<configured-m5-agent>'
   $env:ATM_SMOKE_REMOTE_TEAM = '<configured-m5-team>'
   just smoke peer-preflight <m5-ssh-alias>
   just smoke crosshost-send <m5-ssh-alias>
   just smoke crosshost-ack <m5-ssh-alias>
   ```

   A pass requires the runner's forward and reverse exact-ID/body proof. The
   acknowledgement stage additionally requires both acknowledgement replies
   to arrive with correct source linkage. A peer-preflight failure blocks the
   send and ack stages; do not replace it with a manual connection test.
6. Retain the self-contained reports under
   `site/reports/smoke/windows/<host>/<run>/`, confirm each is linked from
   `site/reports/index.html`, and commit them on `feature/al-14-smoke`.
7. Post a cwin PR status report with all command results, tested SHA/version,
   master-index and per-run evidence links, failures/blocked stages, and any
   narrow fix made before rerunning. Ask Rand for a decision if the only
   apparent workaround changes endpoint selection, message serialization, or
   direct-send semantics.

## Acceptance criteria

- cwin runs a healthy, version-matched single daemon and passes localhost plus
  local-IP smoke before cross-host work.
- The cwin-initiated direct ladder passes in order and the evidence proves the
  same exact send/read and ack/reply behavior both directions with M5.
- No Windows-only wire format, local daemon proxy, TLS/curl diagnostic, retry,
  replay, message mutation, second listener, or legacy-daemon change is used;
  the tested-candidate manifest and scope gate pass.
- The master report index links every retained cwin run, each report includes
  Windows platform and host identity, and the cwin PR describes the result.

## Required validation

- `just test` for any code change before the live run.
- `atm doctor --json` on cwin and M5 before each cross-host ladder.
- The complete ordered cwin sequence above, plus manual browser navigation
  from `site/reports/index.html` to every generated evidence page.
- Diff review of `tested_sha..HEAD`, recorded in the PR, confirming no
  `crates/atm-daemon/**` change, no added TLS/certificate/peer-wire-security
  setting, and no added resend/replay/retry/heartbeat/cursor/scheduler/batch
  behavior.
- Cross-check that the M5 and cwin reports name the same tested SHA/version
  and report the same message IDs for the shared run window.
- Review of the cwin PR by the M5 operator for peer availability and by the
  coordinator for version/SHA parity and the recorded scope-gate diff result.

## Non-closure

AL.14 does not replace the M5-originating proof, test recovery after a network
outage, or approve any resend/replay behavior. AL.15 evaluates only the
retained evidence from the two direct lanes.
