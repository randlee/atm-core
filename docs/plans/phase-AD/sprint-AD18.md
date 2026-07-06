---
id: AD.18
title: Raw CLI Runtime Root Unification
status: planned
branch: feature/pAD-s18-raw-cli-runtime-root-unification
worktree: ../atm-core-worktrees/feature/pAD-s18-raw-cli-runtime-root-unification
target: integrate/phase-AD
---

# Sprint AD.18 — Raw CLI Runtime Root Unification

## Goal

- remove worktree- and invocation-directory-sensitive runtime selection from
  raw ATM CLI commands so `atm send`, `atm read`, `atm ack`, and the retained
  caller-context command surface always hit the same daemon/SQLite runtime for
  one `ATM_HOME` / host-home installation

## Hard Dependencies

- `AD.1` complete
- `AD.9` complete
- `AD.10` complete
- `AD.11` complete
- `AD.13` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/violation-inventory.md`

## Exact Targets

- `crates/atm-core/src/home.rs`
- `crates/atm/src/composition.rs`
- `crates/atm/src/commands/send.rs`
- `crates/atm/src/commands/read.rs`
- `crates/atm/src/commands/ack.rs`
- `crates/atm/src/commands/log.rs`
- `crates/atm/src/commands/list.rs`
- `crates/atm/src/commands/clear.rs`
- `crates/atm/src/commands/members.rs`
- `crates/atm/src/commands/teams.rs`
- `crates/atm/src/commands/doctor.rs`
- `scripts/smoke/fixtures.py`
- `scripts/smoke/run.py`
- `scripts/smoke/run_thorough.py`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`

## Interfaces To Add Or Modify

Raw CLI bootstrap must stop inferring runtime roots from ambient process state
after the command entrypoint resolves them once:

```rust
pub fn command_invocation_dir() -> Result<PathBuf, AtmError>;

pub fn bootstrap(
    command: &'static str,
    observability: &CliObservability,
    invocation_dir: &Path,
    atm_home: &Path,
) -> Result<CliComposition<'_>, AtmError>;
```

The accepted runtime-root rule after this sprint is:

- `atm_home` comes from `ATM_HOME` or OS user home through the existing ATM
  home resolver
- host-scoped daemon socket, launch-gate lock, log root, and durable SQLite
  root are derived from that accepted home resolution only
- `invocation_dir` exists only for workspace config ingress, repo/file-policy
  checks, and hook/config-relative path resolution
- no retained raw CLI command may switch to a JSON-only compatibility path,
  UUID-style message-id output, or an alternate mailbox store because the user
  invoked the command from a sibling worktree or another repo checkout
- any advisory-session deletion work inside `crates/atm/src/composition.rs`
  remains owned by `AD.14`; this sprint may touch that file only for
  runtime-root/bootstrap selection logic

## Paths To Delete

- ambient `std::env::current_dir()` dependence inside raw CLI bootstrap after
  the command entrypoint has already captured the invocation directory
- any retained path that treats discovered `.atm.toml`, repo root, or worktree
  root as authority for daemon socket selection, durable SQLite root
  selection, or retained mailbox store selection
- any smoke/readiness proof that only validates the Python wrapper path for
  this behavior while leaving raw `atm` commands unverified

## Deliverables

- raw `atm send`, `atm read`, and `atm ack` executed from the primary repo and
  from a sibling worktree hit the same daemon/SQLite runtime and return ULID
  message ids
- raw retained caller-context commands no longer require wrapper-enforced `cwd`
  normalization to avoid JSON-only compatibility writes, regardless of whether
  the command is `send`, `read`, `ack`, `log`, `list`, `clear`, `members`,
  `teams`, `teams add-member`, `teams update-member`, `teams backup`, or
  `teams restore`
- command/root-resolution docs explicitly distinguish:
  - accepted ATM home / host-runtime root
  - invocation directory
  - workspace config discovery root
- smoke coverage proves one `ATM_HOME` installation stays canonical across
  sibling worktrees

## This Sprint Does Not Close

- graft boundary reset
- post-send emitter changes
- caller-context identity/team policy changes
- new wrapper features

## Acceptance Criteria

- raw `atm send` from a sibling worktree persists to the same SQLite database
  as raw `atm send` from the primary repo, with no JSON-only fallback row set
  and no UUID-form message id in command output
- raw `atm read --message-id <ulid>` from a sibling worktree can retrieve a
  message sent from the primary repo against the same durable store
- raw `atm ack <ulid>` from a sibling worktree acknowledges the same durable
  message and persists the reply through the same SQLite-backed runtime
- raw retained caller-context commands no longer depend on worktree `cwd` to
  select retained runtime/storage roots:
  - `send`
  - `read`
  - `ack`
  - `log`
  - `list`
  - `clear`
  - `members`
  - `teams`
  - `teams add-member`
  - `teams update-member`
  - `teams backup`
  - `teams restore`
- `docs/requirements.md`, `docs/architecture.md`,
  `docs/atm-core/requirements.md`, `docs/atm-core/architecture.md`,
  `docs/atm-daemon/requirements.md`, and `docs/atm-daemon/architecture.md`
  state that invocation directory is not a daemon/socket/database selector
- wrappers remain optional convenience only; release readiness no longer
  depends on wrapper-enforced `cwd` forcing

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `just smoke normal`
- `just smoke thorough`
- targeted raw multi-worktree CLI regression coverage for send/read/ack and the
  retained caller-context command matrix
- `rg -n "std::env::current_dir\\(\\).*bootstrap|current_dir.*daemon_socket_path|current_dir.*mail\\.db|current_dir.*\\.claude/.*/inboxes" crates/atm crates/atm-core`
- `git diff --check`
