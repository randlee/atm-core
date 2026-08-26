# Sprint AQ4 — Send-To Core: ATM_TEMP, CLI Surface, Transfer Scripts

Status: draft · Branch: `feature/aq-4-send-to-core` off `integrate/phase-aq`
· PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Consolidation note (Rand, 2026-08-23): this sprint deliberately carries
what an earlier draft split across four sprints (ATM_TEMP contract, CLI
surface, sweeper, transfer scripts). The justification is not scope-neutral
reshuffling: all four share one owner and one contract — the `ATM_TEMP`
root and the staging convention — so splitting them buys four dispatch/QA
cycles for work that is sequential inside one owner's context, and the
finer-grained split already went through this phase's QA rounds (its
hardened acceptance criteria are preserved verbatim below). To keep a
slipping sub-scope visible before the final gate, the deliverables land in
the stated order (1 → 2 → 3/4 → 5 → 6 → 7) and the PR opens once
deliverables 1–4 are pushed, so CLI-surface QA runs while the sweeper and
example scripts finish; a script-harness or sweeper failure is triaged
against its own deliverable, not treated as a CLI-surface regression.

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
--chat-id --as --file --stdin --template --vars --var --tag --category
--content-format --summary --requires-ack --task-id --dry-run --json`;
`AtmConfig` has no temp key; daemon dirs are `~/.atm/{daemon,db,logs}`;
maintenance-worker precedent for periodic tasks; `ensure_column` for
migrations. The fleet has passwordless SSH from this machine to all
destinations (Rand) — the sftp example's baseline.

## Deliverables

1. **ADR-055 atm-temp-and-transfer-seam**, quality-mgr-reviewed, deciding:
   (a) **`ATM_TEMP` system contract** — mandatory env var naming the ATM
   scratch root for all features. Daemon validates **eagerly at startup**
   (it owns the sweeper) — unset/relative/unresolvable/unwritable, or zero
   sweep-interval/TTL, fail boot with actionable errors; the CLI resolves
   **lazily at first scratch-space use** (commands touching no scratch
   space run unaffected). This sprint owns the environment rollout:
   `ATM_TEMP` set in `.github/workflows/ci.yml` (all lanes), justfile test
   targets, daemon launch overlays, and developer docs, in the same PR.
   (b) **Sweep policy** — TTL-only, 30 days, everything under `$ATM_TEMP`;
   no ack coupling, no storage traits. Accepted, documented risk: staged
   attachments for messages unread past the TTL may be reclaimed before a
   deferred nudge fires — an ordinary missing-file case; 30-days-unread is
   abandoned (open-item register, AQ5).
   (c) **Transfer-script seam** — per-destination-host user script at
   `~/.atm/transfer/<host>` (`.ps1` on Windows), invoked
   `<script> <host> <transfer-id> <file>...` via **argv-array exec** (never
   shell-interpolated), **bounded deadline** (default 60 s, configurable;
   child killed on expiry), **capped stdout/stderr** (truncate with
   marker), success = single-line absolute-path landed dir on stdout
   **validated as untrusted input** (one line, absolute, no control chars);
   nonzero exit propagates stderr to the user (bounded). Missing script →
   the canonical error, exactly: `File transfer to <host> not enabled.
   Read docs/cross-host-file-transfer.md to set up cross-host file
   transfer.` Any transfer failure fails the whole invocation closed —
   zero messages sent (R5/R13).
   (d) **Message-text path convention** — landed paths ride in message text
   ("Attached files (on this host): …"); no envelope change; structured
   `attachments`/`note_source` are Phase-2 candidates.
   (e) **Member-host sourcing** — durable optional roster field via
   `teams add-member/update-member --host` (`RosterEntry.metadata_json
   ["host"]`, validated `HostName`), never inferred from heartbeat/DNS/
   sockets; `host: null` = unroutable via `--from-json`. Resolution
   algorithm is this sprint's `resolve_picker_recipient` — single
   canonical implementation.

```rust
pub struct AtmTemp(PathBuf); // constructed only by startup/lazy validation

pub trait EnvSource { fn var(&self, key: &str) -> Option<String>; }

pub trait RosterStore {
    /// Roster lookup for picker/recipient resolution: name, registered
    /// host binding (decision (e)), and projection fields.
    fn member(&self, member_id: &str) -> Result<Option<RosterEntry>, StorageError>;
}
// Both fixed/internal; ADR-001 sealed-supertrait pattern (as are this
// sprint's other seams). Trivially object-safe sync traits.

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
| `Unset` | var not set | `export ATM_TEMP=<absolute path>`; see the setup doc |
| `NotAbsolute` | relative path | use an absolute path |
| `Unresolvable` | canonicalization failed | check broken symlinks / missing parents |
| `NotWritable` | not writable/creatable | fix permissions or pick another dir |

| `AddressResolutionError` | Cause | Recovery |
|---|---|---|
| `UnknownMember` | id not in roster | `atm members` to list |
| `HostUnregistered` | `host: null`, remote required | `atm teams update-member --host <h>` or explicit `agent@team.host` |

2. **Picker projection** `atm teams --json --members` (existing team-count
   shape unchanged): per-member `{"id","name","host":host-or-null,
   "cwd":absolute-or-null,"status":"active|idle|dead"}` with the normative
   mapping `Active→active`, `Idle→idle`, `Offline|Unknown|IdentityConflict
   →dead`; `--host` admin plumbing added; no heartbeat/runtime changes.
3. **`atm send --attach <path>...`** (repeatable; `atm queue` inherits via
   the shared surface): same-host → copy into `send_to_staging_dir()`;
   remote → resolve + invoke the transfer script per (c), grouped one
   transfer per destination host, **sequential per host** (bounded, no
   unbounded child fan-out). Missing/unreadable source → hard error before
   any staging.
4. **`atm send --from-json`**: stdin is exactly one `PickerOutput` object
   `{"schema_version":1,"recipients":[...],"note":"..."}` (rejects unknown
   keys, empty/dup recipients, malformed/trailing input, and an
   unrecognized `schema_version` — the version gate is the picker
   compatibility contract, PRD §4.2/§5a); fan-out one canonical write per
   recipient (send is single-recipient today — positional `to`/`message`
   become clap-optional; mutually exclusive with positional `to`,
   `--stdin`, `--file`, `--template`; conflicts rejected by clap). All
   recipients and paths validate before any staging or transfer (R5/R13).
5. **`$ATM_TEMP` sweeper**: periodic Tokio task (maintenance-worker
   precedent, `spawn_blocking` rail, shutdown cancel+join within deadline),
   removing entries older than TTL; never follows symlinks out of the
   root; per-entry failures skip-and-log (`SweeperError` only for
   pass-fatal root conditions, surfaced on health); per-sweep structured
   event `{scanned, reclaimed_bytes, skipped}` + `subsystem`/`action`/
   `outcome` + health counter (recorded `emit_daemon_event` exception).
6. **Transfer example scripts + setup doc**: `scripts/send-to/
   transfer-examples/{sftp.sh,tailscale.sh,rsync.sh,sftp.ps1}` — short,
   commented, agent/human-modifiable, honoring the exact invocation
   contract (remote `$ATM_TEMP` resolution shown two ways: fixed value or
   `ssh <host> 'echo $ATM_TEMP'`; destination dir created by the script);
   `docs/cross-host-file-transfer.md` walks copy → chmod → adapt → verify
   and quotes the canonical error verbatim.
7. **Single-owner gate**: an `atm-architecture` test (precedent:
   `boundary_enforcement.rs`'s `emit_received_hook` single-call-site
   assertion) pins one construction site for `send_to_staging_dir()` and
   one member-address resolver.

## Acceptance criteria

1. Validation: unset/relative/unresolvable/unwritable `ATM_TEMP` fails
   daemon boot and fails the CLI at first scratch use with the table's
   error text; scratch-free commands succeed unconfigured; zero
   interval/TTL rejected at boot; env rollout proven (pre-existing suites
   pass unchanged post-rollout).
2. Same-host E2E: file lands under `$ATM_TEMP/send-to/<transfer-id>/`,
   message text names the landed path, content matches.
3. Transfer failures: missing script → canonical error verbatim, exit ≠ 0,
   zero sends; failing script → stderr propagated (bounded), zero sends;
   wedged script (sleep past deadline) → child killed, ordinary failure;
   stub-script happy path → one invocation per destination host, message
   carries the stub's dir; multi-line/relative/control-char stdout →
   rejected as transfer failure.
4. `--from-json` truth-table: valid multi-recipient → N messages;
   empty/malformed/cancel/unrecognized `schema_version` → exit ≠ 0, zero
   staging, zero transfer invocations. Legacy single-recipient `atm send <to> <msg>` preserves
   required positionals and existing diagnostics (captured from pre-sprint
   baseline).
5. Projection fixture validates against PRD §4.2; `--from-json` resolves
   every recipient through the roster host binding, fails closed on
   null/unknown host.
6. Sweeper: expired reclaimed, fresh kept, symlink escape refused,
   per-entry failure skips; shutdown joins within deadline; Windows lane
   exercises junction/symlink rails.
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
- ADR-055 quality-mgr sign-off on decisions (a)–(e).

## Non-closure / out of scope

- Pickers/shell glue/R8 and phase evidence (AQ5). Managed SSH/Tailscale
  enrollment (environment/IT concern — documented, not implemented).
- Structured `attachments` envelope metadata, `note_source` (Phase 2).
- The rejected pull-based transfer design (fetch endpoints,
  delivery-gating, attachment storage traits) — see plan Non-closure.

## Dependencies

- must_follow: AQ1–AQ3 and AQ2.6–AQ2.7 (queue ships first per Rand; the
  retained-tmux/alternate-Herdr backend chain must land before this sprint
  changes the shared send surface with AQ1's `NudgeMode` seam) —
  merge-forward before every dev/fix round.
- parallel_safe: none at start; AQ5 follows.
