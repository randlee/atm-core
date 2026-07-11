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
- `crates/atm/src/observability.rs`
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
- `docs/atm-error-codes.md`
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

pub(crate) enum CliBootstrapError {
    AtmHomeUnresolved {
        command: &'static str,
    },
    RuntimeRootInvalid {
        command: &'static str,
        atm_home: PathBuf,
        invocation_dir: PathBuf,
    },
    RuntimeBootstrapRefused {
        command: &'static str,
        atm_home: PathBuf,
        daemon_endpoint: PathBuf,
    },
}

pub(crate) fn bootstrap(
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
- runtime-root failures are explicit discriminated-union failures, not string
  errors or silent fallback:
  - `AtmHomeUnresolved`: CLI could not derive `ATM_HOME` or a valid user-home
    fallback before bootstrap
  - `RuntimeRootInvalid`: the resolved host-runtime root cannot produce a valid
    daemon endpoint / SQLite root / launch-gate path set
  - `RuntimeBootstrapRefused`: the raw CLI refused to talk to a runtime whose
    derived socket / lock / store roots do not match the accepted
    host-home-based contract
- source-site logging is required before each runtime-root failure is returned;
  command entry, bootstrap, or root-derivation code must emit the structured
  failure event at the place that detects it instead of relying on a later
  catch-all wrapper to reconstruct cause
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
- runtime-root failures are machine-distinguishable and logged at the source,
  so operators can tell the difference between missing `ATM_HOME`, invalid
  root derivation, and bootstrap refusal without wrapper-specific debugging
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

## Error Contract

- `AtmHomeUnresolved` / `ATM_HOME_UNRESOLVED`
  - cause: raw CLI command entry could not derive a valid `ATM_HOME` or OS
    user-home fallback before runtime bootstrap
  - emitted by: command entry / ATM-home resolution before daemon bootstrap
  - caller surface: command failure before daemon contact
  - recovery: set `ATM_HOME` explicitly or repair the host-home environment so
    ATM can derive the accepted host runtime root
  - source logging: required at the site that fails home resolution
- `RuntimeRootInvalid` / `ATM_RUNTIME_ROOT_INVALID`
  - cause: the accepted `ATM_HOME` resolved, but the derived daemon socket,
    launch-gate lock, log root, or SQLite root is malformed, missing, or
    inconsistent with the accepted runtime-root contract
  - emitted by: bootstrap/runtime-root derivation before daemon contact
  - caller surface: command failure before daemon contact
  - recovery: repair the derived runtime-root inputs or the invalid persisted
    runtime-root state, then retry the raw CLI command
  - source logging: required at the exact root-derivation failure site
- `RuntimeBootstrapRefused` / `ATM_RUNTIME_BOOTSTRAP_REFUSED`
  - cause: raw CLI bootstrap detected a runtime/store selection mismatch and
    refused to continue rather than silently falling back to a different store
    or compatibility path
  - emitted by: bootstrap/runtime-selection guard before daemon contact
  - caller surface: command failure before daemon contact
  - recovery: repair the runtime-root mismatch so raw `atm` resolves one
    canonical host runtime for the accepted `ATM_HOME`
  - source logging: required at the exact guard that rejects the mismatched
    runtime selection

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
- `docs/atm-error-codes.md` and the accepted runtime-root docs describe the
  discriminated runtime-root failure contract rather than leaving raw CLI
  bootstrap errors as opaque strings
- command entry / bootstrap code logs each runtime-root failure at the source
  before returning the discriminated error variant
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
- targeted runtime-root failure-path coverage proving:
  - missing/invalid `ATM_HOME` returns `ATM_HOME_UNRESOLVED`
  - invalid root derivation returns `ATM_RUNTIME_ROOT_INVALID`
  - runtime-selection mismatch returns `ATM_RUNTIME_BOOTSTRAP_REFUSED`
  - each failure path emits one structured source-site log event
- `rg -n "std::env::current_dir\\(\\).*bootstrap|current_dir.*daemon_socket_path|current_dir.*mail\\.db|current_dir.*\\.claude/.*/inboxes" crates/atm crates/atm-core`
- `git diff --check`
