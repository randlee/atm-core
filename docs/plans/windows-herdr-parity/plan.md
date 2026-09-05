---
phase: AY
title: Phase AY, Windows Herdr parity and UDS/named-pipe transport (same command set, same feature set on macOS, Linux, Windows)
integration_branch: develop (plan) / integrate/phase-ay (dev)
status: draft, not approved (Rand, 2026-09-05)
owner: fenix (fenix@atm-dev on rand-m5, from 2026-09-05T18:51Z)
authored: 2026-09-05
supersedes:
  - docs/adr/ADR-058-herdr-local-steer-backend-contract.md:576 ("Herdr on Windows ... out of AQ scope")
  - docs/atm-herdr/requirements.md:376-378 (Windows non-goal, AX.6 head c3268df8a)
  - docs/plans/phase-aq/sprint-AQ2-6-herdr-steer-backend.md:728-730 (deliverable 5 Windows deferral)
---

# Phase AY: Windows Herdr parity and UDS/named-pipe transport

## Why this plan exists

The 1.5.0 Herdr integration was meant to establish Windows parity for
agent nudging without tmux or WSL. Phase AQ instead recorded Windows as
out of scope in three planning documents without ever surfacing that
narrowing as a decision. That scope-out was never approved. The
expectation is that macOS, Linux and Windows have the same feature set:
the daemon talks to Herdr with one command set, over a Unix domain socket
on macOS/Linux and over Herdr's named pipe on Windows.

Post-mortem entry: filed as AW-READY-W1 (blocking) with a
planning-process-improvement action (scope narrowings hidden inside ADR
"not relied upon" lists or sprint deliverables must be surfaced to the
user explicitly).

## Design rulings this revision implements (Rand, 2026-09-05)

These rulings were given on 2026-09-05 during review of the previous
revision (9e7577143) and are binding for every sprint below. They replace
the launch contract and startup path of that revision, which tried to make
atm own Herdr and gated the daemon on Herdr being reachable.

1. **atm never owns Herdr.** Herdr is the per-user singleton that owns its
   server, socket, session state, startup and restart. atm is one more
   client of it, exactly like a user typing `herdr agent prompt`. The
   daemon never launches, stops, waits for, or probes Herdr as part of its
   own startup, and never computes Herdr's paths.
2. **No hard gate on Herdr.** The daemon always starts and always serves
   messaging, tmux nudges, hermes nudges and doctor, whether or not Herdr
   is installed, reachable, or crashes later. atm is the tool used to
   diagnose and repair a machine whose Herdr is misconfigured, so a Herdr
   failure is a per-call failure on the herdr harness only. The daemon
   never exits because of Herdr.
3. **Startup ordering is an installer concern, not a daemon concern.**
   When atm is configured to work with Herdr (the roster has at least
   one member whose local backend is Herdr; no flag), the service
   installer (`daemon-switch`, through its explicit `herdr-entry install`
   step in the documented install procedure; see "Governance") also
   installs a Herdr start-at-login entry so Herdr is normally up before
   the daemon. When atm is not configured
   for Herdr, nothing Herdr-related is installed. Ordering is
   best-effort: the daemon tolerates Herdr arriving later (section
   "Startup, environment and failure model").
4. **Explicit configuration, no environment inference.** The daemon does
   not detect or infer Herdr's environment. It carries explicit,
   optional configuration for the Herdr session or socket path and for
   the binary path, defaulting to Herdr's own defaults, passes them to
   the child exactly as HR-CORE-006 already specifies, and doctor reports
   them with their provenance.
5. **Herdr over the CLI transport works today; prove Windows once, on the
   target transport** (Rand, 2026-09-05, r19). No sprint runs a live
   Windows evidence campaign for the CLI transport. AY.7 is limited to
   Windows process correctness in `transport_cli.rs`, the Windows branch
   of the installer, and the audit columns; the single live Windows Herdr
   evidence set of the phase is AY.10's, on the socket transport after the
   AY.9 cutover has merged.

## Baseline: the phase-ax Herdr contract is the starting point

Phase AY starts from the Herdr contract as phase AX leaves it, not from
the AQ/AW state this plan was first written against. Concretely, the
baseline for every AY sprint is the `integrate/phase-ax` head after PR
#1204 (AX.6) has merged: `docs/atm-herdr/requirements.md` HR-CORE-001..010,
HR-SAFE-001..007 and HR-TEST-*, ADR-058 D1..D10.x, the atm-herdr boundary
TOML with the AX.6 D7 note, the AX.5 queue-wake pump and reminder cadence,
the AX.6 escalation/notify path and doctor section, and the AX.7 evidence
table format. Any Herdr contract change that lands in phase AX before the
cut is inherited by AY as-is. AY changes that contract only where this
plan says so: it deletes the three Windows scope-out statements, adds
HR-PLAT-001 and HR-LIFE-001, amends ADR-058 D3 for the transport pair,
corrects the ADR-058 Herdr pin, and (AY.8 only) revises the boundary TOML
io_owns. Everything else in the AX contract is carried unchanged, and the
"Request set" section below lists it from the AX.6 head.

## What blocks Windows today (verified 2026-09-05, integrate/phase-aw)

Nothing in the code. `crates/atm-herdr/src/lib.rs` production code has
no platform branches; it spawns the `herdr` binary by name and Herdr's
own CLI selects UDS or named pipe. Backend selection, roster harness
fields, the received-hook selector and the doctor presence probe are all
platform-neutral. The Herdr client is daemon-side only (atm-http-runtime
and atm-daemon-bootstrap depend on atm-herdr; the CLI crate does not), so
nothing here touches the frozen legacy synchronous daemon.

The real gaps:

1. Three documentation/policy statements declare Windows unsupported
   (listed under `supersedes` above).
2. All six process-behaviour tests in atm-herdr are `#[cfg(unix)]`
   (develop a7aebefb8 lib.rs:1124, 1140, 1159, 1182, 1200, 1220; /bin/sh
   fixtures). The
   windows-latest CI leg compiles the crate and runs none of them. The
   "Windows verifies selection and command construction" claim in AQ2.6
   refers only to the platform-neutral argv/env unit tests that happen to
   run there.
3. Untested Windows spawn concerns: `herdr.exe` discovery (configured
   path, PATH), console-window flash on every nudge (CREATE_NO_WINDOW),
   kill/reap on timeout (orphaned herdr.exe), stdout encoding.
4. No live Windows Herdr deployment or evidence of any kind.
5. No startup/failure model: the daemon has no stated behaviour for
   "Herdr not installed", "Herdr not running yet" or "Herdr crashed".

## Herdr minimum version, drift and schema management

Ruling (Rand, 2026-09-05; authority: ADR-061
`docs/adr/ADR-061-governed-interface-schema-versioning.md`, landing on PR
#1218 together with the constant itself at 29e483e50; discussion in
randlee/atm-core#1217): Herdr drifts
outside our control, and atm-core is responsible for supporting **every
Herdr version at or above `HERDR_MINIMUM_VERSION`**, following the same
schema-management pattern as the HTTP API semver rules. Rand's minimum
today is **0.8.0**. This replaces the previous revision's "pin v0.8.2"
approach.

ADR numbering (AYS-R2-001): `docs/adr/ADR-061-governed-interface-schema-versioning.md`
is on develop (PR #1218). Phase AX's AX.3 created
`docs/adr/ADR-061-task-state-machine.md` on `integrate/phase-ax`, not yet
on develop, so the two collide when phase AX lands. Resolution, owned by
fenix inside phase AX (not AY): the AX.3 ADR is renumbered to the next
free number on `integrate/phase-ax`, with its references in
`docs/plans/phase-ax/`, `docs/requirements.md` and the boundary records,
before the integrate/phase-ax to develop PR opens. AY keeps citing
ADR-061 for the schema-versioning ruling. AY.1 acceptance adds the
mechanical check `ls docs/adr/ADR-061-*.md | wc -l` = 1 on its branch.

Plan-declared schema rules (checked by the `schema-reviewer` agent, PR
#1218, at plan review and phase-ending review):

1. `crates/atm-herdr` declares `pub const HERDR_MINIMUM_VERSION` = 0.8.0
   (Herdr release version, semver, as reported by `ping.version` and
   `herdr --version`). Rand set it on PR #1218 (29e483e50,
   `crates/atm-herdr/src/lib.rs`, with a guard test); AY.3 builds the
   doctor check on that constant and does not introduce it. Changing the
   value is a separate PR after #1218 completes. One daemon build supports every Herdr version at or above
   it simultaneously. Herdr's integer `PROTOCOL_VERSION` (19 at v0.8.0,
   20 at v0.8.2, 22 at master 3a822e81) is recorded per release as a
   secondary fact; it versions Herdr's bincode client socket, not the
   NDJSON API, and the NDJSON server does not check it (see
   "PROTOCOL_VERSION and compatibility").
2. Raising `HERDR_MINIMUM_VERSION` is a breaking change: explicit,
   recorded sign-off from Rand, same as a major `HTTP_API_VERSION` bump.
   Without a cited sign-off, a raised minimum or a newest-Herdr
   assumption in any AY document is Blocking.
3. New Herdr capability is adopted additively, feature-detected or
   version-gated at runtime (`ping.version`, `ping.capabilities`), never
   by requiring the newest Herdr. Behaviour that differs across supported
   versions (today: `agent prompt --wait` semantics after v0.8.2) is
   handled on atm's side so that the atm-visible contract is identical.
4. Every supported Herdr version has a conformance fixture in this repo,
   documented and tested the way SQLite migrations and the HTTP schema
   are: `docs/atm-herdr/herdr-versions.md` lists each version with its
   `PROTOCOL_VERSION`, the observed argv/JSON for the six operations atm
   uses (five nudge commands plus `status server --json` for doctor), and
   the drift items; the fake-herdr fixture (AY.2)
   replays the recorded responses of the design target (v0.8.2 today)
   plus a delta mode per documented behavioural difference (v0.8.0:
   blocked-prompt submits then waits), so the ADR-058 suite covers every
   0.8.\* release on every CI lane without one recording set per
   release. A new Herdr release adds a recording set only when its diff
   on our six operations is non-empty (rule 5).
5. A dedicated drift check runs against the reference checkout
   (`/Users/randlee/Documents/github/herdr`, kept current by Rand): at
   every AY sprint start and at phase end, `git diff <last-recorded>..HEAD`
   on the client-facing paths (`src/api/`, `src/cli/agent.rs`,
   `src/cli/notification.rs`, `src/cli/spec.rs`, `src/cli.rs`,
   `src/cli/protocol_guard.rs`, `src/ipc.rs`, `src/session.rs`,
   `src/integration/env.rs`, `src/protocol/wire.rs`, `distribution/`) is
   reviewed and the result appended to `herdr-versions.md`. fenix@rand-m4's
   tightened schema checking and its drift QA agent are extended to Herdr;
   until that agent ships, quality-mgr's req-qa pass performs the check
   from the recorded table.

Before PR #1218 `crates/atm-herdr` had no version constant and a mismatch
surfaced only as `HerdrError::ProtocolMismatch`. AY.1 (version table, drift
procedure), AY.2 (recordings and replay) and AY.3 (doctor check) land
rules 2 to 5 for the CLI transport on top of the #1218 constant; AY.8
extends the same matrix to the direct socket client.

### Mechanism: low-code by design (Rand, 2026-09-05)

Rand's constraint: "complicated logic managing change that will rarely
occur is maintenance we do not want to be involved with." The drift
review supports a minimal design: across v0.8.0, v0.8.2 and master
3a822e81 the six operations atm uses (five nudge commands plus `status
server --json`) have identical argv, JSON shapes, exit codes and error
codes; the only behaviour change is when a
`--wait` ends in `timeout` versus `agent_prompt_stalled`. So the
mechanism is:

1. **One implementation, version-agnostic by construction.** No adapter
   registry, no per-version code paths, no version chosen at startup and
   none negotiated per call. The single Herdr client keys on error
   *codes* (never message text), ignores unknown JSON fields (serde
   default), and treats `timeout` and `agent_prompt_stalled` as the same
   atm outcome ("prompt produced no activity", already the case for the
   queue pump and reminder cadence, which re-nudge pending mail). That
   one collapse absorbs the only known behaviour drift with zero
   version logic. Herdr upgrading or downgrading while the daemon runs
   therefore needs **no daemon restart and no code**: the next call is a
   fresh process against whatever Herdr is there.
2. **`HERDR_MINIMUM_VERSION` is a doctor check, not a runtime branch.**
   Doctor runs `herdr --version` (CLI transport) or reads `ping.version`
   (AY.8) and reports below-minimum as a finding with the remedy. The
   daemon does not gate calls on it: a too-old Herdr fails on its own
   terms (unknown flag, exit 2) and the breaker/doctor already surface
   that.
3. **Drift is managed as process and data, not code.** Rule 4's
   `docs/atm-herdr/herdr-versions.md` records, per Herdr release, the
   observed argv and JSON for the six operations; the fake-herdr fixture
   replays those recordings, so the conformance suite is data-driven and
   adding a Herdr version is adding a recording, not code. Rule 5's
   sprint-start drift check is a `git diff` on the client-facing paths
   plus a reviewer reading it.
4. **Escalation path when drift is not absorbable.** If a future Herdr
   changes something the single implementation cannot tolerate (a
   renamed command, a removed field atm relies on, a new exit-code
   meaning), that is the moment to decide with Rand between raising
   `HERDR_MINIMUM_VERSION` (rule 2) and introducing a second
   implementation behind the existing transport seam. The seam exists
   for the transport (CLI vs socket); a version seam is added only when
   a second implementation actually exists, never speculatively.
5. **Tests (AY.2, AY.3):** conformance replay of each recorded Herdr version
   through the one implementation; a lifecycle test where the fake
   Herdr switches its recorded version between two calls with the
   daemon running and both calls succeed with no restart; a
   below-minimum fake asserting the doctor finding.

Windows note: 0.8.0 is the minimum for macOS and Linux. `herdr.exe` is a
released artifact from v0.8.2 onward (9fac5172 "make Windows generally
available", 2026-08-18, `release.yml` `x86_64-pc-windows-msvc`,
`windows-arm64.yml`); v0.8.0 shipped Windows as beta without a release
job. This is a packaging fact, not a compatibility floor: the minimum
stays 0.8.0 on every platform.

Design target (Rand, 2026-09-05): **design and test to v0.8.2 and claim
0.8.\* compatibility**, minimum 0.8.0. Basis, verified on the tags
(`git diff v0.8.0 v0.8.2`): the six operations atm uses have identical
argv, flags, JSON shapes and exit codes at both tags (`src/cli/agent.rs`
changes are all in `agent start`, unused by atm; `src/cli.rs` changes are
output buffering, `--flag=value` expansion and the Windows channel gate;
`src/cli/spec.rs` changes are help text and an unrelated `input`
subcommand; `notification.rs` and `status.rs` unchanged; `ping`/`status
server --json` fields unchanged). Error-code delta on our path: exactly
one additive behaviour, `agent prompt` to an already-blocked agent is
rejected with `agent_blocked` before any input at v0.8.2
(`src/app/api/agents.rs`), where v0.8.0 submitted and then waited; atm
already maps `agent_blocked` to `HerdrError::AgentBlocked` (AX contract)
and handles the v0.8.0 outcome through the normal wait path, so both
work with no branch. `agent_not_ready`/`agent_pane_busy` are `agent
start` codes, not on our path. Consequences for the plan: one recording
set (v0.8.2) is the conformance fixture, plus one delta line for v0.8.0
in `herdr-versions.md` and a fake-herdr mode for the v0.8.0 blocked
prompt behaviour; AY.2 runs the ADR-058 suite once against the real
v0.8.0 macOS artifact as a one-off confirmation (the artifact exists);
the Windows question for 0.8.0 is moot (no official artifact exists, so
no user can be on it; Windows users are 0.8.2+ by construction).

### Reference checkout and release state (2026-09-05)

| Item | Value |
|---|---|
| Reference checkout | `/Users/randlee/Documents/github/herdr`, branch master, HEAD `3a822e81` (2026-09-05) |
| Releases in range | v0.8.0 = `857196de` (2026-08-03, PROTOCOL_VERSION 19); no v0.8.1 tag; v0.8.2 = `34ba52cc` (2026-08-19, PROTOCOL_VERSION 20) |
| Release artifacts (GitHub Releases, 2026-09-05) | v0.8.0: `herdr-macos-aarch64`, `herdr-macos-x86_64`, `herdr-linux-aarch64`, `herdr-linux-x86_64` (no Windows). v0.8.2 (Latest): the same four plus `herdr-windows-x86_64.zip` (no Windows arm64 asset despite `windows-arm64.yml`). Preview builds are pre-releases (`preview-2026-08-31-b1ff4582e968` newest) |
| After v0.8.2 | 92 commits to 3a822e81; `Cargo.toml` still 0.8.2; newest tag `preview-2026-08-31-b1ff4582e968` (HEAD is 41 past it, untagged); preview channel publishes from master; no sign of an imminent release |
| Superseded pins | ADR-058 cites `d79fd746` / PROTOCOL_VERSION 21; the previous revision of this plan pinned v0.8.2 / 20. Both replaced by the minimum-version rule (AY.1 amends ADR-058) |

### Drift v0.8.0 -> v0.8.2 (to be completed by AY.1)

`--timeout MS` on `agent prompt`/`agent wait`, `notification show <title>
[--body TEXT] [--sound none|done|request]` and the `--wait` activity gate
(5000 ms state-change requirement, `agent_prompt_stalled`) all exist at
v0.8.0 (`src/cli/spec.rs:294-357`, `src/cli/notification.rs:28` at the
tag). Between v0.8.0 and v0.8.2 the client-facing files changed by
544/104 lines (`src/cli/agent.rs`, `src/cli.rs`, `src/cli/spec.rs`,
`src/api/server.rs`, `src/api/schema/response.rs`, `src/ipc.rs`,
`src/integration/env.rs`), and `src/cli/agent.rs` gained the
`agent_not_ready`, `agent_pane_busy` and `agent_status` codes. AY.1 records the per-operation diff for the six operations atm
uses and the full error-code delta, so the v0.8.0 conformance fixture is
grounded in the tag rather than assumed.

### Drift v0.8.2 -> master 3a822e81 (two background agents, 2026-09-05)

| Area | Change | Effect on atm |
|---|---|---|
| `agent prompt --wait` | Activity gate now requires an observed `working` or `blocked` status (8633a398, 7916be16; `src/api/wait.rs:177-320,516`); a prompt into an already-working agent skips the gate; done/idle churn without `working` ends in `timeout`; blocked-after-submit returns success with `agent_status: blocked`; `agent_prompt_stalled` message text changed (`wait.rs:662-664`); on Windows the dispatch is bounded by the caller timeout and returns `timeout`, not `server_unavailable` | Material for HR-CORE-002/003: the same nudge can end in `timeout` on a newer Herdr where v0.8.2 returned `agent_prompt_stalled`. atm keys on codes, never message text; AY.1 re-verifies (docs), AY.2 tests the `HerdrError` mapping, and AY.4 tests queue-pump/reminder behaviour under both outcomes (rule 3) |
| Update model | `herdr update` keeps a compatible old server running (cc88b3b8, `CompatibleServerKept`); `herdr status` `restart_needed` keys on endpoint generation, not PROTOCOL_VERSION | An updated CLI binary against a still-running old server fails every CLI command with `protocol_mismatch`, and that state is now normal and sticky; doctor must make it legible; direct NDJSON (AY.8) is immune |
| PROTOCOL_VERSION | 20 -> 22 (d79fd746, 99c23cd1); versions the bincode client socket, not the NDJSON API | See "PROTOCOL_VERSION and compatibility" |
| Windows PATH | Installer prepends the versioned release dir to the user PATH (8162e265, `distribution/install.ps1:881-904`); `%LOCALAPPDATA%\Programs\Herdr\bin` stays as a junction alias | Never persist a resolved `herdr.exe` path; resolve per spawn; configured `binary_path` may point at the alias |
| CRT | x86_64 build statically links the CRT (73825652) | No VC++ redistributable needed; arm64 not covered |
| `--no-session` | Removed (207be3c7) | Never passed by atm; nothing to do |
| `herdr server` | Entry unchanged (`src/main.rs:542`); `run_server` moved to `src/server/headless/bootstrap.rs` (207be3c7) with identical duplicate detection and restore | No change |
| Endpoint resolution: `ipc.rs`, `session.rs`, `api/client.rs`, `socket_paths.rs` | Byte-identical across the range | No change |
| NDJSON envelope, `agent.*` shapes, limits, `notification show`, CLI argv and exit codes | No change; additive methods and fields only (`ping.capabilities.endpoint_protocol_generation`, `workspace.close.close_group`, `worktree.*.trust_repository`, new `pane.*`, `command.invoke`, `integration.list`) | Parsers must tolerate unknown fields; AY.1 asserts it |
| `events.subscribe` | Starts at the live sequence, no replay (20a500a7) | Irrelevant unless AY.8 subscribes; documented |
| Autostart | Still none at HEAD (grep for login item / LaunchAgent / RunAtLoad / autostart / schtasks / Register-ScheduledTask / systemd / SMAppService: only SSH keepalive hits) | Confirms the installer-owned start-at-login entry design |
| API pipe ACL | Unchanged: `restrict_socket_permissions` is a no-op on Windows (`src/ipc.rs:342-345`); the SDDL DACL at `ipc.rs:141-167` serves only the remote-attach bridge | AY.8 boundary revision records the API pipe as default-DACL |

Summary for the six operations atm uses (`agent prompt`, `agent wait`,
`agent get`, `agent list`, `notification show`, `status server --json`):
argv, flags, JSON shapes, exit codes and error-code set are unchanged
from v0.8.2 to master. The one material behaviour change is the `--wait` gate.

## Herdr facts (IPC, session, environment; verified at v0.8.2 and master 3a822e81)

Read from `src/ipc.rs` and `src/session.rs`; both files are byte-identical
between v0.8.2 and 3a822e81. AY.1 adds the v0.8.0 column.

- Endpoint resolution order in Herdr's own client: explicit `--session` >
  `HERDR_SOCKET_PATH` > `HERDR_SESSION` > default (`src/session.rs:173-181`;
  env constants `src/api/mod.rs:20`, `src/session.rs:10-11`). Default path
  is `<config_dir>/herdr.sock`, named session
  `<config_dir>/sessions/<name>/herdr.sock` (`src/session.rs:161-171`);
  `config_dir` on Windows is `%APPDATA%\herdr` (`src/config/io.rs:30-59`),
  `herdr-dev` in debug builds. Session names: ASCII alnum plus `. _ -`,
  max 64 bytes (`src/session.rs:13,425-446`).
- Windows pipe name: the resolved path string is used verbatim as an
  `interprocess` namespaced name (`src/ipc.rs:44-51`), so the endpoint is
  `\\.\pipe\C:\Users\<user>\AppData\Roaming\herdr\herdr.sock` (or the
  `sessions\<name>\` variant, or `\\.\pipe\<HERDR_SOCKET_PATH>`). The
  `.sock` path still exists on disk on Windows as a marker file holding
  `<pid>:<nanos>` (`src/ipc.rs:76,326-333`). Because the default derives
  from the account's `%APPDATA%`, a Herdr and a daemon running under
  different Windows accounts (or a service in session 0) resolve
  different pipes. This is why the daemon must run per-user (below).
- Transport: `interprocess` 2.4.2 local sockets, named pipe on Windows
  and UDS on Unix (`src/ipc.rs:10-11,137,247`). Framing is identical: one
  NDJSON request line, one JSON response line (`src/api/client.rs:158-174`);
  server caps the request at 1 MiB with a 5 s initial deadline
  (`src/api/server.rs:28-32,510-554`). PROTOCOL_VERSION 20, checked by
  `src/cli/protocol_guard.rs:16-43` (`protocol_mismatch`).
- Timeouts: the default CLI request path sets no read/write timeout
  (`src/api/client.rs:55-61`), and on Windows an `Unsupported` timeout
  failure is swallowed (`src/api/client.rs:115-120`). atm's own process
  deadline plus kill-and-reap is the only bound on a hung `herdr.exe`;
  the Windows kill/reap correctness item (AY.7) is mandatory, not hygiene.
- ACL: Unix API socket is chmod 0600 (`src/server/socket_paths.rs:12`);
  on Windows `restrict_socket_permissions` is a no-op for the API pipe
  (`src/ipc.rs:342-345`); the same-user SDDL DACL exists only on the
  private remote-attach listener (`src/ipc.rs:143-168`). AY.8 must treat
  the Windows API pipe as reachable by any local user and record that in
  the boundary revision; doctor reports it.
- Child env: every pane gets `HERDR_SOCKET_PATH`, `HERDR_BIN_PATH`,
  `HERDR_PANE_ID`, `HERDR_TAB_ID`, `HERDR_WORKSPACE_ID`
  (`src/integration/env.rs:28-33`); `HERDR_SESSION` is NOT propagated to
  panes, and `HERDR_SOCKET_PATH` outranks `HERDR_SESSION`
  (`src/session.rs:82-83`). `HERDR_ENV=1` blocks nested TUI/client
  launches (`src/main.rs:478-482`) but `herdr agent ...` subcommands are
  handled before that guard, so a `herdr` client spawned from inside a
  pane works.
- `herdr server` (`src/main.rs:542` at HEAD, `run_server` in
  `src/server/headless/bootstrap.rs`; help text "Run as headless server")
  is a foreground event loop suitable for a supervisor such as launchd. It restores persisted workspaces and
  pane PTYs and seeds a startup workspace when none exists; a second
  instance exits 1 with "herdr server is already running". Herdr's GUI,
  launched over a running server, attaches as a client. Herdr's own
  autodetect launch spawns the server detached (setsid equivalent, null
  stdio, 15 s ready timeout). Neither v0.8.2 nor master 3a822e81 has a
  login-item or autostart feature of its own (see drift table), which is
  why the start-at-login entry below is installed by atm's installer.
  `--no-session` no longer exists (207be3c7); atm never passes it.

## Herdr facts (CLI parity; verified at v0.8.2 and master 3a822e81)

- Exit codes are platform-independent code: 0 success (JSON on stdout),
  1 server error, `server_not_running` or `protocol_mismatch` (JSON on
  stderr), 2 usage (`src/cli.rs:738-746`, `src/main.rs:559-570`,
  `src/cli/agent.rs:762-816`). Dead-socket classification is
  ErrorKind-only and commented as deliberately transport-neutral
  (`src/cli.rs:816-824`). `herdr status` exits 0 even with no server, so
  doctor probes with `agent list`, not `status`.
- Output is single-line compact JSON, LF only (`println!`, no CRLF
  handling anywhere under `src/cli*`): `{"id":"cli:agent:<cmd>","result":{"type":...}}`
  with `agent_list`/`agent_info`/`agent_prompted`, error
  `{"id","error":{"code","message"}}` (`src/api/schema/response.rs:30-109`).
  atm's parsers may still tolerate a trailing `\r` defensively.
- Argv is hand-rolled positional: `agent prompt <target> <text> [--wait]
  [--until S]... [--timeout MS]`, `agent wait <target> [--until S]...
  [--timeout MS]`, `agent get <target>`, `agent list` (no args). `--timeout`
  is milliseconds at v0.8.0, v0.8.2 and HEAD (`src/cli/spec.rs:365-367`
  at HEAD); omitted means wait indefinitely; `--flag=value` is not
  accepted for agent commands (`src/cli/agent.rs:424-545,757-826`). Argv,
  flags, output and exit codes of `agent prompt|wait|get|list` and
  `notification show` are unchanged through 3a822e81. atm's builders
  (`crates/atm-herdr/src/lib.rs:631-657`) match this exactly, and
  HR-CORE-003 in `docs/atm-herdr/requirements.md` already reads
  `--timeout <ms>`; there is nothing to correct there (the previous
  revision's "doc slip" claim was wrong and is withdrawn).
- `agent prompt --wait` semantics changed after v0.8.2 (drift table): the
  wait resolves on an observed `working` or `blocked`, a prompt into an
  already-working agent skips the gate, and stalls surface as `timeout`
  or `agent_prompt_stalled` with new message text. atm keys on error
  codes, never message text; AY.1 confirms that for every
  `HerdrError` mapping, and the fake-herdr fixture replays both the
  v0.8.x and the master outcome (rule 3).
- The only CLI platform branches are `agent start` busy-pane retry
  (`src/cli/agent.rs:644-674`, unused by atm) and Kitty keyboard flags.
  One behavioural divergence: on a closed stdout, Unix dies by SIGPIPE
  silently while Windows panics (exit 101) because `begin_cli_output` is
  a no-op there (`src/platform/mod.rs:204-208`). atm captures stdout to
  a buffer, so it is not hit; the audit records it.
- Packaging: release builds `x86_64-pc-windows-msvc` only, as
  `herdr-windows-x86_64.zip` containing `herdr.exe` plus a `conpty/`
  sibling directory that must stay together (`release.yml:64-68,159-175`,
  install.mdx:96). The installer places releases under
  `%USERPROFILE%\.herdr\packages\standalone\releases\<ver>\` and, since
  8162e265, prepends that versioned directory to the user PATH; the
  `%LOCALAPPDATA%\Programs\Herdr\bin` junction remains as a stable alias
  to the current release. A PATH-resolved `herdr.exe` therefore goes stale
  after an update, and a daemon started before the update inherits the
  old PATH: atm resolves the binary on every spawn, never persists the
  result, and the recommended `binary_path` on Windows is the alias
  directory. The configured-binary-path option must accept a
  directory-resident exe, never copy `herdr.exe` alone; `herdr.exe` is a
  real exe, not a `.cmd` shim, and needs no VC++ redistributable on
  x86_64 (73825652). ARM64 Windows runs the x86_64 build under emulation. No
  code signing: SmartScreen prompts are expected on first run, which
  matters for the FastPC4 setup.

Open audit rows (answered by AY.1, none of them gates a
design decision in this plan):

| Question | Needed by |
|---|---|
| CRLF vs LF on stdout/stderr JSON observed on a real Windows run | AY.7 audit run |
| Detached-child stdio and console behaviour of `herdr.exe` spawned by the daemon | AY.7 |
| Pipe ACL / impersonation model in practice (same-user vs any local user) | AY.8 boundary revision |

## Startup, environment and failure model (normative)

This section is the contract every AY sprint implements and every QA pass
checks. It applies to all three platforms.

### Configured or not

atm is **Herdr-configured** on a host when the roster has at least one
member whose local backend is Herdr:
`MemberSummary::local_message_received_backend()` is
`Some(LocalMessageReceivedBackend::Herdr { .. })` (team_admin.rs:82-88,
delivery_channel.rs:148-187 at a7aebefb8; roster metadata `backendType ==
"herdr"`). There is **no separate enabled flag** (AYP-R5-002): roster
backend selection is the enable signal, as today, and `RosterHarness` has
no Herdr variant (atm-storage contract.rs:448-467). The optional `[herdr]`
section of `$HOME/.atm.toml` (`binary_path`, `socket_path`) is tuning
only, read by atm-daemon-bootstrap beside its existing `atm_temp_config.rs`
reader of the same file. Otherwise atm is **not Herdr-configured** and
nothing in this section applies: no Herdr traits are built, nothing Herdr-related is
installed, doctor prints one line "herdr: not configured".

### Installer (`daemon-switch`, service install)

- Herdr-configured: the operator runs `daemon-switch herdr-entry
  install` after the atm service is installed. It installs one **Herdr
  start-at-login entry per distinct configured endpoint** (AYP-R3-003):
  the default endpoint runs `herdr server`; a named session `<name>`
  runs `herdr --session <name> server` (Herdr applies `--session`
  globally before dispatching `server`: `src/session.rs`
  `configure_from_args`, `src/main.rs:579` at v0.8.2). The distinct
  endpoints are the sessions named by roster members with the Herdr backend plus the default when any such member names none. Only the
  endpoint derived from a configured `herdr.socket_path` gets no entry
  (named-session endpoints on the same host do, AYP-R5-005): atm cannot know which Herdr
  invocation owns that path, and doctor says so (state
  `HERDR_SOCKET_PATH_NO_ENTRY`, remedy "start Herdr with the same socket
  override"). macOS: one LaunchAgent per entry with `RunAtLoad` and no
  `KeepAlive` (an attempt at login, not supervision; a duplicate exits 1
  once and nothing loops when a GUI Herdr is already up). Linux: the
  equivalent user unit. Windows: a per-user logon task under the same
  account as the atm daemon. Identifiers are fixed and deterministic:
  `com.randlee.atm.herdr-server` / `com.randlee.atm.herdr-server.<name>`
  (LaunchAgent label), `atm-herdr-server.service` /
  `atm-herdr-server@<name>.service` (user unit), `ATM Herdr Server` /
  `ATM Herdr Server (<name>)` (logon task); session names are ASCII
  alnum plus `. _ -`, max 64 bytes, so they are identifier-safe. launchd
  and Windows give no ordering guarantee between the Herdr and atm
  entries, and none is needed (below).
- Not Herdr-configured: `herdr-entry install` refuses and nothing is
  written. Switching a host from configured to not configured is the
  operator running `herdr-entry remove`, which removes only the
  marker-bearing entries atm installed; until then doctor reports "entry
  present, Herdr not configured" with that remedy. atm never removes
  anything it did not install, and nothing installs or removes an entry
  implicitly.
- Windows deployment is **per-user** (logon task in the user's session,
  same account as Herdr). A Windows service in session 0 or under a
  different account resolves a different default pipe and is out of
  scope; `daemon-switch` refuses that configuration with an actionable
  message rather than installing it. The reverse order (daemon already
  installed under another account or as a service, Herdr configured
  later) is caught by the same predicate at the next `daemon-switch`
  run and by doctor, which reports the account mismatch as a finding
  with the remedy "reinstall per-user"; nothing is migrated
  automatically.
- The installer never starts Herdr itself and never checks whether Herdr
  is running. Herdr owns its socket, session state and restart.

### Daemon startup

- The daemon builds the Herdr traits from configuration and roster data
  at composition time (the single `HerdrProcessInvoker::new` site in
  `atm-daemon-bootstrap/src/replacement_handler.rs` today). No probe, no
  wait, no launch, and no Herdr version is chosen at startup: every call
  is a fresh process against whatever Herdr is there, so a Herdr upgrade
  while the daemon runs needs no daemon restart. Readiness does not
  depend on Herdr in any way.
- Normal case: Herdr started at login and owns the default session
  socket; the daemon starts under launchd afterwards in the same user
  session with no Herdr variables in its environment; the `herdr` client
  it spawns falls back to Herdr's default socket, which is the socket
  Herdr just created. Nothing to detect, nothing to resolve.
- Explicit configuration (optional, daemon config and roster):
  `herdr.transport` (`"cli"` or `"socket"`; the reader accepts the key in
  AY.9, defaults to `"socket"` after cutover, and rejects every other
  value as `ConfigParseFailed`), `herdr.binary_path` (absolute path or directory containing `herdr` /
  `herdr.exe`; default: PATH lookup by name as today), `herdr.socket_path`
  (absolute path of Herdr's API socket, on Windows the path string Herdr
  was started with; default: none) and, per roster member, the Herdr
  session (default: none). Endpoint precedence per call (AYP-R3-002) is
  the order Herdr's own client uses (`--session` > `HERDR_SOCKET_PATH` >
  `HERDR_SESSION` > default, `src/session.rs:80-95` at v0.8.2): the
  member's session wins; otherwise the configured `socket_path`;
  otherwise Herdr's default. On the CLI transport the child receives
  exactly one of `HERDR_SESSION=<session>` (HR-CORE-006, retained) or
  `HERDR_SOCKET_PATH=<socket_path>`, never both (Herdr would let the path
  outrank the session); on the socket transport (AY.8) the same rule
  selects the explicit path or the session-derived path. Config load
  rejects a relative `socket_path`. Doctor reports each endpoint with
  its provenance (`session`, `socket_path`, `herdr_default`). When
  nothing is configured the child inherits the daemon's ambient
  environment unmodified. HR-CORE-006 is **retained unchanged**; the
  previous revision's deletion of it is withdrawn. The CLI fallback is
  retained through atm 1.5.x and removed in atm 1.6.0. atm never reads
  `HERDR_SESSION` or `HERDR_SOCKET_PATH` from its own environment to
  synthesize a choice (requirements.md:153-160). Tests: AY.2 (env
  mapping and exclusivity, relative-path rejection), AY.3 (doctor
  provenance per endpoint), AY.8 (socket target selection), AY.9 (round
  trip through a configured `socket_path` against the fake socket
  server).

### Failure behaviour (Herdr missing, not running, crashed)

- Every Herdr harness call is bounded (HR-SAFE-002) and fails with the
  existing closed `HerdrError` enum: `ServerNotRunning` (Herdr's
  `server_not_running` code), `ServerUnavailable` (binary not found;
  cause names what was searched), `ProtocolMismatch`, `Timeout`. **No new
  variant is introduced**; the previous revision's
  `HerdrError::NotRunning` is `ServerNotRunning`.
- Infrastructure-class failures open the ADR-058 D10.1 breaker
  host-wide, as today. Herdr-harness nudges then fail fast with the
  breaker error; tmux and hermes nudges, mail delivery, the CLI and
  doctor are unaffected. The daemon keeps running.
- Doctor `herdr` section is one exhaustive typed state **per configured
  endpoint** (AY.3; AYP-R3-003): the endpoints are the sessions named by
  roster members with the Herdr backend plus the default endpoint when any
  such member names none (or when `herdr.socket_path` is set). Rendered
  for humans and as `atm doctor --json`: `herdr.configured: bool`,
  `herdr.endpoints[]` (per endpoint: `session` or `"default"`,
  provenance, transport kind, `endpoint` (resolved socket/pipe name on
  the socket transport, `null` on CLI), binary resolution with
  provenance `"configured"`/`"path"`, server `version`/`protocol` from
  `herdr status server --json` (CLI) or `ping` (socket), `state`,
  `remedy`, `capabilities.live_handoff` (true/false/null), `members[]`
  (entries carry exactly `name` and `outcome`: no `member` wrapper, no
  `ordinal`, AYP-R8-002); ordered
  `default` first, then sessions sorted bytewise, so
  the JSON is reproducible), and **no aggregate** `herdr.state` /
  `herdr.remedy` (AYP-R4-002): `daemon-switch` consumes
  `herdr.configured` and iterates `herdr.endpoints[]`, and `herdr.breaker` (the existing AX.6
  `HerdrBreakerDoctorReport`: state, retry_after_ms,
  consecutive_failures; the breaker is host-wide and is **not** folded
  into the endpoint state, AYP-R3-006). The probe is one `herdr status server --json` (CLI) or one `ping`
  (socket) per endpoint plus the existing bounded `get` per Herdr-backend
  member of that endpoint (exactly today's presence probe,
  replacement_handler.rs:125-206), run by doctor itself under
  `BreakerPolicy::Bypass`, so an open breaker never hides the live
  endpoint state (test). Doctor does **not** compute Herdr's socket or
  pipe path under the CLI transport; it reports what Herdr says.

  ```rust
  // crates/atm-core/src/doctor/herdr_state.rs. The DTOs below (plain data, serde derives,
  // no I/O) land in AY.3 with HerdrDoctorProbe, which returns and consumes them; the
  // HerdrEndpointDoctor port, ClosedHerdrEndpointDoctor, presence_findings and
  // herdr_configured.rs are also AY.3. atm-core owns the DTOs and
  // the port because atm-core -> atm-herdr is a forbidden edge
  // (boundary_enforcement.rs:47); atm-herdr, which already depends on atm-core,
  // fills them. HerdrEndpointDoctor REPLACES HerdrPresenceDoctor (doctor/mod.rs:42;
  // AYP-R4-003): one port, one probe path. The presence DoctorFindings that
  // HerdrPresenceDoctor produced are derived in atm-core from the observations
  // (fn presence_findings(&[HerdrEndpointObservation]) -> Vec<DoctorFinding>, from the typed
  // per-member outcomes each endpoint carries; AYP-R5-001/AYP-R6-002), so no member is
  // probed twice. HerdrBreakerDoctor (doctor/report.rs:262) is unchanged. Every
  // variant carries its remedy via `fn remedy(&self) -> &'static str`; tests
  // iterate every variant on the CLI transport (AY.3) and again on the socket
  // transport (AY.9).
  pub struct HerdrVersion(pub String);                 // as Herdr reports it; semver compare lives in atm-herdr
  pub enum HerdrTransportKind { Cli, Socket }
  #[serde(rename_all = "snake_case")]
  pub enum HerdrEndpointProvenance { Session, SocketPath, HerdrDefault }
  pub struct HerdrEndpointObservation {
      pub session: Option<HerdrSession>,               // None = default endpoint
      pub provenance: HerdrEndpointProvenance,
      pub transport: HerdrTransportKind,
      pub endpoint: Option<String>,                    // AYS-R3-001: Display form of the resolved socket/pipe name,
                                                       // filled by atm-herdr from its own HerdrEndpoint (AY.8/AY.9);
                                                       // None on the CLI transport. HerdrEndpoint never enters atm-core.
      pub binary: Option<(PathBuf, &'static str)>,     // resolved path + "configured" | "path" (CLI only)
      pub state: HerdrDoctorState,
      pub live_handoff: Option<bool>,                  // AYP-R5-004/AYP-R6-001: a probe fact, independent of `state`:
                                                       // Some(capabilities contains "live_handoff") whenever server_info/ping
                                                       // returned (Ok, BelowMinimum, ClientServerMismatch); None only when
                                                       // capability retrieval failed. JSON: endpoints[].capabilities.live_handoff
      pub members: Vec<HerdrMemberPresence>,           // AYP-R5-001/AYP-R6-002: one entry per Herdr-backend roster member
                                                       // routed to this endpoint, never cleared by `state` (BelowMinimum
                                                       // still probes: calls continue on Herdr's terms)
  }
  pub struct HerdrRosterMember { pub ordinal: usize, pub name: AgentName }   // probe INPUT only (observe); ordinal = index in
                                                                             // MembersList.members; never serialized
  pub struct HerdrMemberPresence {
      #[serde(skip)] pub ordinal: usize,               // copied from the HerdrRosterMember: an ordering detail, not schema
      pub name: AgentName,
      pub outcome: HerdrPresenceOutcome,               // internally tagged snake_case serde shape; snapshot-pinned
  }
  // JSON: endpoints[].members[] entries are exactly {"name", "outcome"}; the AY.3 snapshot test
  // asserts there is no `member` wrapper and no `ordinal` key (AYP-R8-002).
  #[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
  pub enum HerdrPresenceOutcome {
      Visible,                                         // `get` succeeded; any AgentStatus (Blocked included) is visible, as today
      Finding(DoctorFinding),                          // any non-infrastructure HerdrError, AgentNotFound included: the exact
                                                       // DoctorFinding the AX adapter emits today, built in atm-herdr by
                                                       // `presence_finding` (replacement_handler.rs:181-206
                                                       // `herdr_presence_finding` moved verbatim: typed AtmErrorCode, catalog
                                                       // remediation, emission_outcome text). DoctorFinding (doctor/report.rs:31,
                                                       // PartialEq) is an existing atm-core type; nothing is rebuilt from strings.
      Infrastructure { reason: String },               // is_infrastructure(): reason = format!("{error:?}"), as today
  }
  // presence_findings (atm-core) is the AX loop (replacement_handler.rs:125-154) run over the
  // flattened observations (AYP-R7-001): collect every HerdrMemberPresence of every endpoint,
  // sort by `ordinal` (roster order restored), push each Finding(f) in that order,
  // remember only the FIRST Infrastructure reason across all endpoints, and append exactly
  // one Info HerdrUnavailable "Herdr presence probe skipped: {reason}" after the loop;
  // Visible -> nothing. Never one Info per endpoint. Zero-regression tests compare the whole
  // ordered Vec<DoctorFinding> (PartialEq) with the AX adapter's output on the same
  // fake-herdr recording: visible (Idle/Working/Blocked/Done), not-visible, failed (e.g.
  // AgentBlocked), one and several infrastructure-failed members, and a two-endpoint fixture
  // (default plus one session) with interleaved roster failures and two distinct
  // infrastructure reasons (only the first reason appears, once).
  #[serde(tag = "kind", rename_all = "snake_case")]
  pub enum HerdrDoctorState {
      Ok { version: HerdrVersion, protocol: u32 },
      NotConfigured,
      BinaryNotFound { searched: Vec<PathBuf> },
      BinaryNotExecutable { path: PathBuf, cause: String },   // exists, not a file / no exec bit / not herdr
      BelowMinimum { version: HerdrVersion, minimum: HerdrVersion },
      ServerNotRunning { endpoint_named_by_herdr: Option<String> },
      ClientServerMismatch { client: Option<HerdrVersion>, server: Option<HerdrVersion> }, // protocol_mismatch
      EndpointUnreachable { endpoint: String },               // socket transport: refused / absent
      PermissionDenied { endpoint: String },
      ProbeTimedOut { after: Duration },
      UnexpectedResponse { code: Option<String>, detail: String },    // malformed JSON, oversized line, Advisory, InternalError
      Other { error: String },                                        // HerdrError variants not listed above
  }
  // No BreakerOpen variant: the breaker is `HerdrBreakerDoctorReport` (AX.6, unchanged).
  // ADR-001 workspace-convention seal (AYP-R4-003): Sealed supertrait; `pub mod sealed`
  // visibility unchanged. Machine-readable record boundaries/atm-core/herdr-endpoint-doctor.toml
  // (BOUNDARY-HerdrEndpointDoctor; [implementation] visibility = "trait_only";
  // [dependencies] allowed_dependents = ["atm-daemon-bootstrap"]; forbidden_edges include
  // "atm-core -> atm-herdr"), discovered by boundary_enforcement.rs core_boundary_files()
  // and linked from a `## HerdrEndpointDoctor` section of docs/atm-core/boundaries.md; an
  // architecture test counts `impl HerdrEndpointDoctor for` across the workspace and
  // requires exactly two: ClosedHerdrEndpointDoctor (atm-core) and
  // HerdrEndpointDoctorAdapter (atm-daemon-bootstrap). The TOML is P-E's AY.3 file.
  pub trait HerdrEndpointDoctor: crate::boundary::sealed::Sealed + Send + Sync {
      fn observe<'a>(&'a self, roster: &'a MembersList, caller_deadline: RequestDeadline)
          -> Pin<Box<dyn Future<Output = Vec<HerdrEndpointObservation>> + Send + 'a>>;
  }
  pub struct ClosedHerdrEndpointDoctor;   // replaces ClosedHerdrPresenceDoctor; returns an empty Vec
  // Implemented once, in atm-daemon-bootstrap (replacement_handler.rs:
  // HerdrEndpointDoctorAdapter replaces HerdrPresenceDoctorAdapter) over atm-herdr's
  // public `HerdrDoctorProbe` (AY.3). `MembersList` (team_admin.rs:93) is the doctor's
  // view of one `load_team_roster` RosterSnapshot, built by
  // `ordered_roster_member_summaries` (doctor/mod.rs:679-698); the same instance feeds
  // `herdr_is_configured` below (AYS-R3-003).
  ```

  The `HerdrError` to `HerdrDoctorState` mapping lives in atm-herdr
  (`crates/atm-herdr/src/doctor_probe.rs`), the crate that owns
  `HerdrError` and `HERDR_MINIMUM_VERSION`; it covers every `HerdrError`
  variant (`Other` is the exhaustive-match fallback; a test asserts no
  listed variant maps to `Other`).
- Architecture and lifecycle tests (AY.4): (a) daemon starts and reaches
  ready with no `herdr` binary on PATH and none configured, and a tmux
  nudge and a hermes graft nudge still succeed in that state; (b) same
  with a fake `herdr` that always returns `server_not_running`: the
  breaker opens, doctor reports it, the daemon stays up; (c) no code path
  in atm-daemon-bootstrap or atm-http-runtime spawns `herdr server` or
  any long-lived Herdr child (grep guard in boundary_enforcement.rs); (d)
  `daemon-switch` in the not-configured case writes no Herdr entry.

### Herdr starts after atm has been running

No special handling exists or is needed; the plan states the three
mechanisms that make late start self-healing, and AY.4 tests them:

1. Every call spawns a fresh `herdr` client process (AY.8: a fresh
   connection per command). There is no session or connection state to
   re-establish, and the traits were configured from roster data, not
   from Herdr's runtime state.
2. The ADR-058 breaker goes half-open after one backoff window
   (ADR-058:650), so the first successful call closes it; recovery is
   detected within one backoff window of Herdr coming up.
3. Mail queued while Herdr was down stays queued. The queue-wake pump
   (`HERDR_POLL_INTERVAL_MS = 5_000`) and the AX.5 reminder cycle
   re-nudge idle members with pending work on their cadence, so the
   backlog drains without any startup code.

The one case that does not self-heal is configuration mismatch (Herdr
started on a named session while atm is configured for the default, or a
different account on Windows). Doctor is where that shows up, as a
`server_not_running` probe result next to the configured values.

## Upgrade and restart coordination (normative)

Rand, 2026-09-05: "This needs to be well understood so other users have a
good user experience." Herdr facts this section rests on (master
3a822e81; `docs/next/website/src/content/docs/session-state.mdx:14-15,91-103`,
`troubleshooting.mdx:62-71`, `src/update.rs:835-1220`,
`src/cli/server.rs:196-259`):

- `herdr update` without `--handoff` keeps a compatible running server
  and, for a restart-required server, needs a stop/restart. **Stopping a
  Herdr server exits every pane process**, i.e. every agent in that
  session; session restore then rebuilds panes and resumes agents that
  have official session references.
- `herdr update --handoff` (experimental, opt-in) performs a live
  handoff: pane PTYs and processes, agent identity and durable metadata
  survive the server replacement. In-flight CLI/API requests and waits
  may be interrupted; "clients should reconnect and retry". The running
  server advertises `capabilities.live_handoff`.
- After an update without a restart, the updated `herdr` binary rejects
  every command against the old server with `protocol_mismatch` (see
  "PROTOCOL_VERSION and compatibility").

What atm does in each case. None requires an atm daemon restart and none
is initiated by the daemon; every row reuses behaviour that already
exists (breaker, queue pump, reminder cadence, AX.6 escalation notify,
doctor).

| Event | atm daemon | Agents in panes | Operator experience |
|---|---|---|---|
| Herdr updated, old compatible server kept running | Herdr nudges fail `ProtocolMismatch`; breaker opens; mail stays queued | Unaffected | `atm doctor`: "Herdr binary X, running server Y: run `herdr update --handoff` or restart Herdr"; one escalation through the AX.6 helper (`herdr_escalation::escalate`: queued ATM mail to the lead and configured recipients, Herdr desktop notify best-effort, no mail body) when the breaker opens; nudges resume within one backoff window after the restart |
| `herdr update --handoff` | A call in flight fails with an infrastructure-class error; **a prompt whose submission is unknown is never retried automatically** (prompts are not idempotent); the queue pump and reminder cadence re-nudge pending mail, which is idempotent | Survive | Nothing to do; at most one reminder cadence of delay |
| Herdr server stopped and restarted without handoff | `ServerNotRunning` during the stop, then `AgentNotFound`/`AgentNotRunning` for panes that did not come back; mail stays queued; the AX.5 reminder threshold and AX.6 escalation notify the lead about members that stay unreachable | Exit; restored only as far as Herdr's session restore reaches | `atm doctor` lists Herdr members with no live pane; the lead is escalated through the existing channel |
| System restart | Started by the atm service entry | Restarted by Herdr's start-at-login entry and session restore | Order between the two entries does not matter |
| atm daemon upgrade (`daemon-switch`) | Restarted by `daemon-switch` as today; queued mail persists in SQLite; the pump drains it after startup | Unaffected (atm is a client) | Herdr is never restarted for an atm upgrade |
| Both upgraded together | Coordinated restart below | | |

Coordinated restart (Rand's suggestion; operator-invoked, thin wrapper,
no daemon logic, **restart only**, **one endpoint per invocation**,
AYP-R4-001). When atm is Herdr-configured, `daemon-switch` gains
`--restart-herdr <endpoint>`, where `<endpoint>` is `default` or a
configured session name. It never runs `herdr update`: upgrading Herdr is
the operator's own Herdr operation; atm only restarts what is installed.

0. Resolve the endpoint against `atm doctor --json` `herdr.endpoints[]`:
   a selector naming no configured endpoint is refused
   (`HERDR_RESTART_ENDPOINT_UNKNOWN`); an omitted selector is accepted
   only when exactly one endpoint is configured, otherwise refused
   (`HERDR_RESTART_ENDPOINT_REQUIRED`, the message lists the endpoints);
   an endpoint whose `provenance` is `socket_path` is refused
   (`HERDR_RESTART_SOCKET_PATH`: atm does not own that Herdr invocation);
   named-session endpoints on the same host are eligible, since a
   member's session outranks `socket_path` (AYP-R5-005). Every Herdr
   command below is scoped to the selected endpoint: `herdr server ...`
   for `default`, `herdr --session <name> server ...` for a session; the
   entry is resolved by the one `herdr_entry_identifier(platform,
   endpoint)` helper that `herdr-entry install/remove/status` also use
   (macOS `com.randlee.atm.herdr-server[.<name>]`, Linux
   `atm-herdr-server[@<name>].service`, Windows `ATM Herdr Server[
   (<name>)]`), never rebuilt in restart code (AYP-R5-006).
1. If the installed `herdr` binary is newer than that endpoint's running
   server (its own `version` from doctor) and doctor's
   `endpoints[].capabilities.live_handoff` is `true` for it (AYP-R5-004;
   `null` counts as unknown and takes the stop path; that endpoint's state
   is normally `ClientServerMismatch` here, which is why the capability is
   carried independently of state, AYP-R6-001; no second probe), run the
   scoped `server live-handoff` so agent panes
   survive; report Herdr's own result and stop on failure (Herdr's
   rollback owns the socket state).
2. Otherwise print that stopping the server exits every agent pane on
   that endpoint and require an explicit `--stop-herdr-panes`
   acknowledgement before the scoped `server stop` and relaunch through
   that endpoint's installed start-at-login entry (`launchctl kickstart`
   on macOS, the user unit on Linux, `schtasks /Run` on Windows); re-run
   doctor and exit 0 only when that endpoint reports `Ok`.
3. `--restart-herdr` never restarts the atm daemon. With several
   endpoints the operator repeats it per endpoint, then runs the atm
   daemon restart as today; that restart step refuses
   (`HERDR_RESTART_ENDPOINTS_PENDING`) while doctor reports any
   configured endpoint in `ClientServerMismatch`, so atm is never
   restarted over a half-upgraded set. The daemon knows nothing about
   steps 0 to 2; it finds a working Herdr on its first call.

`daemon-switch` never does this implicitly, the daemon never does it at
all, and the flag is rejected when atm is not Herdr-configured. This
keeps ruling 1 (atm is a client) and gives operators one command for the
"restart both" case. Implementation budget: shell-level sequencing of
existing Herdr commands plus string output; the only state is the
ownership marker on the entry `daemon-switch` installed (below); no
polling loops beyond Herdr's own completion.

Governance (AYP-R2-005, AYP-R3-005): the Herdr entry is outside
REQ-P-DAEMON-SWITCH-001 (which covers only the matched ATM pair's
temporary typed overlay). AY.5 therefore lands, as its first deliverable,
an amendment approved with this plan: `REQ-P-DAEMON-SWITCH-002` and an
ADR-053 addendum defining a separate, explicit subcommand `daemon-switch
herdr-entry {install,remove,status}`. It is **operator-invoked only**:
`daemon-switch` has no service-install step today (it switches an
already-managed service), so there is no implicit hook and none is
added; the skill doc's install procedure lists `herdr-entry install` as
the step after the ATM service is installed, and the configured to
not-configured transition is the operator running `herdr-entry remove`,
prompted by doctor's stale-entry finding. Identifiers per endpoint and
the ownership marker inside each entry (comment/label `managed-by=atm
daemon-switch`) are in "Installer". The owned object is not one file
(AYP-R3-005): LaunchAgent plist plus bootstrap state (macOS), user unit
file plus `enable` state (Linux), scheduled-task definition plus enabled
state (Windows). AY.5 gives it its own small transaction, one journal
record type stored in the ADR-053 journal directory (never the ATM
overlay journal itself): `plan (render object, digest) -> durable
journal {entry id, platform, digest, phase} -> atomic write ->
register (launchctl bootstrap / systemctl --user enable / schtasks
/Create) -> verify (status shows the object with the marker) -> journal
complete`. `remove` reverses it (unregister, then delete only a
marker-bearing object). An incomplete journal makes `install`/`remove`
refuse with `HERDR_ENTRY_JOURNAL_ACTIVE` until `herdr-entry status
--repair` re-runs verify and either completes or rolls the half-written
object back (unregister plus delete). Failure-injection tests: crash
between write and register, between register and verify, foreign object
with the same identifier, marker present but digest differs (refuse,
never overwrite). `install` refuses when an entry with the same
identifier exists without the marker; `remove` is a no-op when the
marker is absent. Fail-closed on any ambiguity. That is the whole state
model.

Error inventory (AYP-R3-012, RBP-001), machine-consumed: `daemon-switch
herdr-entry *` and `--restart-herdr` print exactly one JSON object on
stdout, `{"ok": bool, "code": <CODE>, "message": str, "remedy": str,
"entries": [...]}`, and exit 0 on success, 3 on refusal, 4 on failure.
Codes: `HERDR_NOT_CONFIGURED`, `HERDR_DOCTOR_UNREADABLE` (doctor JSON
missing, malformed, `configured: null`, or non-zero exit; always exit
4, never a guess), `HERDR_ENTRY_FOREIGN`, `HERDR_ENTRY_JOURNAL_ACTIVE`,
`HERDR_ENTRY_DIGEST_MISMATCH`, `HERDR_ENTRY_REGISTER_FAILED`,
`HERDR_ENTRY_ACCOUNT_MISMATCH` (Windows service or session-0 daemon),
`HERDR_SOCKET_PATH_NO_ENTRY`, `HERDR_RESTART_ENDPOINT_REQUIRED`,
`HERDR_RESTART_ENDPOINT_UNKNOWN`, `HERDR_RESTART_SOCKET_PATH`,
`HERDR_RESTART_PANES_ACK_REQUIRED`, `HERDR_RESTART_NO_LIVE_HANDOFF`,
`HERDR_RESTART_HERDR_FAILED` (Herdr's own stderr quoted in `message`),
`HERDR_RESTART_ENDPOINTS_PENDING`. Doctor: each
`herdr.endpoints[].state` is always one of the `HerdrDoctorState`
variant names (snake_case) with that variant's `remedy` text beside it;
there is no aggregate `herdr.state` (AYP-R4-002).
Every code has a fixture and a snapshot test of the JSON shape.

User-experience contract (AY.3 through AY.6 acceptance, verified by req-qa):

- `atm doctor` herdr section ends in exactly one `HerdrDoctorState`
  variant per configured endpoint (table in "Failure behaviour"), each
  with the remedy on the same line, plus the breaker line; `atm doctor
  --json` exposes `herdr.configured`, `herdr.endpoints[]` (ordered
  `default` first, then sessions bytewise) and `herdr.breaker` for
  `daemon-switch` and tests; no aggregate state (AYP-R4-002).
- The breaker-open escalation carries the same state text through the
  AX.6 helper: queued ATM mail to the lead and configured recipients is
  the durable path (it does not depend on Herdr), Herdr desktop notify
  is an independent best-effort attempt whose failure is logged, once
  per open cycle (dedup keyed on the breaker's open timestamp), never
  the mail body (HR-SAFE-003).
- Lifecycle tests: fake Herdr enters the mismatch state mid-run (breaker
  opens, doctor text, one notification, recovery on the next half-open);
  fake Herdr resets the connection during a `wait` (no duplicate prompt,
  pending mail re-nudged by the pump); daemon restarted with queued mail
  (backlog drains); `daemon-switch --restart-herdr` rejected on a
  not-configured host and refused without `--stop-herdr-panes` when the
  fake Herdr lacks `live_handoff`.

## Sprint map, parallel tracks and stacking

Ten sprints, numbered sequentially. Each row maps to exactly one
authoritative sprint file. Relations follow
`.claude/skills/plan-hardening/sprint-planning-guidelines.md`:
`must_follow` names a development/merge dependency, while
`parallel_safe` asserts non-intersecting implementation modules, public
contracts, artifacts, and ownership.

| Sprint | Production closure | Size | Track | must_follow | parallel_safe with | Machine |
| --- | --- | --- | --- | --- | --- | --- |
| AY.1 | Audit, version ledger, requirements, ADR, architecture/history correction | S | Docs | P-A, P-B | AY.2 | any |
| AY.2 | Private CLI transport foundation plus portable fake/replay fixtures | M | Core stack | P-A, P-B | AY.1 | macOS/Linux + Windows CI |
| AY.3 | Endpoint doctor/config boundary and end-to-end activation | M | Core stack | AY.2, P-E(a) | none | macOS/Linux + Windows CI |
| AY.4 | Breaker escalation and real-composition failure/recovery lifecycle | M | Core stack | AY.3 | AY.8 | macOS/Linux + Windows CI |
| AY.5 | Transactional Herdr entry install/remove/status/repair | M | Core stack | AY.4 | AY.8 | macOS/Linux + platform fakes |
| AY.6 | Coordinated Herdr restart/live-handoff and ATM-restart preflight | M | Core stack | AY.5 | AY.8 | macOS/Linux + platform fakes |
| AY.7 | Windows process correctness and installer verification | S | Core/Windows stack | AY.6, P-C, P-D | AY.8 | FastPC4 |
| AY.8 | Direct socket/pipe transport, fake server, compatibility/equivalence | L | Socket | AY.1, AY.2, AY.3, P-E(b) | AY.4, AY.5, AY.6, AY.7 | macOS/Linux + Windows CI |
| AY.9 | Socket-default cutover, CLI fallback, doctor projection, lifecycle/CI | M | Join | AY.7, AY.8 | none | all CI lanes |
| AY.10 | Live macOS/Windows proof and phase disposition | S | Proof | AY.9, P-C, P-D | none | rand-m5 + FastPC4 |

Execution waves are explicit:

| Wave | Work that may overlap | Entry/exit rule |
| --- | --- | --- |
| 1 | AY.1 and AY.2 | Both start after P-A/P-B; AY.1 is standalone and AY.2 is the stack bottom. |
| 2 | AY.1 completion, AY.2 review, AY.3 development | AY.3 starts after AY.2 development/contracts are pushed and P-E(a) is approved; AY.2 merges before AY.3. |
| 3 | AY.3 review and AY.4 development | AY.4 is stacked after AY.3 development is pushed; AY.3 merges before AY.4. |
| 4 | AY.4 and AY.8 | AY.8 starts independently only after AY.1, AY.2, and AY.3 merge and P-E(b) is approved. |
| 5 | AY.5 and AY.8 | AY.5 stays in the linear stack; AY.8 remains an independent sibling. |
| 6 | AY.6 and AY.8 | Same independent concurrency; neither branch merges the unmerged sibling. |
| 7 | AY.7 and AY.8 | AY.7 also requires P-C/P-D and FastPC4; AY.8 has no physical-host gate. |
| 8 | AY.9 | Starts from `integrate/phase-ay` only after AY.7 and AY.8 merge. |
| 9 | AY.10 | Starts only after AY.9 merges; proof uses that merged integration SHA. |

The sole linear stack is AY.2 -> AY.3 -> AY.4 -> AY.5 -> AY.6 ->
AY.7. Use the `/gh-stack` skill for every operation on that stack.
Branches are created with `sc-git-worktree` from the immediate parent so
each child carries the parent's unmerged work. Because this is an external
worktree/PR workflow, use `gh stack link` to create or update remote stack
state, then verify actual PR bases directly:

```bash
gh stack link --base integrate/phase-ay \
  feature/ay2-herdr-transport-seam \
  feature/ay3-herdr-endpoint-doctor-config \
  feature/ay4-herdr-breaker-lifecycle \
  feature/ay5-herdr-entry-control-plane \
  feature/ay6-herdr-restart-coordination \
  feature/ay7-windows-herdr-process-installer

gh pr view feature/ay2-herdr-transport-seam --json headRefName,baseRefName,state
gh pr view feature/ay3-herdr-endpoint-doctor-config --json headRefName,baseRefName,state
gh pr view feature/ay4-herdr-breaker-lifecycle --json headRefName,baseRefName,state
gh pr view feature/ay5-herdr-entry-control-plane --json headRefName,baseRefName,state
gh pr view feature/ay6-herdr-restart-coordination --json headRefName,baseRefName,state
gh pr view feature/ay7-windows-herdr-process-installer --json headRefName,baseRefName,state
```

All commands are noninteractive. The parent-development-pushed event,
not QA, triggers a merge commit from the parent into each active child
before a development or fix round. Parent PRs merge first. Repository
policy narrows the general `/gh-stack` workflow: never run `gh stack
rebase`, `gh stack sync`, or `gh stack merge`; do not force-push; merge
each PR in dependency order with `gh pr merge --merge`.

AY.1 is standalone. AY.8 is a standalone three-parent join created from
the merged integration head; AY.9 is a standalone two-parent join; AY.10
is a standalone proof sprint that must observe the merged AY.9 head.
None is passed to `gh stack link`, and no unmerged sibling is ever merged
into one of those branches.

The sprint files below are authoritative. Each has mandatory YAML
frontmatter with exact scalar branch/worktree/stack-parent/PR-target
values, plus one authoritative list each for production-ready
deliverables, acceptance criteria, paths to delete, required validation,
and explicit non-closure. QA reviews from these files, not from the
umbrella:

- [`sprint-AY.1-herdr-audit-docs.md`](../phase-ay/sprint-AY.1-herdr-audit-docs.md)
- [`sprint-AY.2-herdr-transport-seam.md`](../phase-ay/sprint-AY.2-herdr-transport-seam.md)
- [`sprint-AY.3-herdr-endpoint-doctor-config.md`](../phase-ay/sprint-AY.3-herdr-endpoint-doctor-config.md)
- [`sprint-AY.4-herdr-breaker-lifecycle.md`](../phase-ay/sprint-AY.4-herdr-breaker-lifecycle.md)
- [`sprint-AY.5-herdr-entry-control-plane.md`](../phase-ay/sprint-AY.5-herdr-entry-control-plane.md)
- [`sprint-AY.6-herdr-restart-coordination.md`](../phase-ay/sprint-AY.6-herdr-restart-coordination.md)
- [`sprint-AY.7-windows-herdr-process-installer.md`](../phase-ay/sprint-AY.7-windows-herdr-process-installer.md)
- [`sprint-AY.8-herdr-socket-transport.md`](../phase-ay/sprint-AY.8-herdr-socket-transport.md)
- [`sprint-AY.9-herdr-socket-cutover.md`](../phase-ay/sprint-AY.9-herdr-socket-cutover.md)
- [`sprint-AY.10-herdr-live-proof.md`](../phase-ay/sprint-AY.10-herdr-live-proof.md)

Common preconditions:

- P-A: `integrate/phase-ay` exists, cut from `integrate/phase-ax` after
  PR #1204 (AX.6) has merged into it, then `develop` merged into it with
  a merge commit so the baseline contains PR #1218 (merge commit
  a7aebefb8, 2026-09-05T20:11Z: `HERDR_MINIMUM_VERSION` at
  `crates/atm-herdr/src/lib.rs:22` and ADR-061). Mechanical check before
  any AY dispatch: `git merge-base --is-ancestor a7aebefb8
  integrate/phase-ay` exits 0 (owner: fenix). AYS-R2-002 read a stale
  checkout; the fact stands, and this check makes it verifiable.
- P-B: this plan is approved by Rand (dated line in this file).
- P-C: the FastPC4 Windows `atm-dev` team exists with Herdr installed via
  the official installer and its parked reporter agent has delivered one
  round-trip report to rand-m4 or rand-m5. Owner: Rand. Network
  prerequisite (fenix@rand-m4, 2026-09-05T20:36Z, confirmed by Rand):
  FastPC4 is on neither Mac's mesh VPN and cannot be reached by ssh from
  rand-m4; Rand's own connection to it is a route ATM cannot use. So
  P-C first needs FastPC4 joined to the mesh VPN or the corporate VPN up
  on the dispatching Mac, then ATM trust entries both ways; Rand can
  bring the VPN up on request, so the proof is on demand once the team
  is installed; target date: set by Rand in this line when known.
- P-D: the Windows dev agent is named here (ATM identity on the FastPC4
  team, agent kind) by Rand. Dispatch path (Rand, 2026-09-05): Rand
  establishes the VPN connection; fenix (the atm team on rand-m5) then
  sshes into FastPC4 without a password and manages a remote Herdr pane
  there directly (`herdr agent ...` over ssh), so the FastPC4 dev agent
  is driven by fenix, not relayed through Rand. j2 assignments still go
  over ATM to `<agent>@atm-dev.fastpc4` when the cross-host link is up;
  the ssh path is the fallback and the pane-management channel. The
  parked reporter agent remains the source of truth for evidence.
- P-E (AY.3 and AY.8): two boundary rulings, each reviewed by the
  `boundary-guard` agent, run by fenix before the owning sprint is
  dispatched; devs never author boundary TOML without this ruling. Owner:
  fenix. (a) AY.3: the new `boundaries/atm-core/herdr-endpoint-doctor.toml`
  described under "Failure behaviour" (AYP-R4-003), together with AY.3's
  public-contract inventory update to the atm-herdr boundary record,
  reviewed after AY.2's transport foundation and recordings are pushed and before AY.3
  development starts; AY.2 still merges before AY.3. The approved file is
  AY.3's first commit. (b) AY.8: the revision below, reviewed after AY.3
  merges and before AY.8 dispatch; the approved diff is AY.8's first commit.
  Proposed diff to `boundaries/atm-herdr/herdr-process-adapter.toml`:

  ```toml
  [ownership]
  io_owns = [
    "tokio_process_spawn",          # CLI transport (retained until the AY.9 fallback window closes)
    "herdr_argv_construction",      # CLI transport (same)
    "herdr_local_socket_client",    # new: UDS / named-pipe NDJSON client, transport_socket.rs only
    "herdr_json_error_parsing",
    "herdr_spawn_breaker",
  ]
  ```

  `io_forbidden` is unchanged. The two CLI keys are dropped in the sprint
  that removes the CLI fallback (atm 1.6.0, after AY.9), not in AY.8.

Approval blocker (AYP-R2-011): P-C's readiness proof (date and ATM
message id of the reporter's first round-trip) and P-D's exact FastPC4
ATM identity are placeholders until Rand fills them. Plan approval
(P-B) requires both filled or an explicit dated line from Rand deferring
AY.7/AY.10 scheduling; AY.7 is never dispatched on a placeholder.

Common acceptance for every sprint: merge gate 0 blocking / 0 important /
0 minor in scope, quality-mgr PASS posted on the PR, CI green at merge
time (never a dispatch gate), no flaky-test tolerance, frozen files
untouched without a written ruling, no tokio in atm-core.

## Phase AY exit gate (AY.10 disposition)

Phase AY is not complete until a dated decision in AY.10, after the socket
cutover and live matrix,
is recorded here and in `docs/project-plan.md`, chosen by Rand from
exactly these:

- **Ship**: AY.9 merged and AY.10 live acceptance met.
- **Defer**: the live cutover proof is deferred to a named phase, ADR-058 D3 amended to say
  the CLI transport is the supported design until then; AY.8 code stays
  behind explicit config.
- **Cancel**: the AY.9 cutover is backed out, ADR-058 D3 rewritten to make the CLI
  transport the permanent design, and a cleanup PR merged before phase
  closure that deletes `transport_socket.rs`, the fake socket server,
  the `herdr_local_socket_client` ownership key, the AI.11 exemption
  line and the NDJSON columns; the AY.8 through AY.10 sprint docs marked
  superseded. The gate check for Cancel is mechanical: `test ! -e
  crates/atm-herdr/src/transport_socket.rs` on the integrate head.

The decision line has this exact shape, on its own line in this
section, so quality-mgr can check it mechanically:

```
Decision (Rand, YYYY-MM-DD): Ship|Defer <phase>|Cancel
```

quality-mgr's phase-ending gate must refuse the integrate/phase-ay to
develop PR while `grep -E '^Decision \(Rand, [0-9]{4}-[0-9]{2}-[0-9]{2}\): (Ship|Defer [A-Z]+|Cancel)$'`
finds no line in this file. This is the forcing function that AQ2.6 and
ADR-058 lacked (see AW-READY-W1).

## Request set the transport must carry (from phase AX contract)

Source of truth: feature/ax6-lead-notification-doctor (PR #1204, head
c3268df8a). Every request carries the configured session when there is
one (child env `HERDR_SESSION`, HR-CORE-006, retained); the AY.8 socket
transport derives the endpoint from that same configured session.

| Requirement | Operation | Today's argv |
|---|---|---|
| HR-CORE-002 | prompt | `herdr agent prompt <AgentName> <text>` (rendered nudge template only) |
| HR-CORE-003 | wait | `herdr agent wait <AgentName> [--until <status>]... --timeout <ms>` |
| HR-CORE-004 | get | `herdr agent get <AgentName>` (BreakerPolicy::Bypass allowed) |
| HR-CORE-005 | list | `herdr agent list` |
| HR-CORE-010 (AX.6) | notify | `herdr notification show <title> --body <body> --sound request`; mail body forbidden (HR-SAFE-003); sound fixed |
| doctor (AY.3) | server_status | `herdr status server --json` (JSON `version`, `protocol`; verified present at v0.8.0 346411fa, v0.8.2 9eb52145 and master `src/cli/status.rs`); socket: `ping`. Doctor only, never on the nudge path |

Responses: HR-CORE-007 AgentSnapshot from `result.agent`; HR-CORE-008
closed HerdrError enum keyed by Herdr error codes (unchanged by AY);
HR-CORE-009 and HR-SAFE-005..007 breaker on infrastructure-class failures
(connect/IO class on a socket). HR-SAFE-001 no send-keys fallback;
HR-SAFE-002 every call bounded; HR-SAFE-004 no durable Herdr state in
atm-herdr.

Boundary: `boundaries/atm-herdr/herdr-process-adapter.toml` io_owns
`tokio_process_spawn` and `herdr_argv_construction`. AY.2 changes neither
(pure motion inside the crate). AY.8 adds `herdr_local_socket_client`
under the P-E revision and keeps both CLI keys while the CLI path exists
(additive, AYP-R2-002); the CLI keys go when the CLI code goes. forbidden_edges stay (no
atm-core/atm-storage/rusqlite into atm-herdr; no atm-herdr into
daemon/runtime crates).

## PROTOCOL_VERSION and compatibility

The facts are in "Herdr facts (IPC...)" and the drift tables; this section
keeps only the compatibility rules that follow from them. Corrected on
2026-09-05 from the drift review. `PROTOCOL_VERSION`
(`src/protocol/wire.rs:20`, 22 at 3a822e81) versions Herdr's bincode
client socket (`herdr-client.sock`), not the NDJSON API. The NDJSON server
never checks it; it only echoes it in `ping` (`src/api/server.rs:333-372`).
What rejects a command is the `herdr` CLI itself, which compares its
compiled constant with `ping.protocol` before every request
(`src/cli.rs:762-796`, `src/cli/protocol_guard.rs:16-44`) and exits 1 with
`protocol_mismatch` on any inequality.

Consequences for atm:

- Under the CLI transport (AY.2 through AY.7) the check is Herdr's own and
  passes whenever the CLI binary and the running server come from the
  same install. After cc88b3b8, `herdr update` keeps a compatible old
  server running and `herdr status` no longer flags PROTOCOL_VERSION
  drift as needing a restart, so "new CLI, old server, every atm nudge
  fails with `protocol_mismatch`" is a normal post-update state. atm maps
  the code (existing `HerdrError::ProtocolMismatch`), the breaker opens,
  and doctor names the remedy (restart Herdr). No atm release is
  involved.
- Under the socket transport (AY.8, AY.9) atm never sees `protocol_mismatch`.
  Compatibility becomes atm's own responsibility and is keyed on
  `HERDR_MINIMUM_VERSION` and the `ping` result (`version`,
  `capabilities`), never on PROTOCOL_VERSION. This is why AY.8's blocking
  prerequisite is the conformance matrix, not the socket code.
- ADR-058's pin of `d79fd746` / 21 is replaced by `HERDR_MINIMUM_VERSION`
  and the per-release table in `herdr-versions.md` (AY.1).

## Ordering with phase AX

Status 2026-09-05 (fenix@rand-m5): AX.1 through AX.5 are merged into
integrate/phase-ax; AX.6 (PR #1204) is in its second fix pass; AX.7
(PR #1206, stacked on AX.6) is the macOS live-proof sprint and keeps its
own accepted scope (Windows runs are out of AX.7 scope and stay so; AY.9
does not inherit that proof; AY.10 owns Windows evidence, ruling 5). One PR integrate/phase-ax -> develop follows
AX.7 and the phase-ending review.

Decision (Rand, 2026-09-05): `integrate/phase-ay` is cut from
`integrate/phase-ax` after PR #1204 has merged into it, or from develop if
phase-ax has already merged to develop at that moment. Either way AY.1 and AY.2
take the AX contract, including HR-CORE-010 notify, as its baseline.
When cut from phase-ax, `integrate/phase-ay` is retargeted to develop and
merged forward once the phase-ax PR lands, before the phase-ay PR opens.
AX does not wait on any AY sprint.

## Risks

1. Herdr keeps moving (92 commits in the 17 days after v0.8.2, no
   release cut yet) and every version at or above 0.8.0 must keep
   working: the per-version conformance fixtures (rule 4) and the
   sprint-start drift check against the reference checkout (rule 5) are
   the controls; a behaviour change that cannot be absorbed on atm's side
   is escalated to Rand as a minimum-version decision, never silently
   adopted.
2. Transport extraction silently changes macOS/Linux behaviour:
   pure-motion commit, unedited fixtures, boundary tests unchanged.
3. Orphaned herdr.exe after timeout: explicit live-verified test (AY.7).
4. Console flash per nudge: CREATE_NO_WINDOW plus visual confirmation
   (AY.7).
5. Merge collision with AX: AY branches from phase-ax after AX.6 and
   merges forward after the phase-ax PR lands (P-A).
6. Hot-path regression: official benchmark run before merge (AY.2, AY.8, AY.9).
7. AY.9/AY.10 (socket cutover and proof) quietly dropped again: the exit gate requires a dated
   Ship/Defer/Cancel decision line before the phase PR.
8. FastPC4 not ready: only AY.7 and AY.10 depend on it (P-C/P-D), and
   AY.10 waits for AY.9. AY.1 through AY.6 and AY.8 land without a physical
   Windows machine; AY.9 waits for the AY.7 Windows implementation gate,
   and the phase cannot close with Windows parity claimed until AY.10's
   Windows evidence exists (ruling 5).
9. Two launchers race at login (atm-installed Herdr entry vs a GUI Herdr
   the user opens): the entry has no KeepAlive, the loser exits 1 once,
   the GUI attaches as a client to whichever server won, and the daemon
   is indifferent to which one it was. Doctor shows the probe result.
10. Herdr update leaves an old server running (Herdr's default since
    cc88b3b8): every nudge fails until someone restarts Herdr. Controls:
    doctor state text, one lead notification per breaker-open,
    `daemon-switch --restart-herdr` preferring live handoff so agent panes
    survive.
11. Herdr not running for a long stretch: nudges on the Herdr harness
    fail fast under the open breaker while mail still queues; the
    reminder cycle drains the backlog when Herdr returns. Operators see
    it in doctor, not in a dead daemon.

## Windows machine and team (Rand, 2026-09-05)

- Windows testing runs on **FastPC4**. Rand will set up a Windows
  `atm-dev` team on FastPC4; AY.7 dev work (including filling the
  Windows-observed audit rows) and AY.10's live-evidence matrix
  (the phase's only evidence owner, ruling 5) both execute there, inside
  a Herdr session, with the daemon and Herdr installed per-user by
  `daemon-switch`.
- Cross-host messaging from FastPC4 has been unreliable because of VPN
  issues. Design: park one agent on the FastPC4 team whose only job is
  to report back regularly (on a fixed cadence and on every sprint
  event) to either the `atm-dev` team on rand-m4 or on rand-m5,
  whichever is reachable, so work continues even when a direct
  cross-host session is down. Reports carry SHA, test results, and
  evidence paths; the FastPC4 team is the source of truth for Windows
  evidence. Outbound dispatch to FastPC4 is fenix's j2 assignment sent
  to the named dev agent (P-D); when the link is down, the reporter's
  next report carries the last assignment id it saw and fenix re-sends.
- The Windows CI job stays the merge gate; FastPC4 evidence is the
  release-readiness gate for Windows Herdr parity.
- cwin remains routed through Rand; nothing is dispatched to FastPC4
  directly from this team until the FastPC4 team exists.

## Review ledger

- qa-pr1214-plan r1 (73f79aa4f, FAIL) and r2 (04685bfbf, PASS on the
  blocking contradiction; Important "W1 bundles eight closure types"
  open): addressed here by the seven-sprint split, the exit-gate
  decision line, and P-C/P-D.
- critical-plan-reviewer AY-PLAN-HOSTILE-R1 on 9e7577143 (FAIL, 5
  blocking / 7 important / 2 minor): AYP-001 (HR-CORE-006 retained),
  AYP-002 (daemon launch path removed; installer entry instead),
  AYP-003 (HerdrError unchanged, no NotRunning), AYP-004 (notify sound
  fixed), AYP-005/006 (P-C/P-D with owner, risk 8, outbound dispatch),
  AYP-007 (AX.7 widening dropped), AYP-008 (open-rows table reduced to
  genuinely open rows), AYP-009 (single v0.8.2 pin, SHAs reconciled),
  AYP-010 (HR-CORE-003 claim withdrawn, table fixed), AYP-011 (grep-able
  decision line), AYP-012 (fallback retention unit), AYP-M1 (risks
  renumbered), AYP-M2 (citation line fixed).
- Herdr drift review (two background agents, v0.8.2..3a822e81,
  2026-09-05): findings in "Herdr minimum version, drift and schema
  management"; PROTOCOL_VERSION section corrected; prompt-wait semantics
  and Windows PATH change added to AY.2/AY.5 deliverables.
- Rand, 2026-09-05 (via fenix@rand-m4; ADR-061 on PR #1218 is the
  authority, discussion in #1217): Herdr IPC
  follows the HTTP schema-management pattern; support every Herdr version
  at or above `HERDR_MINIMUM_VERSION` (0.8.0 today); raising it needs
  Rand's recorded sign-off; new capability adopted additively; a
  conformance fixture per supported version; `schema-reviewer` (PR #1218)
  checks this at plan and phase-end review. Landed in AY.1 to AY.3,
  extended by AY.6.
- Rand, 2026-09-05 (API change management; then "low-code solution"):
  r4 proposed per-epoch adapters with a registry; r5 replaces that with
  the low-code mechanism (one version-agnostic implementation, doctor-only
  minimum check, data-driven conformance recordings, seam added only when
  a second implementation exists). No daemon restart on Herdr upgrade.
- Rand, 2026-09-05 (restart coordination, user experience, low-code):
  "Upgrade and restart coordination" added: no atm restart on Herdr
  upgrade; operator-invoked `daemon-switch --restart-herdr` preferring
  Herdr live handoff; a typed doctor state set (seven variants then,
  exhaustive from r8, per endpoint from r10); one lead notification per
  breaker-open; lifecycle tests.
- Rand, 2026-09-05 (parallel sprints, gh-stack, sprint-planning
  guidelines): r6 renumbers into seven sequential sprints with explicit
  must_follow/parallel_safe relations, four parallel tracks, the
  gh-stack stacking rule (link only; merge commits; never rebase/sync),
  recommended agents, and per-sprint deliverables/acceptance/validation
  lists.
- Rand, 2026-09-05 (relayed by team-lead, then corrected directly):
  FastPC4 dispatch is ssh over VPN run by fenix (Rand brings up the VPN,
  passwordless ssh, fenix manages the remote Herdr pane) (P-D updated); the LaunchAgent/user-unit start-at-login
  entry (RunAtLoad, no KeepAlive) is the decided mechanism, "leave it to
  the user" rejected. Decisions go to Rand directly from now on.
- plan-scope-reviewer r1 on f1e42c200 (FAIL, 3 blocking / 7 important /
  2 minor wording): AYS-001 (AI.11 guard bans `named_pipe` workspace-wide;
  AY.6 deliverable 0 guard exemption, AY.1 ADR text corrected), AYS-002
  (AY.3/AY.6 both touched `build_replacement_handler`; transport selection
  moved to AY.7, AY.6 has no composition change), AYS-003 (AY.6
  must_follow AY.1 added), AYS-004 (split AY.6 by platform: declined with
  rationale in the AY.6 header; Windows lane covers the named-pipe path,
  live Windows is AY.7), AYS-005 (two-parent join rule added to the
  stacking rule), AYS-006 (AY.7 deliverable 2 re-validates lifecycle
  tests on the socket default), AYS-007 (predicate signature in AY.3),
  AYS-008 (endpoint signature and TOML fragment), AYS-009 (P-E with
  owner and timing), AYS-010 (acceptance/validation itemized in all
  sprints), M1 (cross-reference), M2 (reverse-order account case).
- critical-plan-reviewer AYP-R2 (solar, CPR-AY-R2-1788638959) on
  f1e42c200 (FAIL, 4 blocking / 9 important): 001 (per-call session
  kept on `HerdrProcessAdapter`; no session in client config), 002
  (boundary revision additive; CLI keys stay), 003 (no second public
  trait: crate-private `HerdrIo` enum + `HerdrEnvelope`,
  `RequestDeadline`, no async_trait), 004 (`status server --json` /
  ping as the sixth recorded operation via `HerdrDoctorProbe`), 005
  (REQ-P-DAEMON-SWITCH-002 + ADR-053 addendum, `herdr-entry`
  subcommand, ownership marker, restart-only `--restart-herdr`), 006
  (daemon-switch consumes `atm doctor --json herdr.configured`, fail
  closed), 007 (same as AYS-002, fixed in r7), 008 (`HerdrDoctorState`
  enum with `Other`, tests per variant), 009 (breaker-open escalation
  uses the AX.6 helper: mail durable, notify best-effort), 010 (0.8.0
  Windows recorded as packaging fact; recordings-only proof assumed,
  open for Rand), 011 (approval blocker on P-C/P-D placeholders; Rand to
  fill), 012 (Cancel requires a cleanup PR, mechanical check), 013
  (`just validate` and `just benchmark-official` named exactly). Both
  reviewers noted the missing plan-hardening fenced-JSON handoff: these
  were pre-hardening reviews by Rand's request; the handoff comes with
  `/plan-hardening` after approval.
- Rand, 2026-09-05: "design to 0.8.0 ... if there are no public api
  changes between 0.8.0 and 0.8.2, we can design/test to 0.8.2 and call
  it 0.8.* compatible." Verified on the tags: no changes on our six
  operations except the additive `agent_blocked` prompt rejection.
  Design target v0.8.2, claim 0.8.*, minimum 0.8.0; one recording set
  plus one delta mode; artifact table added. The Windows 0.8.0 question
  is moot (no artifact).
- Rand, 2026-09-05: plan not approved yet; startup and environment
  concerns; atm must not own Herdr; no hard gate; launchd installs Herdr
  entry only when configured, mirrored in daemon-switch; late Herdr
  start must be handled; daemon must work and diagnose when Herdr
  cannot start. All recorded in "Design rulings" and the normative
  section.
- plan-scope-reviewer r2 on the 8f4d6b760 working tree (FAIL, 3
  blocking / 4 important / 2 minor wording): AYS-R2-001 (ADR-061
  collision with AX.3's ADR on integrate/phase-ax: resolved on the AX
  side by renumbering before the phase-ax PR; AY.1 gains the one-file
  check), AYS-R2-002 (disputed on the fact: PR #1218 merged
  2026-09-05T20:11Z as a7aebefb8 with the constant at lib.rs:22 and the
  ADR file; the reviewer read a stale checkout; P-A now carries the
  merge-base check), AYS-R2-003 (`HerdrTransportKind` defined, core
  owned), AYS-R2-004 (AY.7 adapted (d) distinct from (g)), AYS-R2-005
  (AY.4 size L with internal order), AYS-R2-006 (line citations
  re-pinned to develop a7aebefb8), AYS-R2-007 (`[contracts]` type list
  named), M1 (ledger wording), M2 (`HerdrDoctorProbe` signature). All
  r1 findings confirmed CLOSED by the reviewer.
- critical-plan-reviewer AYP-R3 (solar, CPR-AY-R3) on 20eb116f1 (FAIL, 5
  blocking / 7 important): 001 (doctor boundary redrawn: atm-core owns
  `HerdrEndpointObservation`/`HerdrDoctorState` and the
  `HerdrEndpointDoctor` port; atm-herdr fills them through the public
  `HerdrDoctorProbe`; bootstrap adapts; public-item pin test), 002
  (`herdr.socket_path` added with Herdr's own precedence, exclusive env
  mapping, validation, provenance, tests in AY.2/3/6/7), 003 (per
  endpoint diagnostics and one start-at-login entry per distinct
  configured session with deterministic identifiers; `herdr --session
  <name> server` verified), 004 (AY.6 endpoint sample corrected to
  Herdr's real resolution: `<config_dir>/herdr.sock`, XDG/APPDATA,
  `\\.\pipe\` plus the path string; pinned tests), 005 (entry is a
  per-platform native object with its own journal transaction and
  repair; operator-invoked only, no implicit hook because daemon-switch
  has no service-install step), 006 (`BreakerOpen` removed; breaker
  stays the separate AX.6 report; Bypass probe test), 007
  (`deliver_escalation` corrected to the real `pub(crate) escalate`
  signature), 008 (six operations everywhere; v0.8.2 Windows recorded as
  packaging fact only), 009 (executable commands: `just lint spell`,
  `just lint adr-index`, `just lint boundaries`, `just benchmark-official
  --branch integrate/phase-ay`), 010 (one absolute deadline across
  connect/ping/command, 1 MiB line cap, fake-server cases), 011 (AY.6
  file set listed and compared; construction-site allowlist test), 012
  (error inventory with codes, exit statuses, JSON shape, snapshot
  tests). Both reviewers again recorded the missing plan-hardening
  handoff as an input-contract note, not a finding.
- r11 (2026-09-05): critical-plan-reviewer R4 (solar, on r10: 1 blocking
  / 4 important) and plan-scope r3 (1 blocking / 2 important / M1).
  AYP-R3-010 (one request per connection, no hot-path `ping`; doctor's
  `server_info` opens its own connection; fake refuses a second line),
  AYP-R3-011 (AY.6 must_follow AY.3: table, tracks, rationale,
  dependencies and file set corrected), AYP-R4-001
  (`--restart-herdr <endpoint>`: one endpoint per invocation, scoped argv
  and entry identifier, refusal codes, never restarts the daemon, daemon
  restart refused while any endpoint mismatches), AYP-R4-002 (aggregate
  `herdr.state`/`herdr.remedy` removed; `endpoints[]` in a fixed order),
  AYP-R4-003 (`HerdrEndpointDoctor` replaces `HerdrPresenceDoctor`:
  ADR-001 `Sealed` supertrait, one recorded impl site, presence findings
  derived from observations), AYS-R3-001 (`endpoint: Option<String>` on
  the atm-core DTO, filled by atm-herdr via `Display`), AYS-R3-002 (AY.6
  deliverable 5 amends the AY.2 pin test; wording in both sprints),
  AYS-R3-003 (`herdr_is_configured` takes the same `MembersList` doctor
  builds from one roster load), AYS-R3-M1 (AY.2 and AY.6 acceptance
  lists renumbered in reading order).
- r12 (2026-09-05): plan-scope r4 AYS-R4-001 (AY.6 dependency trigger
  stated four ways in r11): one rule everywhere, AY.6 is a multi-parent
  join created from `integrate/phase-ay` after AY.1, AY.2 and AY.3 have
  merged, never stacked (AY.4 is AY.3's stacked child); Socket track,
  rationale, stacking rule, AY.6 header and the r11 ledger line agree.
- r13 (2026-09-05): critical-plan-reviewer R5 (solar, on r11: 2 blocking
  / 5 important; AYP-R5-003 was the same defect as AYS-R4-001, fixed in
  r12). AYP-R4-003 (machine-readable
  `boundaries/atm-core/herdr-endpoint-doctor.toml` under P-E, docs
  section, impl-count test, AY.3 acceptance 6), AYP-R5-001 (composite
  observation with `agents`, `presence_findings(observations, roster)`,
  zero-regression tests; `HerdrDoctorProbe` has one method), AYP-R5-002
  (`herdr_is_configured(roster)` on `LocalMessageReceivedBackend::Herdr`;
  no `enabled` flag, no `RosterHarness::Herdr`; `[herdr]` tuning section
  of `$HOME/.atm.toml` read beside `atm_temp_config.rs`; "Herdr harness
  members" wording corrected), AYP-R5-004 (`live_handoff` on the
  observation and in doctor JSON; restart consumes it), AYP-R5-005
  (refusal by endpoint provenance, mixed fixture; only the socket_path
  endpoint lacks an entry), AYP-R5-006 (one `herdr_entry_identifier`
  helper, asserted on three platforms).
- r14 (2026-09-05): plan-scope r5 (AYS-R4-001 closed; AYS-R5-001 blocking,
  AYS-R5-002 important). AY.3 resized to L with AY.4's internal order
  (stage 1: P-E(a) TOML, port, DTOs, predicate, presence correlation,
  impl-count test; stage 2: reader, wiring, escalation, lifecycle tests);
  `herdr_config.rs` given its contract (`daemon_herdr_client_config(env)
  -> Result<HerdrClientConfig, AtmError>`, missing table = default, fail
  closed otherwise) and relative-path validation moved to the pure
  `HerdrClientConfig::validate` so AY.2 acceptance 6 needs no file I/O.
- r15 (2026-09-05): critical-plan-reviewer R6 (solar, on r13: 2 blocking;
  AYP-R4-003, R5-002/003/005/006 closed). AYP-R6-001 (`live_handoff:
  Option<bool>` kept whenever the server answered, independent of state;
  restart consumes `true`, positive `ClientServerMismatch` fixture),
  AYP-R6-002 (per-member outcomes from the existing bounded `get`, typed
  `HerdrPresenceOutcome` {Visible, NotVisible, Failed, Skipped}, mapped in
  atm-herdr, never cleared by state; `presence_findings` byte-identical
  to the AX adapter; `agent list` no longer used for presence).
- r16 (2026-09-05): critical-plan-reviewer R7 (solar, on r15: 1 blocking,
  2 important, 1 minor; AYP-R6-001 closed) and plan-scope r6 (AYS-R5-001/002
  closed; 1 important, 1 minor). AYP-R7-001 (`HerdrPresenceOutcome` now
  {Visible, Finding(DoctorFinding), Infrastructure{reason}} built by
  atm-herdr's moved `presence_finding`; `HerdrRosterMember` ordinal;
  `presence_findings` flattens, restores roster order, one global
  HerdrUnavailable Info; two-endpoint interleaved fixture), AYP-R7-002 and
  AYS-R6-001 (`HerdrEndpointDoctorAdapter` and `herdr_configured.rs`
  moved into deliverable 2 = stage 1, over a default-config probe;
  deliverable 1 = stage 2 wiring only; header, deliverables and AC 7 use
  one wording), AYP-R7-003 (`validate -> Result<(), AtmError>` with
  `ConfigParseFailed`; reader uses `AtmError::new(..).with_cause(..)`, no
  `config_invalid`; fixtures for unresolvable home, unreadable file,
  relative `binary_path`), AYP-R7-004 and AYS-R6-M1 (AY.3 AC 7/8 in
  reading order).
- r17 (2026-09-05): critical-plan-reviewer R8 (solar, on r16: 0 blocking,
  3 important; AYP-R7-001..004 closed) and plan-scope r7 (PASS, AYS-R6-001
  and M1 closed). AYP-R8-001 (AY.1 `[contracts]`: `request_types` adds
  `HerdrRosterMember`, `error_types` adds `AtmError`; atm-herdr
  boundaries.md error_types bullet names the two construction sites; AY.2
  pin test pins the `observe`/`validate` signatures), AYP-R8-002
  (`HerdrMemberPresence` is `{#[serde(skip)] ordinal, name, outcome}`;
  `HerdrRosterMember` is probe input only; snapshot asserts exactly
  name/outcome), AYP-R8-003 (`herdr_config.rs` failure mapping is a
  per-class table with one constructor/cause policy each; validate Err
  wrapped once with the file, validate error as cause; AY.3 AC 7 asserts
  detail and cause per row). AYF-R17-001 (fenix): the doctor DTO file
  `herdr_state.rs` moves to AY.2 deliverable 1 because
  `HerdrDoctorProbe::observe` returns and consumes it and AY.3 must_follow
  AY.2; port, closed impl, `presence_findings`, predicate and boundary
  record stay AY.3 stage 1.
- r18 (2026-09-05): critical-plan-reviewer R9 (solar, on r17: 0 blocking,
  0 important, 1 minor; AYP-R8-001..003 closed, AYF-R17-001 assessed
  sound) and plan-scope r8 (PASS, 1 minor wording). AYP-R9-001 and
  AYS-R8-M1: AY.2 out-of-scope line now excepts the DTO file and names
  the doctor pieces it excludes; AY.3 size-L rationale says "port over the
  AY.2 DTOs" and states the estimate is unchanged. AY.2 deliverable 1 is
  the authoritative DTO ownership statement.
- r19 (2026-09-05): Rand ruling 5 ("herdr is working w/ cli today"):
  no live Windows evidence campaign for the CLI transport. AY.5 narrowed
  to size S (process correctness in `transport_cli.rs`, installer Windows
  branch, audit columns; deliverable 4 now forbids an evidence directory;
  AC 3 is the kill/reap and no-console-flash verification). AY.7
  deliverable 3 spells the full evidence set inline and takes the
  release-readiness gate for Windows Herdr parity. Sprint map, Windows
  track, AX status note and risk 8 aligned.
- r20 (2026-09-05): critical-plan-reviewer R11 (solar, on r19: 1
  important) and plan-scope r10 (PASS, 1 wording). AYP-R11-001 and
  AYS-R10-M1: the four stale AY.5-evidence references fixed (audit row
  "AY.5 audit run"; parallel-safety file list has no evidence directory;
  AY.5 branch renamed `feature/ay5-windows-herdr-process-installer`;
  "Windows machine and team" attributes the live-evidence deliverable to
  AY.7). AY.7 deliverable 3 is the sole normative evidence-owner
  statement. Sweep phrases checked clean outside this ledger: "AY.5
  evidence", "ay5-windows-herdr-evidence", "audit doc, evidence".
- r21 (2026-09-05): critical-plan-reviewer R12 (solar, on r20: PASS
  0/0/0, AYP-R11-001 closed) and plan-scope r11 (PASS, AYS-R10-M1
  closed, 1 wording). AYS-R11-M1: "Windows machine and team" sentence
  reworded for subject/verb agreement; no scope change. Both reviewers
  PASS on r20 84f2a8469; r21 is this one sentence plus the ledger entry.
- r22 (2026-09-05): sprint-plan hardening supersedes the earlier
  seven-sprint packaging and its pre-hardening PASS statements. Ten
  authoritative sprint files now separate transport foundation, endpoint
  doctor/config, breaker lifecycle, entry transaction, restart
  coordination, Windows verification, socket implementation, cutover, and
  live proof. The umbrella's duplicate sprint checklists were deleted;
  mandatory frontmatter, execution waves, exact PR parents, `/gh-stack`
  external-link workflow, canonical doctor JSON, and the AY.10 evidence
  disposition gate are the current review surface. A new plan-scope and
  critical review is required before approval; r21 is historical evidence,
  not approval of r22.
