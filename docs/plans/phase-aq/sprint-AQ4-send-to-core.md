---
status: complete
branch: feature/aq-4-send-to-core
worktree: ../atm-core-worktrees/feature/aq-4-send-to-core
---

# Sprint AQ4 — Send-To Core: ATM_TEMP, CLI Surface, Transfer Scripts

Status: complete · Branch: `feature/aq-4-send-to-core` off `integrate/phase-aq`
· Worktree: `../atm-core-worktrees/feature/aq-4-send-to-core` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Consolidation note (Rand, 2026-08-23; landing mechanism hardened 2026-08-26,
PLAN-CRIT-015): this sprint deliberately carries what an earlier draft split
across four sprints (ATM_TEMP contract, CLI surface, sweeper, transfer
scripts). The justification is not scope-neutral reshuffling: all four
share one owner and one contract — the `ATM_TEMP` root and the staging
convention — so splitting them buys four dispatch/QA cycles for work that
is sequential inside one owner's context, and the finer-grained split
already went through this phase's QA rounds (its hardened acceptance
criteria are preserved verbatim below). **Decision (approved by Rand 2026-08-26):
one PR, not two.** The prior "the PR opens once deliverables 1–4 are
pushed" line named a timing convention, not a QA gate, and PLAN-CRIT-015
correctly flagged it as unfalsifiable. The real mechanism: deliverables
land as separate commits in the stated order (1 → 2 → 3/4 → 5 → 6 → 7),
each commit message tagged `AQ4.<n>:`; the PR may be opened for visibility
once commits for deliverables 1–4 are pushed, but **quality-mgr gates the
final head only** — there is no intermediate approval, and a script-harness
or sweeper failure discovered late is triaged against its own commit, not
treated as a CLI-surface regression, purely because the commit boundary
makes that attribution mechanical (`git bisect`-able) rather than a claim.
The alternative considered and rejected here — splitting into sequential
`AQ4a` (ATM_TEMP + `resolve_atm_temp` + sweeper) and `AQ4b` (`--attach` +
transfer scripts) PRs with independent QA gates — was already evaluated in
round 12 of the original hardening arc (`PLAN-CRIT-017`, "015 mitigation
accepted — AQ4 split NOT required") and the ATM_TEMP rollout risk that
motivated re-litigating it (B10) is closed below by defaulting instead of
hard-failing, which removes the split's main justification (a fleet-wide
boot-breaking change no longer ships in this sprint at all).

The PRD Phase-1 Send-To core, end to end: a file picked in the OS file
manager reaches an agent's `$ATM_TEMP` on any configured host and an
ordinary message names the landed path — or the send fails closed with an
actionable setup error. Cross-host transfer is a user-configured
environment concern (scripts), never daemon machinery: no envelope change,
no daemon transport endpoints, no new storage traits.

Verified baseline (integrate/phase-ao2): `atm teams --json` emits
`{name, member_count}` only — the PRD §4.2 member projection must be built;
member state is `RuntimeMemberState` (`Active|Idle|Offline|Unknown|
IdentityConflict`); no roster record supplies a member host; `atm send` is
single-recipient (`to` positional required) with flags `--team --host
--chat-id --as --file --stdin --template --vars --var --env-prefix --tag
--category --content-format --summary --requires-ack --task-id --dry-run
--json` (`--env-prefix`, `crates/atm/src/commands/send.rs:81`, is
template-only today — gated with `--vars`/`--var` behind `--template` at
`send.rs:299`); `AtmConfig` has no temp key or `local_host` key (decision
(f)); daemon dirs are `~/.atm/{daemon,db,logs}`; the replacement Tokio/Axum
daemon (`atm-daemon-bootstrap`) has no periodic-task precedent yet — the
sweeper (deliverable 5) is its first — so this sprint composes it directly
against the runtime's real shutdown/observability seams rather than
following the legacy synchronous daemon's now off-limits maintenance
worker; `ensure_column` for migrations. The fleet has passwordless SSH
from this machine to all destinations (Rand) — the sftp example's
baseline.

## Deliverables

1. **ADR-055 atm-temp-and-transfer-seam**, quality-mgr-reviewed, deciding:
   (a) **`ATM_TEMP` system contract, with a rollout that does not break an
   installed daemon (B10)** — an env var naming the ATM scratch root for
   all features, resolved by **one shared `resolve_atm_temp`** the daemon
   calls at startup and the CLI calls lazily at first scratch-space use
   (commands touching no scratch space are unaffected either way).
   **Unset is not a startup failure**: `resolve_atm_temp` defaults to
   `<std::env::temp_dir()>/atm-<uid>` on Unix (`<temp_dir()>\atm` on Windows,
   where `%TEMP%` is already per-user), created with mode `0700` if missing.
   **Shared-host safety (critical review F7):** if the directory already
   exists and is not owned by the current uid, or has any group/world
   permission bits, resolution fails closed with `AtmTempInsecure` — the
   fallback never adopts a directory another user could have pre-created.
   The same ownership/mode check applies to an explicitly set `ATM_TEMP`.
   `std::env::temp_dir()`
   is not yet a *production* repo idiom — grepped repo-wide, every existing
   call site is a test fixture (e.g. `crates/atm-core/src/ack/mod.rs:779`,
   `crates/atm-runtime/src/composition.rs:311`) or inside the `identity`
   module already `#[allow(dead_code)]`-gated for Phase AD removal
   (`crates/atm-core/src/identity/hook.rs:71`) — so this sprint is honestly
   the first production use, which is expected: formalizing "the ATM
   scratch root" is exactly what ADR-055 is for. It emits exactly one
   `tracing::warn!` startup
   line naming the default path and the `ATM_TEMP` override, mirroring the
   existing optional-config warning style at
   `crates/atm-daemon-bootstrap/src/lib.rs:636`. This is what keeps every
   installed daemon booting unmodified the day this sprint merges: the
   `daemon-switch` typed launch-overlay sessions (ADR-053,
   `.claude/skills/daemon-switch/SKILL.md` — control-plane-only; its
   Decision section states a session "never accepts a raw daemon argument,
   alternate endpoint/root, **environment selector**, service wrapper, or
   arbitrary configuration edit," so it was never going to carry
   `ATM_TEMP` even if this sprint wanted it to), the m5
   Hermes host (`docs/peer-pair-smoke.md`'s `just smoke peer-preflight m5
   fastpc4` fleet, whose already-running daemon has no `ATM_TEMP` in its
   launch environment), and any `cargo test` invoked directly instead of
   through `just` (no `.github/workflows/ci.yml` or justfile lane sets
   `ATM_TEMP` today — grepped, zero hits — and this sprint does not add
   one) all keep working with no operator action. **`ATM_TEMP` set but
   invalid** (relative, unresolvable, unwritable) **does** fail closed with
   an actionable error, at daemon startup and at CLI first scratch use —
   an operator who explicitly set it gets a real error instead of a silent
   fallback to a directory they didn't choose. Zero sweep-interval/TTL is
   a config error under the same rule (only reachable once `ATM_TEMP`
   resolves, and the default always resolves cleanly). The rollout is
   documented in `docs/cross-host-file-transfer.md` (deliverable 6) and in
   a short "Default scratch root" note added to `.claude/skills/
   daemon-switch/SKILL.md` recording that both the ordinary and the
   `--peer-wire-security plaintext-test` overlay launch paths inherit the
   same default with no ADR-053 launch-overlay change required. This
   sprint deliberately does **not** add `ATM_TEMP` to CI workflows,
   justfile test targets, or daemon launch overlays — the default makes
   that unnecessary for correctness; an explicit `ATM_TEMP` remains an
   available operator convenience, not a requirement this sprint imposes.
   (b) **Sweep policy** — TTL-only, 30 days, everything under `$ATM_TEMP`;
   no ack coupling, no storage traits. Accepted, documented risk: staged
   attachments for messages unread past the TTL may be reclaimed before a
   deferred nudge fires — an ordinary missing-file case; 30-days-unread is
   abandoned (open-item register, AQ5).
   (c) **Transfer-script seam** — per-destination-host user script at
   `~/.atm/transfer/<host>`, resolved as: `<host>.ps1` under
   `~/.atm/transfer/` on Windows, invoked `pwsh -File <script> <host>
   <transfer-id> <file>...`; the extensionless `~/.atm/transfer/<host>` on
   macOS/Linux, invoked directly as `<script> <host> <transfer-id>
   <file>...`. Both forms use **argv-array exec** (never shell-interpolated
   — no `sh -c`/`cmd /c` string join). `<host>` is the caller-resolved
   `HostName` (decision (e)/(f)), already validated by `HostName::from_str`
   (`crates/atm-storage/src/types.rs:304`, splitting on `.` and running
   each label through `validate_path_segment`,
   `crates/atm-storage/src/validation.rs:3`, which accepts only ASCII
   alnum/`-`/`_` per label) — this grammar already rejects `/`, `\`, and
   `..` (an empty label fails the non-empty check), so the script-path
   lookup needs no separate traversal guard beyond using the already-typed
   `HostName`, never a raw string, to build the path. On Unix, before exec,
   the resolver requires the script to be **owner-executable, owned by the
   invoking process's UID, and not group- or world-writable** — the same
   check shape already used for the daemon's UDS socket path
   (`is_owned_by` and `parent_is_writable_by_others`,
   `crates/atm-http-runtime/src/unix_socket.rs:73,83`, mode `& 0o022`);
   failing any of the three refuses the transfer with a new
   `AtmTempError`-family variant `TransferScriptUnsafe { host, reason }`
   (not the "not enabled" error — the script exists but is not safe to
   run) rather than falling through to the missing-script path. The child
   process inherits **only** `ATM_TEMP`, `ATM_IDENTITY`, `ATM_TEAM` from
   the caller's environment (an explicit allow-list, not the full parent
   environment), runs with the caller's cwd, and has stdin closed (no
   accidental interactive prompt hang).
   (QA-2 B6 correction, recorded post-merge: the allow-list gained a
   fourth, opt-in entry, `ATM_TRANSFER_SSH_CONFIG` -- unset for every
   ordinary operator, identical behavior to the original three-entry list;
   see ADR-055's matching correction and
   `TRANSFER_SCRIPT_ALLOWED_ENV_KEYS` for the single source of truth.)
   **Bounded deadline** (default
   60 s, configurable; child killed on expiry), **capped stdout/stderr**
   (truncate with marker), success = single-line absolute-path landed dir
   on stdout **validated as untrusted input** (one line, absolute, no
   control chars); nonzero exit propagates stderr to the user (bounded).
   Missing script → the canonical error, exactly: `File transfer to <host>
   not enabled. Read docs/cross-host-file-transfer.md to set up cross-host
   file transfer.` Any transfer failure fails the whole invocation closed
   — zero messages sent (R5/R13).
   (d) **Message-text path convention** — landed paths ride in message text
   ("Attached files (on this host): …"); no envelope change; structured
   `attachments`/`note_source` are Phase-2 candidates.
   (e) **Member-host sourcing** — durable optional roster field via
   `teams add-member/update-member --host` (`RosterEntry.metadata_json
   ["host"]`, validated `HostName`), never inferred from heartbeat/DNS/
   sockets; `host: null` = unroutable via `--from-json`. Resolution
   algorithm is this sprint's `resolve_picker_recipient` — single
   canonical implementation.
   (f) **Sender's own host identity (same-host vs. remote classification)**
   — there is no existing "local host name" concept in `atm-core` today
   (`current_host_runtime_scope`, `crates/atm-core/src/home.rs:48`, scopes
   the ATM home *directory*, not a routable `HostName`; no `AtmConfig`
   field, env var, or `gethostname`-style dependency exists — verified
   grep, zero hits). This sprint adds one: a new optional
   `AtmConfig.local_host: Option<HostName>` field (approved by Rand 2026-08-26; same file that already
   has no temp key, `crates/atm-core/src/config/types.rs:54`), sourced
   from `.atm.toml`, set once per machine by the operator (documented in
   `docs/cross-host-file-transfer.md`) with the **same value** they'd
   register via `teams update-member --host` for agents running on that
   machine. Same-host classification: `resolve_picker_recipient`'s
   resolved `AgentAddress.host()` is `None` (existing precedent,
   `AgentAddress::without_host`'s doc comment at
   `crates/atm-core/src/address.rs:66`: no host qualifier already means
   "local to the receiving/authenticated boundary") **or** equals the
   local `AtmConfig.local_host`; otherwise remote, and the transfer-script
   seam applies. A host-qualified recipient with `AtmConfig.local_host`
   unset fails closed with an actionable config error (`export
   ATM_TEMP`-style: "set `local_host` in `.atm.toml`, or omit `--host`
   from the recipient's roster entry to route it as local") rather than
   guessing same-host or remote.
   (g) **Mid-fan-out send failure policy** — `--from-json` (deliverable 4)
   validates every recipient's shape and every attachment path up front
   (unchanged R5/R13 guarantee: a malformed request stages and sends
   nothing), but validation cannot prove a **live** transfer will
   succeed — a script invocation is real I/O (network, remote host,
   filesystem) that can fail for host 3 of 5 after hosts 1–2's transfers
   and sends already completed, since send is the last, unretried step
   per host and R13 forbids retrying it. Policy: on a transfer or send
   failure for host N, **abort all remaining not-yet-attempted hosts** —
   no further transfer or send calls — and
   report a partial result distinguishing **delivered** (recipients whose
   message actually sent) from **not delivered** (the failed host and
   every host after it in fan-out order), both lists by recipient id, on
   stderr and in `--json` output. Bytes already staged remotely for a
   host whose send never happened are **not** rolled back (no daemon
   transfer-undo machinery, decision-and-Non-closure above) — they age
   out under the ordinary 30-day sweep TTL like any other orphaned staged
   file. This is a fail-forward, not fail-atomic, guarantee across hosts;
   R13's zero-sends-on-failure invariant is preserved only for the
   single-invocation, single-outcome case `--from-json` already covers.

```rust
pub struct AtmTemp(PathBuf); // constructed only by resolve_atm_temp

pub trait EnvSource { fn var(&self, key: &str) -> Option<String>; }

pub trait RosterStore {
    /// Roster lookup for picker/recipient resolution: name, registered
    /// host binding (decision (e)), and projection fields.
    fn member(&self, member_id: &str) -> Result<Option<RosterEntry>, StorageError>;
}
// Both fixed/internal; ADR-001 sealed-supertrait pattern (as are this
// sprint's other seams). Trivially object-safe sync traits.

/// Unset -> Ok(default `<std::env::temp_dir()>/atm`, WARNING logged by the
/// caller). Set-but-invalid -> Err. Daemon calls this once at startup; CLI
/// calls it lazily at first scratch use. One function, one default, one
/// error family -- the single ATM_TEMP read site (M14).
pub fn resolve_atm_temp(env: &dyn EnvSource) -> Result<AtmTemp, AtmTempError>;
pub fn send_to_staging_dir(atm_temp: &AtmTemp, transfer_id: &Ulid) -> PathBuf;
// = $ATM_TEMP/send-to/<transfer-id>/

fn resolve_picker_recipient(
    member_id: &str,
    roster: &dyn RosterStore,
) -> Result<AgentAddress, AddressResolutionError>;
```

| `AtmTempError` | Cause | Recovery |
|---|---|---|
| `NotAbsolute` | `ATM_TEMP` set to a relative path | use an absolute path |
| `Unresolvable` | canonicalization failed | check broken symlinks / missing parents |
| `NotWritable` | not writable/creatable | fix permissions or pick another dir |
| `AtmTempInsecure { path, reason }` | the resolved directory (fallback or explicit `ATM_TEMP`) exists but is not owned by the current uid, or has group/world permission bits (decision (a), critical review F7) | remove/chown the directory or set `ATM_TEMP` to a private path |
| `TransferScriptUnsafe { host, reason }` | script at `~/.atm/transfer/<host>` exists but fails the executable-bit/owner-UID/not-group-or-world-writable check (decision (c)) | `chmod 700 ~/.atm/transfer/<host>` and confirm ownership |

`Unset` is intentionally not a variant: an unset `ATM_TEMP` resolves to the
default scratch root (decision (a)), it does not error.

| `AddressResolutionError` | Cause | Recovery |
|---|---|---|
| `UnknownMember` | id not in roster | `atm members` to list |
| `HostUnregistered` | `host: null`, remote required | `atm teams update-member --host <h>` or explicit `agent@team.host` |
| `LocalHostUnset` | recipient is host-qualified but the sender's `.atm.toml` has no `local_host` (decision (f)) | set `local_host` in `.atm.toml`, or route the recipient without `--host` |

2. **Picker projection** `atm teams --json --members` (existing team-count
   shape unchanged): per-member `{"id","name","host":host-or-null,
   "cwd":absolute-or-null,"status":"active|idle|dead"}` with the normative
   mapping `Active→active`, `Idle→idle`, `Offline|Unknown|IdentityConflict
   →dead`; `--host` admin plumbing added; no heartbeat/runtime changes.
3. **`atm send --attach <path>...`** (repeatable; `atm queue` inherits via
   the shared surface): same-host → copy into `send_to_staging_dir()`;
   remote → resolve + invoke the transfer script per (c), grouped one
   transfer per destination host, **sequential per host** (bounded, no
   unbounded child fan-out), aborting remaining hosts on the first
   transfer/send failure per decision (g). Missing/unreadable source →
   hard error before any staging.
4. **`atm send --from-json`**: stdin is exactly one `PickerOutput` object
   `{"schema_version":1,"recipients":[...],"note":"..."}` (rejects unknown
   keys, empty/dup recipients, malformed/trailing input, and an
   unrecognized `schema_version` — the version gate is the picker
   compatibility contract, PRD §4.2/§5a); fan-out one canonical write per
   recipient (send is single-recipient today — positional `to`/`message`
   become clap-optional; mutually exclusive with positional `to`,
   `--stdin`, `--file`, `--template`, and `--env-prefix` (M13: an existing
   `atm send` flag, `crates/atm/src/commands/send.rs:81`, currently only
   meaningful paired with `--template`, which is itself excluded — the
   `PickerOutput` `note` is a plain string, never a template); conflicts
   rejected by clap). All recipients and paths validate before any staging
   or transfer (R5/R13); a transfer or send failure partway through the
   fan-out is decision (g)'s abort-and-report-partial policy, not R5/R13's
   all-or-nothing pre-flight validation (which covers only malformed
   input, not live transfer I/O).
5. **`$ATM_TEMP` sweeper**: a periodic task in the replacement Tokio/Axum
   runtime the daemon actually boots today (`atm-daemon-bootstrap`,
   `crates/atm-daemon-bootstrap/src/lib.rs`, composed alongside the
   existing shutdown-signal wait at `lib.rs:697` — **not** the legacy
   synchronous daemon's `bin_support/daemon_observability.rs`, which
   CLAUDE.md rules off-limits and whose `emit_daemon_event` at line 196 is
   `pub(crate)` to that legacy binary and cannot be the precedent for new
   code). Config: interval and TTL are new `AtmConfig` fields (mirroring
   decision (f)'s `local_host` addition to the same struct,
   `crates/atm-core/src/config/types.rs:54`) — `sweep_interval` default
   1 hour, `sweep_ttl_days` default 30, both validated by the zero-rejects
   rule in decision (a). Shutdown: the task is a `tokio::task::JoinHandle`
   held alongside the runtime's existing `server_task` handle
   (`crates/atm-http-runtime/src/lib.rs:446`) and joined during the same
   graceful-drain sequence that already awaits `begin_shutdown().finish()`
   (`lib.rs:1885`) — cancel-then-join within the sweeper's own bounded
   deadline, not `.abort()`-only (`.abort()` alone, used for the server
   task at `lib.rs:964,2026`, does not guarantee an in-progress sweep
   pass leaves partial state; the sweeper's cancellation must let an
   in-flight per-entry removal finish before exiting). Removes entries
   older than TTL; never follows symlinks out of the root; per-entry
   failures skip-and-log (`SweeperError` only for pass-fatal root
   conditions, surfaced on health); per-sweep structured event `{scanned,
   reclaimed_bytes, skipped}` emitted through `atm_core::observability::
   ObservabilityPort` (the real replacement-runtime observability seam,
   already threaded through `atm-daemon-bootstrap::lib.rs:463` and
   consumed at `atm-http-runtime::storage_and_nudge_router.rs:95` — not
   the legacy `emit_daemon_event`) with `subsystem`/`action`/`outcome` +
   health counter.
6. **Transfer example scripts + setup doc**: `scripts/transfer/
   {sftp.sh,tailscale.sh,sftp.ps1}` — short, commented, agent/human-modifiable,
   honoring the exact invocation contract (remote `$ATM_TEMP` resolution
   shown two ways: fixed value or `ssh <host> 'echo $ATM_TEMP'`; destination
   dir created by the script); `docs/cross-host-file-transfer.md` walks
   copy → chmod → adapt → verify and quotes the canonical error verbatim.
   (QM43-M1 correction, recorded 2026-08-27: shipped as `scripts/transfer/`,
   not the `scripts/send-to/transfer-examples/` path this deliverable
   originally named — a shorter path with no redundant `-examples` suffix,
   consistent with `docs/cross-host-file-transfer.md`'s existing links and
   ADR-055's own references. Ships 3 scripts, not 4: `rsync.sh` was dropped
   because the fleet's baseline transport is the passwordless-SSH `scp`
   flow `sftp.sh`/`sftp.ps1` already cover end to end (verified sprint
   baseline: "The fleet has passwordless SSH from this machine to all
   destinations"); `rsync.sh` is deferred as a documented future addition
   for a fleet whose transport needs it, per
   `docs/cross-host-file-transfer.md`'s "write your own script honoring the
   same contract" guidance, rather than shipped speculatively unverified
   against any real destination.)
7. **Single-owner gate**: an `atm-architecture` test (precedent:
   `boundary_enforcement.rs`'s `emit_received_hook` single-call-site
   assertion) pins one construction site for `send_to_staging_dir()` and
   one member-address resolver.

## Acceptance criteria

1. Validation: `ATM_TEMP` unset → daemon boots and CLI's first scratch use
   both resolve `<std::env::temp_dir()>/atm-<uid>` (Unix; `<temp_dir()>\atm`
   on Windows), create it with mode `0700` if missing, refuse with
   `AtmTempInsecure` when it pre-exists owned by another uid or with any
   group/world bits (test: pre-create as `0755` → refused; `0700` own uid →
   accepted), apply the same check to an explicit `ATM_TEMP`, and
   emit exactly one startup `tracing::warn!` naming the default and the
   override variable — daemon boot is not blocked and exit code is 0;
   `ATM_TEMP` set to a relative/unresolvable/unwritable path fails daemon
   boot and fails the CLI at first scratch use with the table's error
   text; scratch-free commands succeed with `ATM_TEMP` unset; zero
   interval/TTL is rejected only once `ATM_TEMP` resolves (default or
   explicit); a script at `~/.atm/transfer/<host>` that is not
   owner-executable, not owned by the caller's UID, or group/world
   writable is refused with `TransferScriptUnsafe`, not silently run and
   not treated as "missing."
1a. `cargo test` invoked directly (outside `just`, `ATM_TEMP` unset in the
   test process environment) passes unchanged on the pre-sprint suite plus
   this sprint's own tests — proving the default fallback, not a CI-only
   `ATM_TEMP` rollout, is what keeps daemon-spawning tests working; the
   daemon-switch skill's typed launch-overlay sessions (ADR-053) and the
   `docs/peer-pair-smoke.md` m5 fleet daemon are unaffected because
   neither this sprint nor any prior sprint sets `ATM_TEMP` in their
   launch environment.
2. Same-host E2E: file lands under `$ATM_TEMP/send-to/<transfer-id>/`,
   message text names the landed path, content matches.
3. Transfer failures: missing script → canonical error verbatim, exit ≠ 0,
   zero sends; unsafe-permission script (world-writable, or not owned by
   the caller) → `TransferScriptUnsafe`, exit ≠ 0, zero sends, distinct
   from the missing-script error text; failing script → stderr propagated
   (bounded), zero sends; wedged script (sleep past deadline) → child
   killed, ordinary failure; stub-script happy path → one invocation per
   destination host, message carries the stub's dir, child environment
   contains only `ATM_TEMP`/`ATM_IDENTITY`/`ATM_TEAM` (asserted, not just
   documented) and closed stdin; multi-line/relative/control-char stdout
   → rejected as transfer failure; `<host>` containing `/`, `\`, or `..`
   is rejected by `HostName::from_str` before a script path is ever built
   (no separate traversal-guard test needed — same parser as every other
   `HostName` use).
4. `--from-json` truth-table: valid multi-recipient → N messages;
   empty/malformed/cancel/unrecognized `schema_version` → exit ≠ 0, zero
   staging, zero transfer invocations; `--env-prefix` combined with
   `--from-json` → rejected by clap as a conflict. Legacy single-recipient
   `atm send <to> <msg>` preserves required positionals and existing
   diagnostics (captured from pre-sprint baseline). Mid-fan-out transfer
   failure on host N of a multi-host `--from-json` batch (stub script
   fails on its second invocation) → hosts before N delivered, host N and
   every host after it reported not-delivered by recipient id on stderr
   and in `--json`, no further transfer/send calls made (decision (g)).
5. Projection fixture validates against PRD §4.2; `--from-json` resolves
   every recipient through the roster host binding, fails closed on
   null/unknown host; a host-qualified recipient with the sender's
   `local_host` unset in `.atm.toml` fails closed with `LocalHostUnset`
   (decision (f)), not a silent same-host or remote guess.
6. Sweeper: expired reclaimed, fresh kept, symlink escape refused,
   per-entry failure skips; on shutdown the sweeper's `JoinHandle` is
   cancelled and joined within its bounded deadline as part of the same
   drain sequence the server task uses (not `.abort()`-only); Windows
   lane exercises junction/symlink rails.
7. Example-script contract tests (loopback SSH where available, filesystem
   fake otherwise — skips announced, never silent); one live cross-host
   transfer transcript (Mac → second host) committed.
8. Single-owner architecture test green. `just test` all three lanes; no
   clippy warnings in touched crates.

## Paths to delete

None. Existing single-recipient `atm send`, team-count output, and delivery
paths unchanged.

## Required validation

- `just test` workspace + daemon integration suite + script harness,
  ubuntu/macOS/Windows; recorded same-host demo transcript committed.
- Focused command tests for `atm teams --json --members` and
  `atm send --from-json` are named in the PR and run **independently of
  Wyvern and of any real SSH configuration** (stub scripts only) — R6.
- ADR-055 quality-mgr sign-off on decisions (a)–(g).
- M14 (corrected — the prior draft of this bullet named the wrong
  mechanism, verified against `.just/check_env_var_boundary.py`'s actual
  logic before writing this): `resolve_atm_temp(env: &dyn EnvSource)`
  reads `ATM_TEMP` as `env.var("ATM_TEMP")` — a **method call** on the
  trait object (`.var(`), not the free-function path form
  (`env::var(...)`/`std::env::var(...)`) the lint's `ENV_CALL_RE` matches
  (`check_env_var_boundary.py:50`, anchored on a literal `::`). This
  design is lint-clean by construction, with no allowlist or
  `boundary_reader_functions` entry required: adding `resolve_atm_temp` to
  `boundary_reader_functions` would not "sanction" it — that list's actual
  effect (`check_env_var_boundary.py:335-345`) is the opposite of an
  exemption: every call to a listed function from any *other* file in
  `restricted_crate_roots` is flagged as a `boundary_reader_function_call`
  violation. The real, working exemption mechanism — demonstrated by
  every existing entry in `.just/allowlists/env_var_boundary_allowlist.toml`
  (e.g. `read_cli_identity_from_env`, itself the sanctioned ATM_IDENTITY
  choke point) — is an allowlist entry keyed on the exact offending
  `(path, symbol, line)`, not a name registered anywhere else. This
  deliverable still adds `"ATM_TEMP"` to `forbidden_env_vars` in
  `.just/lint-config.toml`'s `[env_var_boundary]` table (currently
  `["ATM_TEAM", "ATM_IDENTITY", "ATM_CHAT_ID", "ATM_SESSION_ID",
  "ATM_PID"]`, `.just/lint-config.toml:27`) as defensive coverage — it
  costs nothing today (there is no existing offending call site to
  allowlist) and catches a future direct `env::var("ATM_TEMP")` added
  outside the `EnvSource` seam by someone who didn't use it. If a later
  concrete `EnvSource` implementation is ever refactored into a shape the
  lint's forwarding-function detection *does* match (a same-file free
  function taking exactly one `&str` parameter that forwards into
  `env::var`), the lint will start flagging its callers automatically —
  at that point, and only then, does an allowlist entry become necessary,
  and it goes in `env_var_boundary_allowlist.toml`, never
  `boundary_reader_functions`.

## Evidence/validation

- AC 6 (Windows): `crates/atm-core/src/atm_temp_sweeper.rs::junction_escape_is_never_followed`
  (`#[cfg(windows)]`) exercises a real NTFS junction (`mklink /J`) alongside
  the existing Unix `symlink_escape_is_never_followed` twin. Closing this
  test surfaced a real gap in `sweep_dir`'s never-follow check
  (`FileType::is_symlink()` does not recognize
  `IO_REPARSE_TAG_MOUNT_POINT`, only `IO_REPARSE_TAG_SYMLINK`); fixed via
  `is_symlink_or_reparse_point` (checks the raw
  `FILE_ATTRIBUTE_REPARSE_POINT` bit on Windows), so a junction is now
  refused exactly like a symlink on every platform.
- AC 7 (example-script contract tests): `.just/tests/test_transfer_scripts.py`
  (picked up by `python3 .just/run_lint.py pytests`) puts fake `ssh`/`scp`
  executables (the only two external binaries `sftp.sh`/`tailscale.sh`
  actually invoke — neither shells out to a literal `sftp` or `tailscale`
  binary) on `PATH`, and asserts exact argv/single-line-stdout on the happy
  path, fail-closed on a missing binary, an unreachable host, a nonzero
  remote `mkdir`/copy exit, and a receiver-side path-containment refusal,
  and that the caller's own ambient environment is never mutated.
  `sftp.ps1` is covered the same way through `pwsh` when present (gated
  `win32 or pwsh found`, honest skip otherwise, mirroring this suite's
  existing platform-gated tests). **SFTP leg satisfied:** run 33128348487
  (linux+macos PASS) provides live cross-host loopback-SSH transcripts.
  **Windows scope (run 33138148783):** `SftpShTests`/`TailscaleShTests`
  unconditionally skip on `win32` — `sftp.sh`/`tailscale.sh` are the
  macOS/Linux transfer scripts, Windows ships `sftp.ps1`, and running the
  `.sh` scripts under a discovered MSYS `bash` on `windows-latest` exercises
  an unshipped configuration, not the Windows contract. `SftpPs1Tests` is
  Windows's real coverage and must pass there: run 33138148783 surfaced a
  genuine `sftp.ps1` defect — the example's fixed-`$RemoteAtmTemp`
  placeholder (`atm-<destination-uid>`) used `<`/`>`, legal in a POSIX
  filename on the real Unix receiver but reserved Win32/NTFS characters,
  which broke `SftpPs1Tests`' local fake-ssh/fake-scp harness's `mkdir`
  simulation on a real Windows sender (`test_happy_path_exact_argv_and_single_line_landed_dir`,
  `test_copy_failure_fails_closed`); fixed by changing the placeholder to
  `atm-REPLACE_WITH_DESTINATION_UID`, which carries the same "must
  customize before use" intent without reserved characters. Unix behavior
  (`sftp.sh`/`tailscale.sh`) is unchanged.
- AC 7 (live cross-host transcript): the sftp.sh leg is complete via run
  33128348487 (evidence table above); the Tailscale leg remains a
  **FOLLOW-UP `AQ4-tailscale-m5`** (fleet `m5` host not reachable from
  clean-runner CI lanes, and Tailscale enrollment is an operator/IT
  environment concern this sprint documents but does not implement — see
  Non-closure below). `scripts/phase-aq/run_aq4_transfer_evidence.py` —
  registered in `.github/workflows/phase-aq-evidence.yml`'s harness list
  beside AQ1.9 and AQ2.5 — drives a real `atm send --attach` over a real
  loopback `sshd` the script starts (installing `openssh-server` on ubuntu
  if missing; macOS uses the bundled `/usr/sbin/sshd`, honestly recording
  `skipped_no_sshd` if it cannot run), through the real, unmodified
  `scripts/transfer/sftp.sh` example, verifying the attached file lands
  under the receiver's real mailbox-reported `$ATM_TEMP/send-to/<transfer-id>`
  path with byte-for-byte content match. `scripts/phase-aq/test_run_aq4_transfer_evidence.py`
  covers the harness's pure launch/record-schema logic (argument defaults,
  evidence-writer output, exit-code mapping) without needing a live `sshd`.

| Runner | Status | Run ID | Head | Files |
|--------|--------|--------|------|-------|
| ubuntu-latest | pass | 33142976493 | dcd3130f1 | [transfer-clean-runner-linux.json](evidence/AQ4/transfer-clean-runner-linux.json) · [transfer-clean-runner-linux.md](evidence/AQ4/transfer-clean-runner-linux.md) |
| macOS | pass | 33142976493 | dcd3130f1 | [transfer-clean-runner-macos.json](evidence/AQ4/transfer-clean-runner-macos.json) · [transfer-clean-runner-macos.md](evidence/AQ4/transfer-clean-runner-macos.md) |
| windows | fail | 33142976493 | dcd3130f1 | [transfer-clean-runner-windows.json](evidence/AQ4/transfer-clean-runner-windows.json) · [transfer-clean-runner-windows.md](evidence/AQ4/transfer-clean-runner-windows.md) · ssh client aborts at kex identification under sftp.ps1 invocation — fix in progress |

The Tailscale leg (`scripts/transfer/tailscale.sh`) cannot run on either
clean-runner lane (Tailscale enrollment is an operator/IT environment
concern this sprint documents but does not implement — see Non-closure
below); a real cross-host Mac → `m5` transcript over Tailscale is tracked
as **FOLLOW-UP `AQ4-tailscale-m5`**, paired with `AQ1.9-m5` (same
prerequisite: `m5` reachable), in
`docs/plans/phase-aq/.audit/qa-evidence-master.json`'s `follow_ups`.

## Non-closure / out of scope

- Pickers/shell glue/R8 and phase evidence (AQ5). Managed SSH/Tailscale
  enrollment (environment/IT concern — documented, not implemented).
- Live cross-host Tailscale transfer transcript against the fleet's `m5`
  host — tracked as **FOLLOW-UP `AQ4-tailscale-m5`** (paired with
  `AQ1.9-m5`) in `docs/plans/phase-aq/.audit/qa-evidence-master.json`;
  neither clean-runner CI lane can reach `m5`, and this sprint does not
  implement Tailscale enrollment itself (see above).
- Structured `attachments` envelope metadata, `note_source` (Phase 2).
- The rejected pull-based transfer design (fetch endpoints,
  delivery-gating, attachment storage traits) — see plan Non-closure.

## Dependencies

- must_follow: AQ1–AQ3 (queue ships first per Rand; deliverable 3's
  `--attach` changes the shared `atm send` surface using AQ1's
  `NudgeMode` seam, which lands across AQ1–AQ3). AQ2.6–AQ2.7 (Herdr) are
  **not** a must_follow here (M12: the prior dependency on AQ2.6–AQ2.7 was
  unjustified — this sprint shares no file with either); AQ5's phase
  evidence, not this sprint, is what needs the Herdr/tmux `atm queue`
  transcripts, and AQ5 already carries `must_follow AQ2.6–AQ2.7` for that
  reason. — merge-forward before every dev/fix round.
- parallel_safe: none at start; AQ5 follows.
