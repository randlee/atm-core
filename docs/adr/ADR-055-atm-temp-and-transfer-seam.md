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

**Documented deferral (QM43-I5, explicit non-closure).** `AtmConfig`'s
`sweep_interval_seconds`/`sweep_ttl_days` fields fully parse from
`.atm.toml` through `load_config` today, but the replacement daemon's
composition path does not yet call `load_config` for this purpose —
`start_atm_temp_sweeper` (`crates/atm-daemon-bootstrap/src/lib.rs`)
constructs the sweeper from `AtmConfig::default()`'s compiled-in values
(1 hour / 30 days) unconditionally, because `assemble_daemon_runtime`
deliberately does not depend on a workspace-relative `.atm.toml` (a
LaunchAgent may start with a `getcwd(2)`-blocking working directory; see
that function's own doc comment). This is a known gap, not missed
follow-through: threading a real operator-configured interval/TTL into the
daemon requires resolving *which* `.atm.toml` a system daemon should read
without a workspace context at all, a design question this ADR does not
settle. It is deferred to AQ4 proper, alongside the CLI first-use call site
(`resolve_atm_temp`'s lazy CLI path, decision (a)) that will need the same
config resolution answer. Until then, every replacement daemon runs the
sweeper at the compiled-in defaults regardless of `.atm.toml`.

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

**Concurrent-writer safety (QM43-I6).** A single mtime-only expiry check is
not enough: it does not by itself protect a file that lands via an atomic
rename, whose destination inode gets a fresh `ctime` from the rename itself
even when the file's *content* `mtime` is already old (for example,
preserved from a remote source the transfer script copied from). The
sweeper therefore requires **both** the entry's `mtime` and its `ctime` to
independently be at least `ttl` old before reclaiming it
(`atm_core::atm_temp_sweeper::EntryAge`/`is_expired`); either signal alone
being fresh keeps the entry. On a platform with no ctime-equivalent, the
mtime-only signal decides on its own. The sweeper additionally never
reclaims an entry with an `<entry-name>.inprogress` sibling marker file,
regardless of age — the seam a future writer (`atm send --attach`'s landing
path, a later sprint) uses to protect an in-flight write beyond what the
mtime/ctime check alone covers. **Residual window:** this is still a
best-effort guard, not a lock — a writer that neither uses an atomic rename
nor creates the `.inprogress` marker, and whose write spans a TTL-length
stall with no metadata touch in between, is not protected. The sweeper does
not take out a file lock (`flock`) on every candidate before reclaiming it;
doing so would require every producer (transfer scripts, `--attach`) to
also cooperate with the same lock protocol, which is out of this sprint's
scope (deliverable 3, lane C excluded).

**Cancellable, chunked passes (QM43-I7).** A `spawn_blocking`-hosted sweep
pass is not itself cancellable by tokio: an unconditional `.abort()` only
detaches the outer task wrapper, letting the blocking closure keep deleting
files in the background after `shutdown()` returns, which breaks the
"leaves the filesystem in a consistent state" guarantee decision (b)
otherwise makes. The sweep pass therefore polls a shared cancellation flag
once per entry (`atm_core::sweep_once_cancellable`) — a chunk size of one
entry — so `AtmTempSweeperRuntime::shutdown` can set it immediately and
have an in-flight pass observe it within a bounded number of entries,
rather than the shutdown grace period needing to cover an entire,
potentially unbounded, remaining walk.

**Observability (QM43-I4).** Each pass emits the existing structured
`tracing::info!`/`tracing::warn!` events described above, and additionally
reports through `atm_core::observability::ObservabilityPort` — the same
seam already threaded through `atm-daemon-bootstrap`'s composition function
and consumed by `atm-http-runtime::storage_and_nudge_router.rs`, not the
legacy daemon's `emit_daemon_event` — mirroring the existing
`record_peer_wire_mode_selection` precedent: the event is emitted only when
the daemon's launch identity supplies a team and agent to attribute it to
(`CommandEvent` has no optional-attribution shape), and the tracing event is
the fallback when it does not. This is what makes a persistently failing
sweeper visible on the daemon's retained observability/health surface, not
only by log-grepping.

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
(no accidental interactive-prompt hang).

**QA-2 B6 correction (recorded post-merge):** the allow-list gained a
fourth, opt-in entry, `ATM_TRANSFER_SSH_CONFIG`. Unset for every ordinary
operator (identical behavior to the original three-entry list), it exists
so `sftp.sh`/`sftp.ps1` can pass `ssh -F <path>`/`scp -F <path>` when a
caller sets it, without needing any of the argv-array invocation contract
above to change. Its only real consumer today is
`scripts/phase-aq/run_aq4_transfer_evidence.py`'s live-evidence harness,
which uses it to route a loopback `sshd` through a scratch config file
instead of mutating the OS account's real `~/.ssh/config` -- a harness
correctness fix (QA-2 B6, "serious"), not a change to what any shipped
example script does by default. `TRANSFER_SCRIPT_ALLOWED_ENV_KEYS`
(`crates/atm-core/src/transfer_script.rs`) is the single source of truth.

It has a bounded deadline (default
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

### Amendment (2026-08-27): the Windows path-safety check

Windows CI first exposed a build break (an unconditional `#[cfg(unix)]`
import used outside its guard), and fixing it surfaced that the transfer-
script safety check's Windows branch had been shipping as a documented
no-op ever since decision (c) above was written: no reparse-point check, no
containment check, nothing beyond "is this a file/directory." This
amendment records the real Windows check that replaced it, and the design
mistake a follow-up review caught before it reached that decision.

**What the Windows check actually verifies**, for both the script file and
its containing `~/.atm/transfer` directory:

1. **Not a reparse point.** An NTFS symlink or junction is refused outright
   (`TransferScriptUnsafe`) — it could point somewhere else entirely by the
   time it is used, defeating every check that follows. Detected from the
   same never-following `symlink_metadata` call this module already uses
   everywhere else, so this costs no extra filesystem round trip and keeps
   the "never follow a symlink out of the checked location" discipline
   uniform across the whole module.
2. **Path containment under the resolved home.** The path must sit under a
   `profile_home` directory, compared component-by-component
   (`Path::components()`, not a raw string prefix — a sibling directory
   sharing a string prefix, e.g. `C:\Users\rand` vs. `C:\Users\randlee`,
   must not pass a naive check, and a verbatim `\\?\`-prefixed path must
   compare equal to its non-verbatim spelling of the same drive/UNC
   location, not mismatch because only one side was `canonicalize()`d).

**The mistake a review caught, and the fix.** The first version of this
check resolved `profile_home` via `home::os_account_home` (the
`SHGetKnownFolderPath` known-folder API) unconditionally — the same source
`current_host_runtime_scope` uses for host-runtime ownership, which
deliberately ignores `$HOME`/`%USERPROFILE%` by design. But
`transfer_script_root` (the function that builds the path this check then
validates) resolves the transfer root via `home::resolve_user_home_via`,
which *does* honor an explicit `$HOME`/`%USERPROFILE%` override. Checking
containment against a *different* home resolution than the one that built
the path being checked meant a legitimate override was silently rejected:
`transfer_script_root` would place `~/.atm/transfer` under the override,
and the Windows check would then refuse it for sitting outside the
unrelated OS-account profile. The fix: `profile_home` is always resolved
the same way `transfer_script_root` resolves it first
(`resolve_user_home_via`), falling back to `os_account_home` only when
neither `$HOME` nor `%USERPROFILE%` is set, and failing closed
(`TransferScriptUnsafe`) if neither source resolves at all — never
silently skipping the containment check.

**Testability.** The comparison logic (reparse refusal plus
component-aware, verbatim/UNC-normalizing containment) is factored into a
pure function taking plain data — a path, a profile-home path, and a
`bool` for "is this a reparse point" — with no filesystem or WinAPI I/O of
its own. It is compiled and unit-tested on every CI platform, not only
Windows, specifically so a mistake like the one above (a check whose
*logic* is fine but whose *inputs* are wrong) has a cheap, fast,
cross-platform test surface instead of depending on Windows CI alone to
catch it.

**Unix/Windows asymmetry, and what remains deferred.** Unix's check (mode
bits + owner uid, `mode & 0o077`, decision (c) above) and Windows's check
(reparse refusal + profile containment) are *not* parallel implementations
of one shared policy — they are the best achievable check for each
platform's actual security model. Windows has no POSIX mode-bits/owner-uid
concept to port; Unix has no NTFS reparse-point or known-folder-profile
concept. **Full Windows ACL inspection (who besides the current user has
write access to the script or its directory) is explicitly deferred, not
silently assumed closed.** Unlike a Unix mode integer, a Windows ACL has no
single-comparison shape to check generically — enumerating and evaluating
an arbitrary ACL correctly (inherited entries, deny-before-allow ordering,
well-known SIDs) is real, scoped work this sprint does not include. This
mirrors decision (a)'s own Windows scratch-root check
(`validate_existing_scratch_dir`), which likewise performs no ACL check and
documents why rather than pretending the gap is closed.

### Amendment (2026-08-27): synthesized transfer-script `PATH`

Windows clean-runner CI (run 33135390308) surfaced the second real gap
decision (c)'s child-environment contract shipped with: `atm send --attach
--host localhost` installed `sftp.ps1`, the transfer-script safety check
passed, `pwsh -File sftp.ps1 ...` started, and the script's own `ssh`
invocation then failed with `The term 'ssh' is not recognized...`. The
cause was the allow-list itself, not the script: `TRANSFER_SCRIPT_ALLOWED_
ENV_KEYS` never included `PATH` (correctly — forwarding the caller's real
`PATH` would leak whatever a developer's shell profile happens to have on
it, exactly the ambient-authority leak the allow-list exists to refuse for
every other variable), but `Command::env_clear()` followed by *no* `PATH`
at all left the child unable to resolve `ssh`/`scp` by name on Windows.

Unix had the identical gap in the allow-list and never noticed, for a
platform reason, not a design one: POSIX `execvp` falls back to a
`confstr(_CS_PATH)`-provided default search path (`/bin:/usr/bin` on most
Unix libc implementations) when `PATH` is entirely absent from the child's
own environment — this is what let `sftp.sh`'s bare `ssh`/`scp` calls keep
resolving by accident. Windows has no equivalent fallback for a command
lookup performed by the *child* process itself (as distinct from the
initial `CreateProcess` search for `pwsh.exe` itself, which uses the
*calling* process's own `PATH`, not the child environment this contract
builds — that part already worked, which is why `pwsh` started at all
before failing inside the script).

**The fix: the allow-list stays exactly as it was — `PATH` is never added
to it, and the caller's real `PATH` is never forwarded to the child, on
any platform. Instead, a synthesized, deliberately narrow `PATH` is set
alongside the allow-listed variables** (`atm_core::transfer_script::
synthesized_transfer_script_env`, applied in `crates/atm/src/commands/
send_to.rs::invoke_transfer_script` after `Command::env_clear()`, alongside
— never instead of — the `TRANSFER_SCRIPT_ALLOWED_ENV_KEYS` loop):

- **Unix**: `/usr/bin:/bin:/usr/local/bin`, plus `/opt/homebrew/bin` on
  macOS — the fixed set of directories the shipped `sftp.sh`/`tailscale.sh`
  examples' `ssh`/`scp` calls need, matching (and now making explicit,
  rather than incidental) the platform's own `execvp` fallback.
- **Windows**: `%SystemRoot%\System32;%SystemRoot%\System32\OpenSSH`, plus
  `pwsh`'s own directory when it can be located (best-effort, searched
  through the *caller's* `PATH` purely to find that one directory — that
  lookup value itself is never forwarded onward as a whole, only the one
  resolved directory it names). Windows additionally carries `SystemRoot`/
  `SYSTEMROOT` (the .NET/PowerShell host needs one of these to start at
  all) and, when the caller has one, `TEMP` (`pwsh`'s own temp-file needs)
  — all sourced from the caller's real environment through the same
  `EnvSource` seam `resolve_atm_temp` uses, never hardcoded, so an
  operator's actual Windows install layout is respected without ever
  forwarding their full `PATH`.

`scripts/transfer/sftp.ps1` was hardened to match: rather than a bare `ssh`/
`scp` call (which depended on whatever `PATH` its host process happened to
receive), it now resolves both explicitly via `Get-Command -CommandType
Application`, falling back to `$env:SystemRoot\System32\OpenSSH\ssh.exe`/
`scp.exe` when `Get-Command` finds nothing, and fails closed with a clear
stderr message (never a bare "term not recognized") when neither source
resolves. The invocation contract (argv-array exec, `['ssh', 'scp']`) is
unchanged — only how the script locates those two binaries changed.

`crates/atm-core/src/transfer_script.rs`'s unit tests assert the exact
synthesized `PATH` per platform and, independently, that a distinctively
marked caller `PATH` never appears anywhere in the synthesized result —
the "never inherited" half of this amendment's rule is tested directly,
not just documented. `scripts/phase-aq/run_aq4_transfer_evidence.py`
additionally records a Python mirror of the synthesized value under
`record["transfer_script"]["synthesized_env"]` in its evidence JSON, purely
for diagnosability (it is never asserted against — only
`install_transfer_script`'s existing safety-check mirrors gate the
scenario), so a future regression like this one is visible in the evidence
transcript itself rather than requiring a second investigation to
reconstruct what the child's environment actually was.

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
  aborting the pass; an entry whose mtime looks old but whose ctime is
  fresh is not reclaimed, and an entry with an `.inprogress` marker is
  never reclaimed regardless of age (QM43-I6); a cancellation flag set
  mid-pass stops the walk before every entry is visited, and (at the
  `atm-daemon-bootstrap` runtime layer) `shutdown()` returns within its
  bounded grace period against a simulated slow filesystem instead of
  waiting for a full, uncancelled pass to finish (QM43-I7).
- `cargo test` with `ATM_TEMP` unset in the test process environment passes
  unchanged on the pre-existing suite plus this ADR's own tests.
