---
id: AY.7
phase: AY
sprint: AY.7
title: Windows process correctness and per-user Herdr installer verification
branch: feature/ay7-windows-herdr-process-installer
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay7-windows-herdr-process-installer
integration_branch: integrate/phase-ay
stack_parent: feature/ay6-herdr-restart-coordination
pr_target: feature/ay6-herdr-restart-coordination
target: feature/ay6-herdr-restart-coordination
status: draft
recommended_agent: arch-ctm (no Windows machine required; the Windows CI lane is the gate)
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
    rationale: AY.9 consumes the CI-verified Windows process and installer facts before selecting the socket transport.
---

# AY.7 — Windows process correctness and per-user Herdr installer verification

Make the retained CLI fallback production-correct on Windows and prove, in
the Windows CI lane, that the AY.5/AY.6 installer and restart control plane
uses per-user logon tasks in the same account and session as Herdr. This
sprint does not implement or exercise the socket transport and has no live
run on a physical Windows machine (ruling 5).

## Dispatch and stack contract

AY.7 dispatches when AY.6 development is pushed; it has no physical-machine
precondition. Its branch is created from
`feature/ay6-herdr-restart-coordination`, not from the integration branch.

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
  kill-then-reap path and waits for the child after kill, under a bounded
  cleanup grace period (default 5 s, a `transport_cli.rs` constant): if kill
  plus wait do not complete inside it, the operation ends with an
  infrastructure-class `HerdrError` whose cause is `child_cleanup_timeout`
  (no new public variant) and the child handle is dropped with a structured
  log line naming the pid. A Windows CI test spawns a long-running stand-in
  child (for example `cmd /c` with a `timeout` longer than the deadline),
  hits the deadline, and asserts that kill and wait both complete and
  `try_wait` reports the child exited; a second test uses the crate-private
  child seam (a `ChildHandle` trait object the fake implements with a
  delayed `wait`) to prove the grace period fires and the typed failure is
  returned; a unit test asserts that the one spawn site applies
  `CREATE_NO_WINDOW`. The console-flash visual check is a release-readiness
  row, not a sprint gate.
- [ ] D3 — an architecture test in the AY.7-owned file
  `crates/atm-herdr/tests/windows_process_confinement.rs` fails if
  Windows-only process code for this behavior (`creation_flags`,
  `CREATE_NO_WINDOW`, `herdr.exe`, `cfg(windows)` process handling) appears
  outside `crates/atm-herdr/src/transport_cli.rs`.
  `crates/atm-architecture/tests/boundary_enforcement.rs` is not touched by
  AY.7 (it is in AY.8's C3 allowlist; this keeps AY.7/AY.8 `parallel_safe`).
  The public `HerdrProcessAdapter` signatures and the `HerdrError` enum
  remain unchanged.
- [ ] D4 — verify the Windows branches of AY.5 entry management and AY.6
  restart coordination in the Windows CI lane, running the real Windows
  branch code against the AY.5/AY.6 platform fakes for the privileged
  operations (`schtasks` argv, account/session detection seam, marker files
  in a per-test temp dir): one marker-bearing, per-user logon task for the
  atm daemon and one for each distinct Herdr endpoint; both are asserted to
  run in the interactive user's account/session.
  A service/session-0 or different-account configuration is refused with
  `HERDR_ENTRY_ACCOUNT_MISMATCH`, no task is written, and doctor reports the
  mismatch with the remedy `reinstall per-user`.
- [ ] D5 — fill every Windows-observed column in
  `docs/atm-herdr/windows-process-audit.md` from the Windows CI lane's test
  output and the doctor JSON an integration test in that lane writes as a CI
  artifact: observed newline convention, detached stdio behavior, the pipe
  name shape Herdr reports, date, CI run URL, and the source artifact name.
  Cells that only a live Herdr on a physical machine can fill (console-flash
  result, live pipe name) are marked `release readiness` and carried by
  `release-readiness-herdr-live-proof.md`. Preserve one row per audited item
  and the verdict vocabulary `no action`, `production fix`, or `upstream
  request`.
- [ ] D6 — this sprint creates no live evidence of any kind (ruling 5). The
  only committed observations are CI artifacts and the audit facts required
  by D5. The live macOS/Windows matrix (prompt, wait, get, list, notify,
  late start, upgrade, latency, negative cases) is the release-readiness
  checklist, run after the phase lands on develop.
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
// On deadline/cancellation: kill then wait, both inside a 5 s cleanup grace
// period; exceeding it returns an infrastructure HerdrError with cause
// `child_cleanup_timeout` and logs the pid.
```

The executable is resolved at call time for every `prompt`, `wait`, `get`,
`list`, `notify`, and doctor `server_status` operation. Resolution failure maps
to the existing `HerdrError::ServerUnavailable` and names the configured path
or PATH search in its cause. No new environment precedence is introduced:
each call still passes exactly one of `HERDR_SESSION` or `HERDR_SOCKET_PATH`,
or neither.

AY.7 consumes, but does not change, the AY.3 doctor JSON contract. The doctor JSON
that the Windows CI integration test writes as an artifact must be valid
`atm doctor --json` output for which this extraction succeeds and contains no
aggregate endpoint state:

```sh
jq -e '.herdr.configured == true and (.herdr.endpoints | type == "array") and (.herdr.breaker | type == "object") and (.herdr | has("state") | not)' windows-ci-doctor.json
```

Each endpoint record remains ordered `default` first and then sessions
bytewise. Under the CLI transport its `transport` is `cli` and `endpoint` is
`null`; account/session mismatch is an ordinary doctor finding with code
`HERDR_ENTRY_ACCOUNT_MISMATCH`, not a new `HerdrDoctorState` variant.

## Required work and exact targets

Files AY.7 may add or edit (anything else is a scope change):

- `crates/atm-herdr/src/transport_cli.rs` (Windows branch only: path
  resolution, `CREATE_NO_WINDOW`, UTF-8/CRLF decoding, bounded kill-then-reap,
  the crate-private `ChildHandle` seam)
- `crates/atm-herdr/tests/windows_process_confinement.rs` (new, D3)
- `crates/atm-herdr/tests/windows_transport_cli.rs` (new, `cfg(windows)`
  tests for D1/D2)
- the Windows-branch test modules that AY.5 and AY.6 already name for the
  entry control plane and restart coordination (platform fakes; no new
  production paths)
- `docs/atm-herdr/windows-process-audit.md` (D5)
- `.github/workflows/*` only if the Windows CI lane needs the new test
  binaries listed, with the diff quoted in the PR

Not touched: `crates/atm-architecture/tests/boundary_enforcement.rs`,
`transport_socket.rs`, any boundary TOML, `.just/lint-config.toml`.

1. Keep every Windows process change inside `transport_cli.rs`, then close the
   path-resolution, no-window, UTF-8/CRLF, timeout, kill, and reap cases with
   deterministic tests that run in the Windows CI lane.
2. Exercise the AY.5 entry-management and AY.6 restart Windows branches
   against the platform fakes in the Windows CI lane, including every refusal
   and zero-write case.
3. Fill the audit columns only from the dated Windows CI run and its
   artifacts; keep the diff free of any evidence directory.

## Acceptance criteria

1. D1–D5 are present and D6 holds; `git diff <AY.6-head>..HEAD --name-only`
   contains no `evidence/` path.
2. File-or-directory `binary_path`, PATH fallback, path re-resolution on two
   consecutive calls, missing binary, UTF-8 LF, UTF-8 CRLF, and invalid UTF-8
   cases have deterministic Windows tests.
3. Deadline and cancellation tests prove kill-then-reap in the Windows CI
   lane (`try_wait` reports exit after kill and wait), the delayed-`wait`
   seam test proves the 5 s cleanup grace period returns the typed
   `child_cleanup_timeout` failure, and the creation-flags test proves
   `CREATE_NO_WINDOW` at the single spawn site.
4. Installer tests prove same-user logon tasks for default-only and default plus
   two named sessions, plus refusal and zero writes for service/session-0 and
   different-account configurations.
5. `crates/atm-herdr/tests/windows_process_confinement.rs` proves no
   `cfg(windows)` process handling for this behavior exists outside
   `crates/atm-herdr/src/transport_cli.rs`; `git diff <AY.6-head>..HEAD
   --name-only` does not list `boundary_enforcement.rs` or any boundary TOML.
6. Every audit value cites the dated Windows CI run URL and artifact name, or
   is marked `release readiness` for the cells only a live machine can fill;
   no cell remains `AY.7`, `TBD`, or an equivalent placeholder.
7. `gh pr view feature/ay7-windows-herdr-process-installer --json
   headRefName,baseRefName,state` reports base
   `feature/ay6-herdr-restart-coordination`.

## Required validation

- `just validate` on the dev host.
- `cargo test -p atm-herdr` in the repository's Windows CI lane (the gate).
- All three CI lanes green at merge time; Windows remains the merge gate.
- Run the architecture guard and the audit placeholder/no-evidence grep gates.
- quality-mgr Final Quality Report: 0 blocking, 0 important, 0 minor in scope.

## Out of scope

- Direct UDS/named-pipe transport or its production cutover (AY.8/AY.9).
- Live evidence of any kind; the live matrix is release readiness (ruling 5).
- Herdr upgrade, supervision, or daemon-startup behavior.
- Any patch, hardening, or remodeling of the legacy synchronous daemon. All
  daemon-side composition remains Tokio/Axum through `atm-http-runtime`.
