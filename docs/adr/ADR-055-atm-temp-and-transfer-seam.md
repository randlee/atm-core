# ADR-055 — `ATM_TEMP` Scratch-Root Contract and Cross-Host Transfer Seam

| Field | Value |
| --- | --- |
| ID | ADR-055 |
| Status | Accepted |
| Scope | ATM scratch-space resolution, TTL sweep, and user-configured cross-host file transfer for Send-To (Phase AQ, AQ4) |
| Relates to | ADR-018, ADR-026, ADR-032, `docs/plans/phase-aq/sprint-AQ4-send-to-core.md`, `docs/plans/phase-aq/prd-atm-send-to.md` §4.5 |

## Context

Send-To (PRD §4.5) needs one place to stage files: locally for same-host
delivery, and as a hand-off point for a user-provided cross-host transfer
script. Before this ADR, `atm-core` had no scratch-root concept at all — no
`AtmConfig` field, no environment variable, no `std::env::temp_dir()`
production call site (every existing repo call site is a test fixture or a
module already gated for removal). Formalizing "the ATM scratch root" is
exactly what this ADR is for.

Two further problems compound the naive "just add an env var" design:

- **Rollout safety (critical review B10).** An already-installed daemon, the
  `daemon-switch` typed launch-overlay sessions (ADR-053), the m5 Hermes
  fleet host, and any `cargo test` invocation outside `just` all start today
  with no `ATM_TEMP` in their launch environment. A design that makes
  `ATM_TEMP` a hard boot requirement breaks every one of them the day this
  ships.
- **Shared-host safety (critical review F7).** A scratch directory that
  falls back to a well-known path under the shared system temp root is a
  classic pre-creation attack: another local user can create
  `/tmp/atm-<uid>` (or guess an explicit `ATM_TEMP` value) ahead of the
  legitimate owner and plant a world-writable or foreign-owned directory
  that ATM would otherwise silently adopt.

Cross-host transfer has its own long-settled context (PRD §4.5, "Binding
decisions"): bytes move via a user-provided script per destination host,
never daemon machinery — no fetch/push endpoints, no transfer state machine,
no envelope change, no new storage traits. This ADR is the first place that
contract gets a concrete resolution algorithm and safety check.

## Decision

### (a) `ATM_TEMP` is a system-level contract with a non-breaking rollout

One environment variable, `ATM_TEMP`, names the ATM scratch root for every
feature that needs one. It is resolved by exactly one shared function,
`resolve_atm_temp`, which the daemon calls once at startup and the CLI calls
lazily at first scratch-space use; commands that touch no scratch space are
unaffected either way.

**Unset is not a startup failure.** `resolve_atm_temp` defaults to:

- Unix: `<std::env::temp_dir()>/atm-<uid>`, created with mode `0700` if
  missing.
- Windows: `<std::env::temp_dir()>\atm` (no uid suffix — `%TEMP%` is already
  per-user on Windows).

Falling back emits exactly one `tracing::warn!` line naming the default path
and the `ATM_TEMP` override, mirroring the existing optional-config warning
style at `crates/atm-daemon-bootstrap/src/lib.rs:636`. This is what keeps
every already-installed daemon, the `daemon-switch` overlay sessions, the m5
Hermes fleet daemon, and any direct `cargo test` invocation booting and
passing unmodified: none of them sets `ATM_TEMP` today, and none needs to.

**Shared-host safety (critical review F7).** If the resolved directory
(fallback or an explicit `ATM_TEMP`) already exists and is not owned by the
current uid, or has any group or world permission bit set, resolution fails
closed with `AtmTempInsecure` rather than adopting a directory another user
could have pre-created. The same check applies uniformly to the default
fallback path and to an explicit `ATM_TEMP` value — there is one security
check, not two.

**Set-but-invalid still fails closed.** An explicit `ATM_TEMP` that is
relative, unresolvable (broken symlink / missing parent), or unwritable
fails daemon boot and fails the CLI at first scratch use with an actionable
error. An operator who explicitly set `ATM_TEMP` gets a real error instead of
a silent fallback to a directory they did not choose.

This sprint deliberately does **not** add `ATM_TEMP` to CI workflows,
justfile test targets, or daemon launch overlays. The default makes that
unnecessary for correctness; an explicit `ATM_TEMP` remains an available
operator convenience, never a requirement this ADR imposes.

```rust
pub struct AtmTemp(PathBuf); // constructed only by resolve_atm_temp

pub trait EnvSource {
    fn var(&self, key: &str) -> Option<String>;
}

/// Unset -> Ok(default path, warning logged by the caller).
/// Set-but-invalid -> Err. Daemon calls this once at startup; CLI calls it
/// lazily at first scratch use. One function, one default, one error
/// family -- the single ATM_TEMP read site.
pub fn resolve_atm_temp(env: &dyn EnvSource) -> Result<AtmTemp, AtmTempError>;
```

| `AtmTempError` | Cause | Recovery |
|---|---|---|
| `NotAbsolute` | `ATM_TEMP` set to a relative path | use an absolute path |
| `Unresolvable` | canonicalization failed | check broken symlinks / missing parents |
| `NotWritable` | not writable/creatable | fix permissions or pick another dir |
| `AtmTempInsecure { path, reason }` | the resolved directory (fallback or explicit `ATM_TEMP`) exists but is not owned by the current uid, or has group/world permission bits | remove/chown the directory or set `ATM_TEMP` to a private path |
| `TransferScriptUnsafe { host, reason }` | script at `~/.atm/transfer/<host>` exists but fails the executable-bit/owner-UID/not-group-or-world-writable check | `chmod 700 ~/.atm/transfer/<host>` and confirm ownership |

`Unset` is intentionally not a variant: an unset `ATM_TEMP` resolves to the
default scratch root, it does not error.

### `resolve_atm_temp` is lint-clean by construction (M14)

`resolve_atm_temp` reads `ATM_TEMP` as `env.var("ATM_TEMP")` — a **method
call** on the `EnvSource` trait object, not the free-function path form
(`env::var(...)` / `std::env::var(...)`) that `.just/check_env_var_boundary.py`'s
`ENV_CALL_RE` matches (anchored on a literal `::`). No allowlist entry or
`boundary_reader_functions` registration is required or appropriate: the
`boundary_reader_functions` list's actual effect is the opposite of an
exemption — every call to a *listed* function from another file in a
restricted crate root is flagged. `ATM_TEMP` is still added to
`forbidden_env_vars` in `.just/lint-config.toml` as defensive coverage: it
costs nothing today (there is no offending call site to allowlist) and
catches a future direct `env::var("ATM_TEMP")` call added outside the
`EnvSource` seam.

### (b) Sweep policy: TTL-only, 30 days, no ack coupling

A periodic sweeper removes everything under `$ATM_TEMP` older than 30 days
(`sweep_ttl_days`, configurable). No ack coupling, no storage traits — a
staged attachment for an unread message past the TTL may be reclaimed before
a deferred nudge fires; this is an ordinary missing-file case, not a
correctness bug, and is accepted risk (tracked as an open item for a later
sprint to reconsider 30-days-unread instead of 30-days-staged).

The sweeper is the first periodic task composed against the replacement
Tokio/Axum runtime (`atm-daemon-bootstrap`), not the legacy synchronous
daemon's maintenance worker, which CLAUDE.md rules off-limits for new work.
It follows the existing cancel-then-join-within-a-bounded-deadline shape
already used for `WorkflowTelemetryRuntime::shutdown`
(`crates/atm-runtime/src/workflow_telemetry.rs:171`): a shutdown signal is
sent, the worker is given its own bounded grace period to finish an
in-flight per-entry removal, and only after that grace period expires is the
task aborted — and even then, the abort is always joined. A raw
`.abort()`-only shutdown (as already used for the HTTP server task,
`crates/atm-http-runtime/src/lib.rs:964,2026`) does not guarantee an
in-progress sweep pass leaves the filesystem in a consistent state, so the
sweeper does not use that shape.

Sweep interval and TTL are new `AtmConfig` fields (`sweep_interval_seconds`,
default 3600; `sweep_ttl_days`, default 30). Zero for either is a config
error, but — mirroring decision (a)'s "unset is not a failure" rule — that
error is only reachable once `ATM_TEMP` itself resolves (default or
explicit); a zero-configured sweeper never blocks daemon boot on its own,
because it is only constructed after `resolve_atm_temp` succeeds.

Per-entry removal failures are skipped and logged, not pass-fatal.
`SweeperError` is reserved for root-condition failures (the scratch root
itself missing or unreadable). Each completed pass emits one structured
`tracing::info!` event carrying `subsystem`, `action`, `outcome`, `scanned`,
`reclaimed_bytes`, and `skipped`, per the ATM daemon observability
convention (`.claude/skills/rust-development/guidelines.txt`'s "ATM daemon
advisory").

The sweeper never follows a symlink found under `$ATM_TEMP` into whatever it
targets. Symlinked entries are aged and reclaimed using their own
(`lstat`-observed) modification time, exactly like any other entry, and are
never traversed into — this makes "never follows symlinks out of the root"
true by construction rather than by an extra traversal guard.

### (c) Transfer-script seam

Cross-host bytes move via a per-destination-host user script, resolved as:

- Windows: `<host>.ps1` under `~/.atm/transfer/`, invoked as
  `pwsh -File <script> <host> <transfer-id> <file>...`.
- macOS/Linux: the extensionless `~/.atm/transfer/<host>`, invoked directly
  as `<script> <host> <transfer-id> <file>...`.

Both forms use **argv-array exec** — never a shell-interpolated string join
(no `sh -c`, no `cmd /c`). `<host>` is the caller-resolved `HostName`
(`crates/atm-storage/src/types.rs:304`), which already rejects `/`, `\`, and
`..` by construction: `HostName::from_str` splits on `.` and runs every
label through `validate_path_segment` (`crates/atm-storage/src/validation.rs:3`),
which accepts only ASCII alphanumerics, `-`, and `_` per label (an empty
label — which a leading/trailing/doubled `.` would produce — fails the
non-empty check). Building the script path from an already-typed `HostName`,
never a raw string, means the script-path lookup needs no separate
traversal guard.

On Unix, before exec, the resolver requires the script to be
**owner-executable, owned by the invoking process's UID, and not group- or
world-writable** — the same check shape already used for the daemon's UDS
socket path (`is_owned_by` and `parent_is_writable_by_others`,
`crates/atm-http-runtime/src/unix_socket.rs:73,83`, mode `& 0o022`, widened
here to reject group-write, world-write, *or* group/world-read of an
executable script: `mode & 0o077 != 0`). Failing any of the three checks
refuses the transfer with `TransferScriptUnsafe { host, reason }` — not the
"not enabled" error. The script exists but is not safe to run, which is a
materially different, and more urgent, operator signal than "not configured
yet."

The child process inherits **only** `ATM_TEMP`, `ATM_IDENTITY`, and
`ATM_TEAM` from the caller's environment (an explicit allow-list, never the
full parent environment), runs with the caller's cwd, and has stdin closed
(no accidental interactive-prompt hang). It has a bounded deadline (default
60 s, configurable; the child is killed on expiry) and capped stdout/stderr
(truncated with a marker). Success is a single-line absolute-path landed
directory on stdout, validated as **untrusted input** (exactly one line,
absolute, no control characters); a nonzero exit propagates stderr to the
user (bounded). A missing script produces the canonical, verbatim error:

```text
File transfer to <host> not enabled. Read docs/cross-host-file-transfer.md
to set up cross-host file transfer.
```

Any transfer failure fails the whole invocation closed — zero messages
sent. This ADR defines script resolution, the safety check, and the argv/env
contract only; wiring the resolved script into an actual child-process exec
from `atm send` is deliverable 3 of the full AQ4 sprint (lane C explicitly
excludes `crates/atm/src/commands/send.rs` and the `--attach` CLI surface).

### (d)–(g): message-text convention, member-host sourcing, local-host identity, fan-out failure policy

These four decisions govern the CLI/roster surface (`atm send --attach`,
`--from-json`, `atm teams --json --members`) and are out of this lane's
scope by construction — lane C is "AQ4 minus `send.rs`." They are recorded
here only for cross-reference completeness and are authoritative in
`docs/plans/phase-aq/sprint-AQ4-send-to-core.md`, not in this ADR:

- (d) Landed paths ride in message text; no envelope change in Phase 1.
- (e) Member-host sourcing is a durable, explicit roster field
  (`teams add-member/update-member --host`), never inferred.
- (f) Same-host vs. remote classification adds `AtmConfig.local_host` and
  compares it against the resolved recipient's host.
- (g) A transfer or send failure partway through a `--from-json` fan-out
  aborts all remaining not-yet-attempted hosts and reports a partial result;
  bytes already staged remotely for an abandoned host are not rolled back
  and age out under the ordinary sweep TTL.

## Consequences

- Every installed daemon, the `daemon-switch` overlay sessions (ADR-053),
  the m5 Hermes fleet daemon, and any direct `cargo test` invocation keep
  booting/passing unmodified: none of them sets `ATM_TEMP`, and the default
  resolves cleanly without one.
- `atm-core` gains its first production `std::env::temp_dir()` use and its
  first system-level (non-CLI-identity) environment-variable contract,
  behind one sealed, testable read site.
- The scratch root's shared-host safety property (owner-uid + no group/world
  bits) is checked identically whether the path came from the default or
  from an explicit `ATM_TEMP` — one code path, not two.
- The transfer-script safety check reuses the same shape as the daemon's
  existing UDS-safety check, so a reviewer auditing "does ATM ever execute a
  file it doesn't fully trust the permissions of" has one pattern to verify,
  not two independently invented ones.
- The sweeper is the first periodic task in the replacement Tokio/Axum
  runtime, establishing the cancel-then-join-within-deadline precedent for
  future daemon maintenance tasks, rather than reviving the legacy
  synchronous daemon's maintenance worker.

## Rejected alternatives

1. **Make `ATM_TEMP` a hard boot requirement.** Rejected (critical review
   B10): breaks every already-installed daemon, the `daemon-switch` overlay
   sessions, the m5 fleet daemon, and any direct `cargo test` invocation the
   day this ships, for a feature (Send-To) most of those processes never
   use.
   Considered mitigation — add `ATM_TEMP` to CI/justfile/launch overlays —
   rejected as unnecessary scope creep once the default makes it moot.
2. **Adopt a pre-existing scratch directory unconditionally.** Rejected
   (critical review F7): a shared-host attacker can pre-create the default
   path; adopting it unconditionally would let another local user observe
   or tamper with staged file transfers.
3. **A pull-based transfer design (fetch endpoints, delivery-gating,
   attachment storage traits).** Rejected 2026-08-23 by Rand as daemon
   state-machine complexity for a script-sized problem; retained only in
   git history and PRD Non-closure.
4. **Shell-interpolated script invocation (`sh -c "$script $host ..."`).**
   Rejected: string interpolation of a hostname or transfer id into a shell
   command is an injection surface even when the inputs are validated
   elsewhere; argv-array exec removes the class of bug entirely.
5. **Treat an unsafe-permission transfer script the same as a missing one.**
   Rejected: silently running an insecure script is worse than refusing it,
   and collapsing the two cases into one error would hide an actionable
   "this script is not safe" signal behind the generic "not configured yet"
   setup message.

## Required evidence

- Unit tests for every `AtmTempError` table row, including the shared-host
  safety scenario (pre-existing `0755` directory refused; pre-existing
  `0700`-own-uid directory accepted) and the Windows default-path naming
  branch (exercised via a fake `EnvSource` and platform-parameterized path
  selection, not gated behind `#[cfg(windows)]`, so it runs on every CI
  lane).
- Unit tests for the transfer-script safety check's three independent
  refusal reasons (not owner-executable, not owned by the caller's UID,
  group/world-writable), each producing `TransferScriptUnsafe` with a
  distinct `reason`, plus the missing-script (`None`) case staying distinct
  from the unsafe-script case.
- Sweeper unit tests with a temporary directory and an injected clock:
  expired entries reclaimed, fresh entries kept, a symlink is never
  traversed into, and a per-entry removal failure is skipped rather than
  aborting the pass.
- `cargo test` with `ATM_TEMP` unset in the test process environment passes
  unchanged on the pre-existing suite plus this ADR's own tests.
