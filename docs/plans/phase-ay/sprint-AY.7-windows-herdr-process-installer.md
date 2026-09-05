---
phase: AY
sprint: AY.7
title: Windows process correctness and per-user Herdr installer verification
branch: feature/ay7-windows-herdr-process-installer
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay7-windows-herdr-process-installer
integration_branch: integrate/phase-ay
stack_parent: feature/ay6-herdr-restart-coordination
pr_target: feature/ay6-herdr-restart-coordination
status: draft
recommended_agent: named-windows-agent (P-D)
recommended_model: n/a
execution_track: windows
parallel_with: [AY.8]
dependency_relations:
  - prerequisite: AY.6
    dependent: AY.7
    relation: must_follow
    rationale: AY.7 verifies the Windows branches of AY.5 entry management and AY.6 restart coordination and is stacked directly on AY.6.
  - prerequisite: AY.7
    dependent: AY.8
    relation: parallel_safe
    rationale: AY.7 owns transport_cli.rs Windows code, Windows installer verification, and the process-audit document; AY.8 owns transport_socket.rs, socket fixtures, and its enumerated boundary changes.
  - prerequisite: AY.7
    dependent: AY.9
    relation: must_follow
    rationale: AY.9 consumes the verified Windows process and installer facts before selecting the socket transport; AY.10 then performs the live Windows socket proof.
---

# AY.7 — Windows process correctness and per-user Herdr installer verification

Make the retained CLI fallback production-correct on Windows and prove that
the AY.5/AY.6 installer and restart control plane uses per-user logon tasks in
the same account and session as Herdr. This sprint does not implement or
exercise the socket transport.

## Dispatch and stack contract

AY.7 does not dispatch until P-C and P-D in the phase plan are filled: FastPC4
is reachable by the agreed operator path, its `atm-dev` reporter has completed
one round trip, and the exact Windows developer identity is named. Its branch
is created from `feature/ay6-herdr-restart-coordination`, not from the integration
branch.

Use the `/gh-stack` skill for the phase's linear implementation stack. After
the `AY.2 -> AY.3 -> AY.4 -> AY.5 -> AY.6 -> AY.7` branches and PRs exist,
append AY.7 to the already-linked remote stack with explicit non-interactive
arguments, then verify the PR base with ordinary GitHub JSON:

```sh
gh stack link <stack-number> feature/ay7-windows-herdr-process-installer
gh pr view feature/ay7-windows-herdr-process-installer --json headRefName,baseRefName,state
```

`gh stack link` is the remote stack/PR update operation for the external
`sc-git-worktree` workflow; it creates no local gh-stack state, so this sprint
uses ordinary `gh pr view --json` for verification. After any AY.6 development
push and before each AY.7 development or fix round, merge AY.6 forward into
AY.7 with a merge commit. AY.6's PR merges first. AY.8 is a separate branch
from `integrate/phase-ay` and runs in parallel; never merge the unmerged AY.8
sibling into AY.7.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or shape-only
completion fails the sprint.

- [ ] D1 — Windows process correctness is confined to the `cfg(windows)`
  implementation in `crates/atm-herdr/src/transport_cli.rs`. Every spawn
  resolves `herdr.exe` anew: an absolute `binary_path` may name the executable
  or its directory; otherwise use PATH lookup. Never cache a resolved path.
  Document `%LOCALAPPDATA%\Programs\Herdr\bin` as the recommended stable alias
  directory and preserve the adjacent `conpty/` payload by never copying the
  executable alone. Captured stdout and stderr are decoded as UTF-8 with both
  LF and CRLF accepted.
- [ ] D2 — every Windows CLI-transport spawn uses
  `CREATE_NO_WINDOW = 0x08000000`; timeout/cancellation follows the existing
  kill-then-reap path and waits for the child after kill. A FastPC4 test proves
  that the child leaves no `herdr.exe` orphan in `tasklist` and a visual check
  proves that no console window flashes.
- [ ] D3 — an architecture test fails if Windows-only process code for this
  behavior appears outside `transport_cli.rs`. The public
  `HerdrProcessAdapter` signatures and the `HerdrError` enum remain unchanged.
- [ ] D4 — live-verify the Windows branches of AY.5 entry management and AY.6
  restart coordination on FastPC4:
  one marker-bearing, per-user logon task for the atm daemon and one for each
  distinct Herdr endpoint; both run in the interactive user's account/session.
  A service/session-0 or different-account configuration is refused with
  `HERDR_ENTRY_ACCOUNT_MISMATCH`, no task is written, and doctor reports the
  mismatch with the remedy `reinstall per-user`.
- [ ] D5 — fill every Windows-observed column in
  `docs/atm-herdr/windows-process-audit.md`: observed newline convention,
  detached stdio behavior, console-flash result, the exact pipe name Herdr
  reports, date, FastPC4 identity, and the source doctor artifact. Preserve one
  row per audited item and the verdict vocabulary `no action`,
  `production fix`, or `upstream request`.
- [ ] D6 — this sprint creates no live Herdr round-trip evidence campaign.
  The only committed observations are the doctor `herdr` section required by
  D4 and the audit facts required by D5. The phase's macOS/Windows prompt,
  wait, get, list, notify, late-start, upgrade, latency, and negative-case
  evidence belongs only to AY.10 on the socket transport after AY.9's cutover.
- [ ] D7 — tests and evidence checks under Required validation pass on the
  branch and in the Windows CI lane.

### Paths to delete

None.

## Exact behavior contracts

No public Rust interface is added. The internal Windows branch implements this
behavior exactly:

```rust
// Pseudocode contract; names may follow the existing transport_cli.rs helpers.
let program = match config.binary_path.as_deref() {
    Some(path) if path.is_dir() => path.join("herdr.exe"),
    Some(path) => path.to_owned(),
    None => PathBuf::from("herdr.exe"), // resolved by Windows PATH for this spawn
};
let mut command = tokio::process::Command::new(program);
command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
// On deadline/cancellation: child.kill().await, then child.wait().await.
```

The executable is resolved at call time for every `prompt`, `wait`, `get`,
`list`, `notify`, and doctor `server_status` operation. Resolution failure maps
to the existing `HerdrError::ServerUnavailable` and names the configured path
or PATH search in its cause. No new environment precedence is introduced:
each call still passes exactly one of `HERDR_SESSION` or `HERDR_SOCKET_PATH`,
or neither.

AY.7 consumes, but does not change, the AY.3 doctor JSON contract. The captured
artifact must be valid `atm doctor --json` output for which this extraction
succeeds and contains no aggregate endpoint state:

```sh
jq -e '.herdr.configured == true and (.herdr.endpoints | type == "array") and (.herdr.breaker | type == "object") and (.herdr | has("state") | not)' fastpc4-doctor.json
```

Each endpoint record remains ordered `default` first and then sessions
bytewise. Under the CLI transport its `transport` is `cli` and `endpoint` is
`null`; account/session mismatch is an ordinary doctor finding with code
`HERDR_ENTRY_ACCOUNT_MISMATCH`, not a new `HerdrDoctorState` variant.

## Required work

1. Keep every Windows process change inside `transport_cli.rs`, then close the
   path-resolution, no-window, UTF-8/CRLF, timeout, kill, and reap cases with
   deterministic tests before using FastPC4.
2. Exercise AY.5 entry management and AY.6 restart behavior under the same
   interactive Windows account, including every refusal and zero-write case.
3. Fill the audit columns only from the dated FastPC4 run and keep the diff
   free of a live round-trip evidence directory; AY.10 owns that campaign.

## Acceptance criteria

1. D1–D5 are present and D6 holds; `git diff <AY.6-head>..HEAD --name-only`
   contains no `evidence/` path.
2. File-or-directory `binary_path`, PATH fallback, path re-resolution on two
   consecutive calls, missing binary, UTF-8 LF, UTF-8 CRLF, and invalid UTF-8
   cases have deterministic Windows tests.
3. Deadline and cancellation tests prove kill-then-reap. The FastPC4 run cites
   before/after `tasklist` output with no orphan and records no console flash.
4. Installer tests prove same-user logon tasks for default-only and default plus
   two named sessions, plus refusal and zero writes for service/session-0 and
   different-account configurations.
5. The architecture test proves no `cfg(windows)` for this process behavior
   exists outside `crates/atm-herdr/src/transport_cli.rs`.
6. Every audit value cites the dated FastPC4 run and source artifact; no cell
   remains `AY.7`, `TBD`, or an equivalent placeholder.
7. `gh pr view feature/ay7-windows-herdr-process-installer --json
   headRefName,baseRefName,state` reports base
   `feature/ay6-herdr-restart-coordination`.

## Required validation

- `just validate` on FastPC4.
- `cargo test -p atm-herdr` on FastPC4 and the repository's Windows CI lane.
- All three CI lanes green at merge time; Windows remains the merge gate.
- Run the architecture guard and the audit placeholder/no-evidence grep gates.
- quality-mgr Final Quality Report: 0 blocking, 0 important, 0 minor in scope.

## Out of scope

- Direct UDS/named-pipe transport or its production cutover (AY.8/AY.9).
- The phase's live Herdr evidence set (AY.10).
- Herdr upgrade, supervision, or daemon-startup behavior.
- Any patch, hardening, or remodeling of the legacy synchronous daemon. All
  daemon-side composition remains Tokio/Axum through `atm-http-runtime`.
