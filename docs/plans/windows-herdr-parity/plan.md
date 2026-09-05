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
corrects the ADR-058 Herdr pin, and (AY.6 only) revises the boundary TOML
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
rules 2 to 5 for the CLI transport on top of the #1218 constant; AY.6
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
   (AY.6) and reports below-minimum as a finding with the remedy. The
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
| `agent prompt --wait` | Activity gate now requires an observed `working` or `blocked` status (8633a398, 7916be16; `src/api/wait.rs:177-320,516`); a prompt into an already-working agent skips the gate; done/idle churn without `working` ends in `timeout`; blocked-after-submit returns success with `agent_status: blocked`; `agent_prompt_stalled` message text changed (`wait.rs:662-664`); on Windows the dispatch is bounded by the caller timeout and returns `timeout`, not `server_unavailable` | Material for HR-CORE-002/003: the same nudge can end in `timeout` on a newer Herdr where v0.8.2 returned `agent_prompt_stalled`. atm keys on codes, never message text; AY.1 re-verifies (docs) and AY.2 tests the `HerdrError` mapping and the queue-pump/reminder behaviour under both outcomes (rule 3) |
| Update model | `herdr update` keeps a compatible old server running (cc88b3b8, `CompatibleServerKept`); `herdr status` `restart_needed` keys on endpoint generation, not PROTOCOL_VERSION | An updated CLI binary against a still-running old server fails every CLI command with `protocol_mismatch`, and that state is now normal and sticky; doctor must make it legible; direct NDJSON (AY.3) is immune |
| PROTOCOL_VERSION | 20 -> 22 (d79fd746, 99c23cd1); versions the bincode client socket, not the NDJSON API | See "PROTOCOL_VERSION and compatibility" |
| Windows PATH | Installer prepends the versioned release dir to the user PATH (8162e265, `distribution/install.ps1:881-904`); `%LOCALAPPDATA%\Programs\Herdr\bin` stays as a junction alias | Never persist a resolved `herdr.exe` path; resolve per spawn; configured `binary_path` may point at the alias |
| CRT | x86_64 build statically links the CRT (73825652) | No VC++ redistributable needed; arm64 not covered |
| `--no-session` | Removed (207be3c7) | Never passed by atm; nothing to do |
| `herdr server` | Entry unchanged (`src/main.rs:542`); `run_server` moved to `src/server/headless/bootstrap.rs` (207be3c7) with identical duplicate detection and restore | No change |
| Endpoint resolution: `ipc.rs`, `session.rs`, `api/client.rs`, `socket_paths.rs` | Byte-identical across the range | No change |
| NDJSON envelope, `agent.*` shapes, limits, `notification show`, CLI argv and exit codes | No change; additive methods and fields only (`ping.capabilities.endpoint_protocol_generation`, `workspace.close.close_group`, `worktree.*.trust_repository`, new `pane.*`, `command.invoke`, `integration.list`) | Parsers must tolerate unknown fields; AY.1 asserts it |
| `events.subscribe` | Starts at the live sequence, no replay (20a500a7) | Irrelevant unless AY.6 subscribes; documented |
| Autostart | Still none at HEAD (grep for login item / LaunchAgent / RunAtLoad / autostart / schtasks / Register-ScheduledTask / systemd / SMAppService: only SSH keepalive hits) | Confirms the installer-owned start-at-login entry design |
| API pipe ACL | Unchanged: `restrict_socket_permissions` is a no-op on Windows (`src/ipc.rs:342-345`); the SDDL DACL at `ipc.rs:141-167` serves only the remote-attach bridge | AY.3 boundary revision records the API pipe as default-DACL |

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
  the Windows kill/reap correctness item (AY.5) is mandatory, not hygiene.
- ACL: Unix API socket is chmod 0600 (`src/server/socket_paths.rs:12`);
  on Windows `restrict_socket_permissions` is a no-op for the API pipe
  (`src/ipc.rs:342-345`); the same-user SDDL DACL exists only on the
  private remote-attach listener (`src/ipc.rs:143-168`). AY.6 must treat
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
| CRLF vs LF on stdout/stderr JSON observed on a real Windows run | AY.5 evidence |
| Detached-child stdio and console behaviour of `herdr.exe` spawned by the daemon | AY.5 |
| Pipe ACL / impersonation model in practice (same-user vs any local user) | AY.6 boundary revision |

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
  `herdr.binary_path` (absolute path or directory containing `herdr` /
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
  outrank the session); on the socket transport (AY.6) the same rule
  selects the explicit path or the session-derived path. Config load
  rejects a relative `socket_path`. Doctor reports each endpoint with
  its provenance (`session`, `socket_path`, `herdr-default`). When
  nothing is configured the child inherits the daemon's ambient
  environment unmodified. HR-CORE-006 is **retained unchanged**; the
  previous revision's deletion of it is withdrawn. atm never reads
  `HERDR_SESSION` or `HERDR_SOCKET_PATH` from its own environment to
  synthesize a choice (requirements.md:153-160). Tests: AY.2 (env
  mapping and exclusivity, relative-path rejection), AY.3 (doctor
  provenance per endpoint), AY.6 (socket target selection), AY.7 (round
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
  provenance `"configured"`/`"PATH"`, server `version`/`protocol` from
  `herdr status server --json` (CLI) or `ping` (socket), `state`,
  `remedy`, `capabilities.live_handoff` (true/false/null), `members[]`
  (name, outcome); ordered
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
  // crates/atm-core/src/doctor/herdr_state.rs (AY.3). atm-core owns the DTOs and
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
  // transport (AY.7).
  pub struct HerdrVersion(pub String);                 // as Herdr reports it; semver compare lives in atm-herdr
  pub enum HerdrTransportKind { Cli, Socket }
  pub enum HerdrEndpointProvenance { Session, SocketPath, HerdrDefault }
  pub struct HerdrEndpointObservation {
      pub session: Option<HerdrSession>,               // None = default endpoint
      pub provenance: HerdrEndpointProvenance,
      pub transport: HerdrTransportKind,
      pub endpoint: Option<String>,                    // AYS-R3-001: Display form of the resolved socket/pipe name,
                                                       // filled by atm-herdr from its own HerdrEndpoint (AY.6/AY.7);
                                                       // None on the CLI transport. HerdrEndpoint never enters atm-core.
      pub binary: Option<(PathBuf, &'static str)>,     // resolved path + "configured" | "PATH" (CLI only)
      pub state: HerdrDoctorState,
      pub live_handoff: Option<bool>,                  // AYP-R5-004/AYP-R6-001: a probe fact, independent of `state`:
                                                       // Some(capabilities contains "live_handoff") whenever server_info/ping
                                                       // returned (Ok, BelowMinimum, ClientServerMismatch); None only when
                                                       // capability retrieval failed. JSON: endpoints[].capabilities.live_handoff
      pub members: Vec<HerdrMemberPresence>,           // AYP-R5-001/AYP-R6-002: one entry per Herdr-backend roster member
                                                       // routed to this endpoint, never cleared by `state` (BelowMinimum
                                                       // still probes: calls continue on Herdr's terms)
  }
  pub struct HerdrMemberPresence { pub member: AgentName, pub outcome: HerdrPresenceOutcome }
  pub enum HerdrPresenceOutcome {
      Visible,                                         // `get` succeeded; any AgentStatus (Blocked included) is visible, as today
      NotVisible,                                      // HerdrError::AgentNotFound
      Failed { code: String, detail: String },         // any other non-infrastructure HerdrError, mapped by atm-herdr
      Skipped,                                         // infrastructure-class HerdrError (is_infrastructure())
  }
  // presence_findings (atm-core) reproduces replacement_handler.rs:125-206 exactly:
  // NotVisible -> the existing HerdrAgentNotVisible warning; Failed -> the existing
  // HerdrError-mapped warning text; any Skipped on an endpoint -> one Info HerdrUnavailable
  // "presence probe skipped" for that endpoint; Visible -> nothing. Zero-regression tests:
  // visible (Idle/Working/Blocked/Done), not-visible, failed (e.g. AgentBlocked), one and
  // several infrastructure-failed members, each asserting byte-identical findings to the
  // AX adapter's output on the same fake-herdr recording.
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
  // public `HerdrDoctorProbe` (AY.2). `MembersList` (team_admin.rs:93) is the doctor's
  // view of one `load_team_roster` RosterSnapshot, built by
  // `ordered_roster_member_summaries` (doctor/mod.rs:679-698); the same instance feeds
  // `herdr_is_configured` below (AYS-R3-003).
  ```

  The `HerdrError` to `HerdrDoctorState` mapping lives in atm-herdr
  (`crates/atm-herdr/src/doctor_probe.rs`), the crate that owns
  `HerdrError` and `HERDR_MINIMUM_VERSION`; it covers every `HerdrError`
  variant (`Other` is the exhaustive-match fallback; a test asserts no
  listed variant maps to `Other`).
- Architecture and lifecycle tests (AY.3): (a) daemon starts and reaches
  ready with no `herdr` binary on PATH and none configured, and a tmux
  nudge and a hermes graft nudge still succeed in that state; (b) same
  with a fake `herdr` that always returns `server_not_running`: the
  breaker opens, doctor reports it, the daemon stays up; (c) no code path
  in atm-daemon-bootstrap or atm-http-runtime spawns `herdr server` or
  any long-lived Herdr child (grep guard in boundary_enforcement.rs); (d)
  `daemon-switch` in the not-configured case writes no Herdr entry.

### Herdr starts after atm has been running

No special handling exists or is needed; the plan states the three
mechanisms that make late start self-healing, and AY.3 tests them:

1. Every call spawns a fresh `herdr` client process (AY.6: a fresh
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
temporary typed overlay). AY.4 therefore lands, as its first deliverable,
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
state (Windows). AY.4 gives it its own small transaction, one journal
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

User-experience contract (AY.3 and AY.4 acceptance, verified by req-qa):

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

Seven sprints, numbered sequentially. Dependency relations follow
`.claude/skills/plan-hardening/sprint-planning-guidelines.md`: every
related sprint is listed as `must_follow` or `parallel_safe` with a
rationale; `parallel_safe` means non-intersecting crates/modules, public
contracts, artifacts and ownership.

| Sprint | Title | Size | Track | must_follow | parallel_safe with | Machine | recommended_agent |
|---|---|---|---|---|---|---|---|
| AY.1 | Herdr audit, version table, requirement and ADR text | S | Docs | (P-A, P-B) | AY.2 | any | Cipher-311d/fast |
| AY.2 | Private transport enum, CLI transport pure motion, portable fake-herdr with per-version recordings | M | Core | (P-A, P-B) | AY.1 | macOS/Linux | arch-ctm/deep-reasoning |
| AY.3 | Startup/failure model: doctor port + boundary record, config wiring, breaker-open lead notification, lifecycle tests | L | Core | AY.2 | none | macOS/Linux | arch-ctm/deep-reasoning |
| AY.4 | Installer: REQ-P-DAEMON-SWITCH-002 / ADR-053 addendum, `daemon-switch herdr-entry`, `--restart-herdr` (restart only) | L | Core | AY.3 | AY.6 | macOS/Linux | arch-ctm/deep-reasoning |
| AY.5 | Windows process correctness, per-user installer, live Windows evidence | M | Windows | AY.4 (+ P-C, P-D) | AY.6 | FastPC4 | named Windows dev agent (P-D) |
| AY.6 | Direct socket/pipe transport: code, fake socket server, compatibility matrix (no cutover, no composition change) | L | Socket | AY.1, AY.2, AY.3 (+ P-E) | AY.4, AY.5 | macOS/Linux (Windows lane in CI) | arch-ctm/deep-reasoning |
| AY.7 | Socket cutover, CLI fallback retention, live evidence on macOS and Windows | M | Join | AY.5, AY.6 | none | macOS + FastPC4 | arch-ctm/deep-reasoning |

Tracks that run in parallel once `integrate/phase-ay` exists:

- **Docs track:** AY.1 alone. Touches `docs/atm-herdr/`, `docs/adr/`,
  `docs/architecture.md`, `docs/plans/phase-aq/` only.
- **Core track:** AY.2 -> AY.3 -> AY.4 (a `must_follow` chain). Touches
  `crates/atm-herdr`, `crates/atm-daemon-bootstrap`, `crates/atm-core`
  doctor, `crates/atm-architecture` tests, the `daemon-switch` skill.
- **Socket track:** AY.6 after AY.1, AY.2 and AY.3 have all merged into
  `integrate/phase-ay`; its branch is created from `integrate/phase-ay`
  at that point, never stacked (AYS-R4-001; AYP-R3-011: AY.3 lands the
  long-lived-child grep guard in `boundary_enforcement.rs` and AY.6 edits
  the same file), in parallel with AY.4 and AY.5. Touches the new
  `crates/atm-herdr/src/transport_socket.rs` and its test fixtures, the
  `Socket(SocketIo)` arm in `crates/atm-herdr/src/transport.rs`, one
  `mod transport_socket;` line in `crates/atm-herdr/src/lib.rs`, the
  tokio `net` feature in `crates/atm-herdr/Cargo.toml` if not already
  on, one exemption line in the AI.11 guard (`crates/atm-architecture`),
  the `herdr-versions.md` NDJSON columns, and the boundary revision
  (P-E). The exact file set is compared against AY.3/AY.4/AY.5 in the
  AY.6 section.
- **Windows track:** AY.5 after AY.4, on FastPC4. Touches
  `transport_cli.rs` `cfg(windows)` code, the Windows branch of the
  installer, and the evidence directory.
- **Join:** AY.7 after AY.5 and AY.6 have both merged into
  `integrate/phase-ay`. It owns every composition change of the phase
  that touches transport selection.

`parallel_safe` rationale per pair: AY.1/AY.2 share no files (docs vs
crate code; the version table in AY.1 points at the recordings AY.2
commits under `crates/atm-herdr/tests/fixtures/herdr-versions/`, it does
not duplicate them). AY.6 versus AY.4/AY.5: AY.6 adds one module and
one fixture directory, edits `transport.rs` (enum arm) and `lib.rs` (mod
line) in atm-herdr, one exemption line in the AI.11 guard, the AY.2
public-item pin test, the NDJSON columns of `herdr-versions.md`, and the
boundary TOML under P-E; AY.4/AY.5 do not touch those paths (AY.4:
daemon-switch skill and governance docs; AY.5: `transport_cli.rs`, the
installer's Windows branch, the audit doc, evidence). AY.6 is **not**
parallel-safe with AY.3 (AYP-R3-011): both edit
`crates/atm-architecture/tests/boundary_enforcement.rs` (AY.3's
long-lived-child grep guard; AY.6's AI.11 exemption and pin-test
amendment), so AY.6 must_follow AY.3 as a multi-parent join, not a
stack (stacking rule below). AY.6 does not touch
`build_replacement_handler` (crates/atm-daemon-bootstrap), doctor, the
installer or `transport_cli.rs`: transport selection in the composition
root is AY.7's first deliverable, so the only composition edits of the
phase are AY.3 (CLI construction site) and AY.7 (selection and default),
which are sequential.

Stacking rule (gh-stack, as used in phase AX): every `must_follow` chain
is a stack rooted on `integrate/phase-ay`. Branches are created with
`sc-git-worktree` from the parent branch (not from integrate) so the
child carries the parent's unmerged work. After the child PR opens,
`gh stack link --base integrate/phase-ay <parent-branch> <child-branch>`
records the dependency (append with `gh stack link <stack-number>
<branch>`); `gh stack view --json` is the status source. Merge-forward
trigger per the guidelines: parent development pushed, not QA; fenix
merges parent -> child with a merge commit before every dev or fix round
on the child. PR-completion trigger: the parent PR merges into
`integrate/phase-ay` first, then the child. Never `gh stack rebase`,
`gh stack sync` or `gh stack merge` in this repo (merge commits only, no
rebase, no force-push); `gh pr merge --merge` per PR in stack order.
Parallel-safe sprints are independent branches off `integrate/phase-ay`,
not stacked. A sprint with several parents (AY.6 after AY.1, AY.2 and AY.3; AY.7
after AY.5 and AY.6) is never stacked on any parent (AY.3's branch
already carries AY.4 as its stacked child, and gh-stack refuses a branch
shared by two stacks): it is dispatched only after every parent PR has
merged into `integrate/phase-ay`, and its branch is created from
`integrate/phase-ay` at that point (AYS-R4-001). No merge of an
unmerged sibling into a sprint branch, ever.

Sprint docs: `/plan-hardening` produces one sprint doc per row at
`docs/plans/phase-ay/sprint-AY.<n>-<slug>.md` with a single authoritative
list each for deliverables, acceptance criteria and required validation,
mirroring the sections below; QA reviews from those docs.

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
- P-E (AY.3 and AY.6): two boundary files, each reviewed by the
  `boundary-guard` agent, run by fenix before the owning sprint is
  dispatched; devs never author boundary TOML without this ruling. Owner:
  fenix. (a) AY.3: the new `boundaries/atm-core/herdr-endpoint-doctor.toml`
  described under "Failure behaviour" (AYP-R4-003), reviewed after AY.2
  merges and before AY.3 dispatch; the approved file is AY.3's first
  commit. (b) AY.6: the revision below, reviewed after AY.2 merges and
  before AY.6 dispatch; the approved diff is AY.6's first commit.
  Proposed diff to `boundaries/atm-herdr/herdr-process-adapter.toml`:

  ```toml
  [ownership]
  io_owns = [
    "tokio_process_spawn",          # CLI transport (retained until the AY.7 fallback window closes)
    "herdr_argv_construction",      # CLI transport (same)
    "herdr_local_socket_client",    # new: UDS / named-pipe NDJSON client, transport_socket.rs only
    "herdr_json_error_parsing",
    "herdr_spawn_breaker",
  ]
  ```

  `io_forbidden` is unchanged. The two CLI keys are dropped in the sprint
  that removes the CLI fallback (after AY.7, tracked in the project plan),
  not in AY.6.

Approval blocker (AYP-R2-011): P-C's readiness proof (date and ATM
message id of the reporter's first round-trip) and P-D's exact FastPC4
ATM identity are placeholders until Rand fills them. Plan approval
(P-B) requires both filled or an explicit dated line from Rand deferring
AY.5/AY.7 scheduling; AY.5 is never dispatched on a placeholder.

Common acceptance for every sprint: merge gate 0 blocking / 0 important /
0 minor in scope, quality-mgr PASS posted on the PR, CI green at merge
time (never a dispatch gate), no flaky-test tolerance, frozen files
untouched without a written ruling, no tokio in atm-core.

## Sprint AY.1: Herdr audit, version table, requirement and ADR text (size S)

Branch `feature/ay1-herdr-audit-docs`. Docs only; no Rust changes.
Dependencies: must_follow none (P-A, P-B); parallel_safe AY.2 (no shared
files). recommended_agent Cipher-311d/fast.

Deliverables:

1. `docs/atm-herdr/windows-process-audit.md`: one row per item (expected
   behaviour, Herdr file:line at v0.8.0, v0.8.2 and 3a822e81, observed,
   verdict: no action / production fix / upstream request), folding in
   the facts sections of this plan and the two drift tables; the v0.8.0
   -> v0.8.2 per-operation diff for the six operations and the error-code
   delta; Windows-observed columns left explicitly "AY.5" until filled.
2. `docs/atm-herdr/herdr-versions.md` (schema rule 4): one row per Herdr
   release from 0.8.0, with PROTOCOL_VERSION, the six operations' argv
   and JSON shapes, drift items, and the path of the AY.2 recording;
   v0.8.2 recorded as the first official Windows release artifact (a
   packaging fact, explicitly not a compatibility floor: the minimum is
   0.8.0 on every platform); the rule 5 drift-check procedure (paths,
   command, where results go).
3. Requirements: delete the Windows scope-out at
   `docs/atm-herdr/requirements.md:376-378`; add HR-PLAT-001 (identical
   command set, error table and breaker semantics on all platforms;
   transport differences never surface as feature differences) and
   HR-LIFE-001 (daemon never depends on Herdr for startup or readiness;
   Herdr failures are per-call; no daemon restart on Herdr upgrade);
   HR-TEST-006 amended for the portable fixture.
4. ADR-058: amend D3 (UDS on macOS/Linux, named pipe on Windows, cited
   from v0.8.2 and 3a822e81); replace the `d79fd746`/21 pin with
   `HERDR_MINIMUM_VERSION` (ADR-061); record the decision that the AI.11
   retired-transport ban targets atm's own IPC listener; the guard
   (`RetiredWindowsTransportDetector` in
   `crates/atm-architecture/tests/boundary_enforcement.rs`) today scans
   every file under `crates/` and bans the identifiers `named_pipe` and
   `NamedPipe`, so ADR-058 states that AY.6 narrows the guard by exactly
   one exempted module (`crates/atm-herdr/src/transport_socket.rs`), the
   ban staying in force everywhere else; delete the Windows scope-out at
   line 576.
5. `docs/architecture.md` Herdr section updated; AQ2.6 deliverable 5
   marked superseded (history kept); the atm-herdr boundary TOML
   `[contracts]` block updated (no io_owns change): `request_types` adds
   `HerdrClientConfig`; `response_types` adds `HerdrEndpointObservation`
   (the atm-core-owned doctor DTO returned by `HerdrDoctorProbe`);
   `error_types` unchanged. Crate-private types (`HerdrIo`, `HerdrOp`,
   `HerdrEnvelope`, `HerdrErrorEnvelope`, `HerdrServerInfo`, `CliIo`,
   `SocketIo`) are not contracts and are not listed. boundary-guard
   reviews the AY.1 diff against exactly this list.

Acceptance criteria:

1. Each deliverable's file exists with the named sections.
2. `grep -n "out of scope" docs/atm-herdr/requirements.md docs/adr/ADR-058*` returns no Windows scope-out.
3. Req-qa can enumerate HR-PLAT-001 and HR-LIFE-001.
4. Schema-reviewer finds ADR-061 cited and no newest-Herdr assumption.
5. `ls docs/adr/ADR-061-*.md | wc -l` prints 1 on the branch.

Validation:

1. Docs-only PR: no full CI cycle is required (repo rule).
2. `just lint spell`, `just lint adr-index` and `just lint boundaries`
   (for the TOML `[contracts]` edit), each exit 0 (`just lint <target>`
   takes one target; there is no `lint-docs` recipe).

Out of scope: any code; the Windows-observed audit columns (AY.5).

## Sprint AY.2: private transport enum, CLI transport pure motion, portable fake-herdr (size M)

Branch `feature/ay2-herdr-transport-seam`. Dependencies: must_follow none
(P-A, P-B); parallel_safe AY.1. recommended_agent arch-ctm/deep-reasoning.
Runs on macOS/Linux with all three CI lanes as the merge gate; no Windows
machine, no live Herdr.

Design rule (AYP-R2-001/003, AYP-R3-001, low-code): `HerdrProcessAdapter`
in `crates/atm-herdr/src/lib.rs` stays the **only public trait** of
atm-herdr, with its existing per-call `session: Option<&HerdrSession>`
and `RequestDeadline` parameters unchanged, so per-member routing to
different Herdr sessions keeps working exactly as in AX. The transport
choice is a crate-private enum beneath `HerdrProcessInvoker`; there is
no second public trait, no `async_trait`, no process-shaped response
type, and no session in the client config. atm-herdr's public surface
grows in AY.2 by exactly two concrete types, both consumed only by the
composition root (`atm-daemon-bootstrap`, an allowed edge):
`HerdrClientConfig` (input) and `HerdrDoctorProbe` (diagnostics, one
method returning the atm-core-owned composite observation). An architecture test pins
atm-herdr's public item list to today's list plus those two; AY.6 amends
that one test once, adding `herdr_api_endpoint`, `HerdrHostEnv` and
`HerdrEndpoint` (consumed by the composition root and rendered by the
doctor probe for endpoint reporting, AYS-R3-002); no other sprint
touches it.

Deliverables:

1. **Private transport enum.** New `crates/atm-herdr/src/transport.rs`
   (crate-private):

   ```rust
   /// Explicit, daemon-global client configuration. Identity (agent, session)
   /// stays per call on HerdrProcessAdapter; this holds only how to reach herdr.
   pub struct HerdrClientConfig {
       pub binary_path: Option<PathBuf>,  // file or directory; None = PATH lookup by name, resolved on every spawn
       pub socket_path: Option<PathBuf>,  // Herdr's HERDR_SOCKET_PATH override; absolute; a per-call session outranks it
   }
   impl HerdrClientConfig {
       /// Default::default() = PATH lookup, no socket_path (the AX behaviour).
       /// Pure validation, no I/O (AYS-R5-002): Err on a relative `socket_path` or a relative
       /// `binary_path`; the message names the key. Tested in AY.2 without any file; AY.3's
       /// `herdr_config.rs` reader calls it after parsing and maps Err into AtmError.
       pub fn validate(&self) -> Result<(), String>;
   }
   /// One Herdr operation as the adapter already expresses it (argv builders at
   /// lib.rs:631-657 keep producing the CLI form; the socket form is AY.6).
   pub(crate) enum HerdrOp<'a> {
       Prompt { agent: &'a AgentName, text: &'a str },
       Wait   { agent: &'a AgentName, until: &'a [HerdrAgentStatus], timeout: Duration },
       Get    { agent: &'a AgentName },
       List,
       Notify { title: &'a str, body: &'a str },
       ServerStatus,                       // doctor only: `status server --json` / ping
   }
   /// Herdr's NDJSON envelope, parsed once; both transports produce exactly this.
   pub(crate) struct HerdrEnvelope { pub result: Option<serde_json::Value>, pub error: Option<HerdrErrorEnvelope> }
   pub(crate) struct HerdrErrorEnvelope { pub code: String, pub message: String, pub retry_after_ms: Option<u64> }
   /// Crate-private transport. AY.2 has one variant; AY.6 adds Socket(SocketIo).
   pub(crate) enum HerdrIo { Cli(CliIo) }
   impl HerdrIo {
       pub(crate) async fn call(&self, op: HerdrOp<'_>, session: Option<&HerdrSession>, deadline: RequestDeadline)
           -> Result<HerdrEnvelope, HerdrError>;
       pub(crate) async fn server_info(&self, session: Option<&HerdrSession>, deadline: RequestDeadline)
           -> Result<HerdrServerInfo, HerdrError>; // doctor, per endpoint
   }
   pub(crate) struct HerdrServerInfo { pub version: String, pub protocol: u32, pub capabilities: Vec<String> }
   // HerdrProcessInvoker { breaker, io: HerdrIo }; HerdrProcessInvoker::new(breaker, HerdrClientConfig)
   // constructs HerdrIo::Cli. HerdrError: the existing closed enum, unchanged.
   // HERDR_MINIMUM_VERSION already exists (PR #1218, 29e483e50); used by doctor (AY.3), never as a runtime branch.

   /// crates/atm-herdr/src/doctor_probe.rs. Public concrete type, no trait (M2, AYS-R2-003).
   pub struct HerdrDoctorProbe { io: HerdrIo, config: HerdrClientConfig }
   impl HerdrDoctorProbe {
       pub fn new(config: HerdrClientConfig) -> Self;                       // CLI transport; AY.7 adds the socket form
       /// Never fails: every HerdrError becomes a HerdrDoctorState; compares against HERDR_MINIMUM_VERSION here.
       /// One server_info (version, protocol, capabilities -> live_handoff: Option<bool>, kept whenever the
       /// server answered) plus the existing bounded `get` per member (min(remaining, 2s) each, as
       /// today) under BreakerPolicy::Bypass; HerdrError -> HerdrPresenceOutcome mapped here, so the
       /// adapter only groups roster members by endpoint (AYP-R5-001/004, AYP-R6-001/002).
       pub async fn observe(&self, session: Option<&HerdrSession>, members: &[AgentName], deadline: RequestDeadline)
           -> atm_core::doctor::HerdrEndpointObservation;
   }
   ```

   `HerdrEnvelope -> AgentSnapshot / HerdrError` (parsers at lib.rs
   675-770) stay in lib.rs (pure motion) and transport-independent;
   `timeout` and `agent_prompt_stalled` map to the same atm outcome. The
   io::Error to `HerdrError` translation lives in `CliIo`. Notify argv
   stays exactly `notification show <title> --body <body> --sound
   request` (HR-CORE-010). `HerdrProcessAdapter` gains no method:
   doctor reaches `server_info` through `HerdrDoctorProbe` above. Per
   call, `CliIo` sets exactly one of `HERDR_SESSION` or
   `HERDR_SOCKET_PATH` in the child environment ("Endpoint precedence"
   in "Daemon startup"); `session_environment` is extended, not
   replaced, so the AX fixture stays valid.
2. **CLI transport.** `transport_cli.rs`: today's `run_command_with_binary`
   (lib.rs:555-625) and `session_environment` moved verbatim into
   `CliIo`. Pure motion: the entire ADR-058 fixture suite, including
   `session_environment_is_only_present_for_an_explicit_session`
   (develop a7aebefb8 lib.rs:1113) and every atm-herdr check in
   `crates/atm-architecture/tests/boundary_enforcement.rs`, passes on
   macOS and Linux with zero fixture edits (zero-regression oracle).
   Binary resolution honours `binary_path` (file or directory) before
   PATH, on every spawn, never cached; missing binary is
   `ServerUnavailable` naming what was searched. `server_info` runs
   `herdr status server --json` under the same deadline.
3. **Portable fake-herdr and recordings.** Test-only fake herdr Rust
   binary (`crates/atm-herdr/tests/support/fake_herdr/main.rs`, test-only
   `[[bin]]`, located via `CARGO_BIN_EXE_fake_herdr` so cargo supplies
   the `.exe` suffix) replaces /bin/sh, so all six process tests run on
   all three CI lanes with identical assertions. Modes: exit 0, exit 1,
   stderr JSON envelope (`server_not_running`, `agent_prompt_stalled`,
   `timeout`, `protocol_mismatch`), stdout JSON line, sleep past
   deadline, echo argv and HERDR_SESSION, `status server --json` with a
   configurable version, and a replay mode that serves recorded
   responses from `crates/atm-herdr/tests/fixtures/herdr-versions/<version>/`
   (one directory per recorded release: `0.8.2/` today, captured from
   the reference checkout at the tag, committed byte-for-byte, each with
   a `manifest.json` naming the version, PROTOCOL_VERSION and the six
   operations; `herdr-versions.md` cites the manifests rather than
   restating them), plus a `v0.8.0-blocked-prompt` delta mode. The
   conformance suite runs once per recorded version and once per delta
   mode. One-off in this sprint: the ADR-058 suite is also run against
   the real v0.8.0 and v0.8.2 macOS artifacts (both published) and the
   result is noted in the PR, so the 0.8.\* claim is grounded in
   binaries, not only in recordings. Byte-exact LF output via write_all, never a .cmd shim. New
   tests: success stdout parse through a real child; argv/HERDR_SESSION
   round trip with two different sessions in one invoker; version switch
   between two calls with no restart; unknown-field tolerance. Parsers
   tolerate a trailing `\r`. HR-TEST-006 still holds. No-flaky rule:
   deterministic fake, injected deadlines, hard bounds.

Acceptance criteria:

1. Zero-regression oracle green on macOS and Linux (ADR-058 suite,
   boundary_enforcement atm-herdr checks, no fixture edits).
2. The windows-latest CI lane executes the process-behaviour suite.
3. Conformance replay passes for every directory under
   `fixtures/herdr-versions/` (each with a `manifest.json`) and for
   every delta mode; the one-off runs against the real v0.8.0 and v0.8.2
   macOS artifacts are recorded in the PR.
4. `HerdrProcessAdapter`'s signatures are unchanged (`git diff` on the
   trait block is empty); two sessions in one invoker route correctly
   (test).
5. No `async_trait` dependency added; `HerdrIo`, `HerdrOp`,
   `HerdrEnvelope`, `HerdrServerInfo` are `pub(crate)`; atm-herdr's
   public item list equals the AX list plus `HerdrClientConfig` and
   `HerdrDoctorProbe` (architecture test; AY.6 amends it to add its three
   items, AYS-R3-002).
6. `socket_path`: relative path rejected by `HerdrClientConfig::validate`
   (pure, no file I/O; the file reader is AY.3, AYS-R5-002); child env has
   exactly one of `HERDR_SESSION` / `HERDR_SOCKET_PATH` per call (tests).
7. Official benchmark shows no hot-path regression (see Validation 3).
8. Boundary TOML io_owns unchanged.

Validation:

1. `just validate` (full workspace gate: lint, boundaries, docs,
   architecture, tests); `cargo test -p atm-herdr` for the focused loop.
2. `python3 .just/check_line_counts.py`.
3. `just benchmark-official --branch integrate/phase-ay` from the dedicated
   benchmark account per `docs/plans/phase-ao2/readiness.md`; pass =
   exit 0 against the published floors; evidence path is the run's
   published report directory, cited in the PR.

Out of scope: composition changes, doctor, installer, Windows-specific
code, socket transport.

## Sprint AY.3: startup/failure model (size L)

Branch `feature/ay3-herdr-startup-failure-model`, created from
`feature/ay2-herdr-transport-seam` (stacked). Dependencies: must_follow
AY.2 (uses `HerdrClientConfig`, `HerdrIo`, `HerdrDoctorProbe`, fake-herdr);
parallel_safe none (AY.6 must_follow AY.3, AYP-R3-011: both edit
`boundary_enforcement.rs`). recommended_agent arch-ctm/deep-reasoning.
Size L (AYS-R5-001; the boundary record, the doctor port and DTOs, the
presence correlation and the config reader were added by the review
rounds) with the same internal order AY.4 uses (AYS-R2-005): **stage 1**,
the P-E(a) boundary TOML as the first commit, then deliverable 2 (port,
DTOs, `herdr_configured.rs`, `presence_findings`, impl-count test,
doctor JSON), is pushed and reviewed by fenix in the PR before **stage
2**, deliverables 1 and 3 (the `herdr_config.rs` reader, composition
wiring, escalation extension) and the lifecycle tests (a) to (k), starts;
quality-mgr reviews the finished sprint once. Rand's seven-sprint ruling
holds; nothing is split out.

Deliverables:

1. Composition: the one `HerdrProcessInvoker::new(breaker, config)`
   site in `build_replacement_handler` (today `::new(breaker)` only,
   replacement_handler.rs:308) fed by `HerdrClientConfig` from the
   optional `[herdr]` section of `$HOME/.atm.toml` (`binary_path`,
   `socket_path`), read by a new `herdr_config.rs` beside the existing
   `atm_temp_config.rs` reader in atm-daemon-bootstrap (AYP-R5-002;
   sessions stay per member, per call, from roster data as in AX):

   ```rust
   // crates/atm-daemon-bootstrap/src/herdr_config.rs (AY.3), same shape as
   // atm_temp_config.rs::daemon_atm_config(env: &dyn EnvSource) -> Option<AtmConfig>.
   /// Reads the optional `[herdr]` table of `$HOME/.atm.toml`. No file or no table ->
   /// Ok(HerdrClientConfig::default()) (PATH lookup, no socket_path: the AX behaviour).
   /// Unresolvable home, unreadable file, malformed TOML, an unknown `[herdr]` key, or a
   /// `HerdrClientConfig::validate` error -> Err(AtmError::config_invalid(..)) naming the
   /// file and key; the daemon then fails startup exactly as it does for any other
   /// invalid `.atm.toml` value (fail closed, never a silent default). Keys: `binary_path`,
   /// `socket_path`, both optional strings (AYS-R5-002).
   pub(crate) fn daemon_herdr_client_config(env: &dyn EnvSource) -> Result<HerdrClientConfig, AtmError>;
   ```

   plus the
   `HerdrEndpointDoctorAdapter` (replacing `HerdrPresenceDoctorAdapter`
   and the `HerdrPresenceDoctor` port; its `impl` carries the ADR-001
   `Sealed` marker and is the one production impl site permitted by
   `boundaries/atm-core/herdr-endpoint-doctor.toml` (P-E, first commit
   of AY.3) and its `docs/atm-core/boundaries.md` section, enforced by
   the impl-count architecture test, AYP-R4-003) over `HerdrDoctorProbe`; the Herdr-configured predicate
   (roster has at least one Herdr-backend member) defined once
   in atm-core and exposed through `atm doctor --json` for AY.4. No
   probe, no wait, no launch, no version chosen at startup.

   ```rust
   // crates/atm-core/src/herdr_configured.rs (new, one function, no state)
   /// True when the daemon should construct a Herdr transport, install the
   /// Herdr start-at-login entry (AY.4) and report the doctor `herdr` section.
   /// Pure: takes already-loaded roster data, never touches I/O or config.
   pub fn herdr_is_configured(roster: &MembersList) -> bool {
       roster.members.iter().any(|m| matches!(
           m.local_message_received_backend(),
           Some(LocalMessageReceivedBackend::Herdr { .. })
       ))
   }
   // MembersList / MemberSummary (team_admin.rs:52-93) and LocalMessageReceivedBackend
   // (delivery_channel.rs) are existing atm-core types; this is the same filter the AX
   // presence probe uses (replacement_handler.rs:139-144 at a7aebefb8). There is no
   // `enabled` flag and no HerdrBackendConfig (AYP-R5-002). MembersList
   // is built from one `load_team_roster` RosterSnapshot by
   // `ordered_roster_member_summaries` (doctor/mod.rs:679-698); a doctor run builds it
   // once and passes the same instance to this predicate and to
   // `HerdrEndpointDoctor::observe`, so `configured` and `endpoints[]` never disagree
   // (AYS-R3-003). The dev names these types exactly, adds no new type.
   ```
2. Doctor `herdr` section: the `HerdrEndpointDoctor` port and DTOs in
   atm-core (code in "Failure behaviour"), one `HerdrEndpointObservation`
   per distinct configured endpoint, each variant with its remedy;
   server version and protocol from `HerdrDoctorProbe::observe` (`herdr
   status server --json`) compared with `HERDR_MINIMUM_VERSION` inside
   atm-herdr; presence data is the `members` list of the same observation
   (the existing bounded `get` per Herdr-backend member under
   `BreakerPolicy::Bypass`, outcomes typed by atm-herdr, never cleared by
   server state), rendered by `presence_findings` in atm-core with
   byte-identical findings to the AX adapter (AYP-R5-001, AYP-R6-002);
   `live_handoff: Option<bool>` is filled from
   `HerdrServerInfo.capabilities` whenever the server answered, including
   `BelowMinimum` and `ClientServerMismatch` (Herdr master
   `src/cli/status.rs:176-187,301-329` emits it with `compatible: false`),
   and snapshotted for v0.8.0, v0.8.2 and 3a822e81 (AYP-R5-004,
   AYP-R6-001); the breaker stays the
   separate `herdr.breaker` report; no socket/pipe path computed by atm.
   `atm doctor --json` gains `herdr.configured` (bool),
   `herdr.endpoints[]` (deterministic order: `default`, then sessions
   sorted bytewise; snapshot test), `herdr.breaker`; no aggregate
   `herdr.state`/`herdr.remedy` (AYP-R4-002); an unreadable config or roster yields
   `herdr.configured: null` plus an error field, so `daemon-switch`
   fails closed.
3. One escalation per breaker-open through the AX.6 helper
   `crates/atm-http-runtime/src/herdr_escalation.rs::escalate(runtime,
   herdr_process, task_store, daemon_home, team, mail_body, notification,
   kind) -> EscalationOutcome` (`pub(crate)`, verified at 5120e0800:139;
   no rename), so the breaker-open call site lives in atm-http-runtime
   next to the queue-wake pump that already observes the breaker, with an
   `EscalationNotification` carrying the doctor state text:
   queued ATM mail to the lead and configured recipients is the durable
   path, Herdr desktop notify is best-effort, dedup keyed on the
   breaker's open timestamp, never the mail body (HR-SAFE-003 guard
   extended to the new call site).
4. Lifecycle tests, real-startup integration with negative proof (the
   daemon-composed-feature rule): (a) daemon reaches ready with no
   `herdr` binary and none configured; tmux and hermes nudges succeed;
   (b) fake herdr always `server_not_running`: breaker opens, doctor
   reports, daemon stays up, one notification; (c) grep guard: no code
   path in atm-daemon-bootstrap or atm-http-runtime spawns `herdr server`
   or any long-lived Herdr child; (d) fake herdr enters the
   `protocol_mismatch` state mid-run: doctor text, one notification,
   recovery on the next half-open; (e) connection reset during a `wait`:
   no duplicate prompt, pending mail re-nudged by the pump; (f) daemon
   restarted with queued mail: backlog drains; (g) below-minimum fake:
   doctor finding, calls fail on Herdr's own terms; (h) every
   `HerdrDoctorState` variant is produced by a fake-herdr mode or config
   fixture and rendered with its remedy, and no listed `HerdrError`
   variant maps to `Other`; (i) breaker opens while the fake's
   `notification show` fails: lead mail and recipient mail are written,
   `notify_ok == false`, exactly one escalation for the open cycle; (j)
   open breaker: the Bypass probe still reports the live endpoint state
   and `herdr.breaker` reports open; (k) roster with two named sessions
   plus one default member yields three endpoint records with correct
   provenance, and a configured `socket_path` yields provenance
   `socket_path` for members without a session.

Acceptance criteria:

1. Tests (a) through (k) present and passing on all three lanes.
2. Req-qa can map every `HerdrDoctorState` variant to a test.
3. Escalation count asserted at exactly one per breaker cycle, with mail
   written even when Herdr notify fails.
4. `atm doctor --json` schema for `herdr.*` documented in
   `docs/atm/cli-reference-1-5-0.md` and covered by a snapshot test.
5. RULE-003 respected (new production code outside herdr_queue_wake.rs).
6. `boundaries/atm-core/herdr-endpoint-doctor.toml` is the first commit
   and matches the P-E-approved file; `just lint boundaries` passes; the
   impl-count test finds exactly two `HerdrEndpointDoctor` impls;
   `HerdrPresenceDoctor` no longer exists in the workspace (grep gate)
   (AYP-R4-003).
8. `daemon_herdr_client_config` tests: no file, file without `[herdr]`,
   both keys set, unknown key, malformed TOML, relative `socket_path`
   (AYS-R5-002); the stage-1 push (boundary TOML plus deliverable 2) is
   reviewed by fenix before stage 2 starts (AYS-R5-001).
7. `presence_findings` zero-regression tests (visible in every
   `AgentStatus`, not-visible, failed, infrastructure-failed) pass with
   findings byte-identical to the AX adapter on the same recordings;
   `live_handoff` is `Some(true)` on a `ClientServerMismatch` fixture whose
   server advertises it (AYP-R6-001/002); `herdr_is_configured` tested
   against a roster with tmux, hermes and Herdr members (AYP-R5-002).

Validation:

1. `just validate`; `cargo test -p atm-core -p atm-http-runtime` for the
   focused loop.
2. `python3 .just/check_line_counts.py`.
3. The architecture guard suite (part of `just validate`).

Out of scope: installer changes (AY.4), Windows (AY.5).

## Sprint AY.4: installer (size L)

Branch `feature/ay4-daemon-switch-herdr`, created from
`feature/ay3-herdr-startup-failure-model` (stacked). Dependencies:
must_follow AY.3 (consumes `atm doctor --json` `herdr.configured` and
`herdr.endpoints[]`); parallel_safe AY.6. recommended_agent
arch-ctm/deep-reasoning (governance text plus a small platform-native
transaction). Scope is the `daemon-switch` control plane
(REQ-P-DAEMON-SWITCH-001, ADR-053, `.claude/skills/daemon-switch/SKILL.md`).
Size L (AYS-R2-005) and internal order: deliverables 0 and 1
(governance text, predicate fixtures) are pushed and reviewed by fenix
in the PR before 2 and 3 (entry transaction, restart coordination)
start; quality-mgr reviews the finished sprint once. Rand's seven-sprint
ruling holds; nothing is split out.

Deliverables:

0. **Governance.** `REQ-P-DAEMON-SWITCH-002` in `docs/requirements.md`
   and an ADR-053 addendum, transcribed from the "Governance" and "Error
   inventory" paragraphs of "Upgrade and restart coordination" (approved
   with this plan): explicit, operator-invoked subcommand `daemon-switch
   herdr-entry {install,remove,status [--repair]}`, never run implicitly;
   identifiers per endpoint; ownership marker; the entry transaction
   (journal record type, plan/write/register/verify, repair); refusal
   rules and error codes; fail-closed on ambiguity.
1. **Configured predicate consumption.** `daemon-switch` decides
   "Herdr-configured" only from `atm doctor --json` (`herdr.configured`
   must be exactly `true`; `false`, `null`, missing or a doctor error
   means not configured and, for `install`, a refusal naming the
   reason). No Python re-implementation of the Rust predicate. Fixtures
   prove the decision for: true, false, null with error, malformed
   JSON, doctor exit non-zero.
2. **Herdr start-at-login entries** via `herdr-entry install`, only
   when configured: one entry per distinct configured endpoint
   ("Installer": `herdr server` or `herdr --session <name> server`,
   deterministic identifiers), macOS LaunchAgent (`RunAtLoad`, no
   `KeepAlive`), Linux user unit, Windows per-user logon task under the
   same account as the daemon, each written through the entry
   transaction with the journal record; `herdr-entry remove` removes
   only marker-bearing entries; `status --repair` completes or rolls
   back an incomplete journal; session-0 / service installs on Windows
   refused with `HERDR_ENTRY_ACCOUNT_MISMATCH`; the skill doc's install
   procedure gains the step.
3. **`--restart-herdr <endpoint>`** (restart only, one endpoint per
   invocation, AYP-R4-001) per "Upgrade and restart coordination":
   endpoint resolved against doctor `herdr.endpoints[]` (unknown,
   ambiguous and `socket_path` cases refused with their codes); scoped
   `server live-handoff` when the installed binary is newer than that
   endpoint's server and its `capabilities.live_handoff` is `true`
   (`ClientServerMismatch` state, AYP-R6-001); otherwise refuse
   without `--stop-herdr-panes`, then scoped `server stop` and relaunch
   via that endpoint's entry, verified by a doctor re-read; never
   restarts the daemon; the daemon restart step refuses with
   `HERDR_RESTART_ENDPOINTS_PENDING` while any endpoint is
   `ClientServerMismatch`; rejected when not configured; never runs
   `herdr update`.
4. **Doctor** reports each entry's presence and marker per endpoint,
   the stale-entry and missing-entry findings, and, on Windows, the
   account and session of each task; skill doc updated.

Acceptance criteria:

1. Not-configured host: `herdr-entry install` refuses with
   `HERDR_NOT_CONFIGURED` and writes nothing (test per fixture in
   deliverable 1).
2. Configured host: exactly one marker-bearing entry per distinct
   endpoint written (fixtures: default only; default plus two named
   sessions); `remove` removes them; `remove` with a foreign (unmarked)
   entry present is a no-op with a message (tests).
2b. Failure injection: crash between write and register, and between
   register and verify, leaves a journal that `install`/`remove` refuse
   with `HERDR_ENTRY_JOURNAL_ACTIVE` and `status --repair` resolves; a
   marker-bearing entry whose digest differs is refused, never
   overwritten (tests).
2c. Every code in the error inventory has a fixture and a JSON snapshot
   test; exit statuses 0/3/4 asserted.
3. `install` with an unmarked entry of the same identifier refuses
   (test).
4. `--restart-herdr` refusals tested against a fake Herdr lacking
   `live_handoff` (read from the doctor fixture's
   `capabilities.live_handoff`, AYP-R5-004) and `null`; the positive
   fixture (newer installed client, older running server in
   `ClientServerMismatch`, `live_handoff: true`) selects the scoped
   live-handoff path without `--stop-herdr-panes` (AYP-R6-001); against
   doctor fixtures with
   two configured endpoints and no selector
   (`HERDR_RESTART_ENDPOINT_REQUIRED`), an unknown selector, a
   `socket_path`-provenance endpoint, a mixed fixture (`socket_path` set
   plus two named sessions: sessions restart, default refused,
   AYP-R5-005), and a pending `ClientServerMismatch` on the daemon
   restart step; the scoped argv (`herdr --session <name> server ...`)
   and the `herdr_entry_identifier` result asserted per fixture for
   default and named endpoints on macOS, Linux and Windows (AYP-R5-006);
   the `herdr update` string does not appear in daemon-switch sources
   (grep gate).
5. The installer never starts Herdr itself outside `--restart-herdr`
   and never probes whether it is running (grep gate).
6. REQ-P-DAEMON-SWITCH-002 and the ADR-053 addendum present; req-qa can
   map each rule to a test.

Validation:

1. `just validate`; daemon-switch's own test suite.
2. Doc-lint for the requirements, ADR and skill doc updates.

Out of scope: Windows live verification (AY.5); any Herdr upgrade
operation.

## Sprint AY.5: Windows process correctness and live evidence (size M, FastPC4)

Branch `feature/ay5-windows-herdr-evidence`, created from
`feature/ay4-daemon-switch-herdr` (stacked). Dependencies: must_follow
AY.4 and preconditions P-C, P-D; parallel_safe AY.6. Dev agent: the
named Windows agent (P-D). AY.5 does not dispatch while P-C or P-D is
unset; the phase proceeds with the other tracks.

Deliverables:

1. Windows process correctness inside `transport_cli.rs` only:
   `herdr.exe` resolution via `binary_path` (file or directory) or PATH
   on every spawn, never cached, alias directory documented as the
   default configuration; CREATE_NO_WINDOW; kill-then-reap verified to
   leave no orphan in `tasklist`; UTF-8 stdout handling. No
   `cfg(windows)` outside the transport module (architecture test).
2. Windows branch of the installer live-verified: logon tasks for the
   daemon and Herdr in the user's session; refusal of service/session-0;
   doctor account/session report.
3. Windows-observed columns of `docs/atm-herdr/windows-process-audit.md`
   filled from the live run (CRLF observation, detached stdio, console
   flash, pipe name Herdr reports).
4. Live Windows evidence (AX.7 evidence table format, owned by AY):
   doctor herdr section; prompt/wait/get/list/notify round trips with
   observed argv and JSON; end-to-end nudge from another host with
   timestamps at both ends; transport-boundary structured logs; negative
   cases live (Herdr stopped: daemon stays up, tmux/hermes unaffected,
   breaker opens and recovers; agent not found; agent blocked; slow
   command hits the 5 s cap with no orphan); late-start case; upgrade
   case (`herdr update` without restart: mismatch state, doctor text,
   notification, recovery after `--restart-herdr`); nudge latency
   sample; explicit confirmation no console window flashes. Evidence is
   captured by the FastPC4 team and committed byte-for-byte; agents never
   author evidence records.

Acceptance criteria:

1. Deliverables 1 through 4.
2. The Windows CI job stays the merge gate.
3. FastPC4 evidence is the release-readiness gate for Windows Herdr parity.

Validation:

1. `just validate` on Windows (FastPC4) and the three CI lanes.
2. Evidence `cmp` against the captured artifacts before commit.

Out of scope: socket transport on Windows (AY.7).

## Sprint AY.6: direct socket/pipe transport, no cutover (size L)

Branch `feature/ay6-herdr-socket-transport`, created from
`integrate/phase-ay` once AY.1, AY.2 and AY.3 have all merged (a
multi-parent join, not stacked; stacking rule under "Sprint map").
Dependencies: must_follow AY.1 (`docs/atm-herdr/herdr-versions.md`
exists; ADR-058 D3 amended text is the protocol authority), AY.2
(`HerdrIo`, fake-herdr replay recordings) and AY.3 (its
`boundary_enforcement.rs` grep guard precedes AY.6's edits to the same
file, AYP-R3-011); precondition P-E (boundary revision approved);
parallel_safe AY.4, AY.5 (see rationale above). recommended_agent
arch-ctm/deep-reasoning. Size L is kept as one sprint on purpose: the
Windows named-pipe branch is `cfg(windows)` code inside the same module,
compiled and tested against the fake pipe server on the windows-latest
CI lane; live Windows verification is AY.7, so AY.6 has no FastPC4
dependency. Splitting it would create a sprint of a few dozen lines with
its own QA cycle for no risk reduction.

Deliverables:

0. **Guard exemption.** `ai11_guarded_workspace_sources` in
   `crates/atm-architecture/tests/boundary_enforcement.rs` excludes
   exactly one extra path, `crates/atm-herdr/src/transport_socket.rs`,
   with a rationale comment citing ADR-058 D3 and this sprint; a new
   assertion pins the exemption list to that single entry so the
   retired-transport ban keeps applying to every other file. Landed as
   the second commit, after the P-E TOML diff.
1. **Socket transport.** `crates/atm-herdr/src/transport_socket.rs`
   adding `HerdrIo::Socket(SocketIo)` (crate-private) with no child
   process:
   `tokio::net::UnixStream` on unix, `tokio::net::windows::named_pipe`
   on Windows (tokio already allowed in atm-herdr; no new crate edge).
   The endpoint is exactly what Herdr's own client resolves for the
   same inputs (AYP-R3-004; `src/api/client.rs` `ConnectionTarget`,
   `src/session.rs` `api_socket_path_for`/`data_dir_for`,
   `src/config/io.rs` `config_dir`; byte-identical v0.8.2..master):
   explicit `socket_path` > session-derived > default;
   `<config_dir>/herdr.sock` or `<config_dir>/sessions/<name>/herdr.sock`,
   where `config_dir` is `$XDG_CONFIG_HOME/herdr` when that variable is
   set (all platforms), else `~/.config/herdr` on macOS/Linux and
   `%APPDATA%\herdr` on Windows (`herdr-dev` in Herdr debug builds is
   never matched). On Windows the **same path string** is the pipe name:
   Herdr hands it to `interprocess` `GenericNamespaced`, which prefixes
   `\\.\pipe\` (`src/ipc.rs:44-51`), so atm connects to
   `\\.\pipe\C:\Users\<user>\AppData\Roaming\herdr\herdr.sock`
   (or the `sessions\<name>\` variant, or `\\.\pipe\<socket_path>`).
   atm uses `tokio::net::windows::named_pipe::ClientOptions::open` and
   `tokio::net::UnixStream::connect` directly and does not depend on
   `interprocess`. This is the one place atm derives a Herdr path,
   because a socket client cannot delegate resolution to the `herdr`
   binary; the platform variables it reads (`XDG_CONFIG_HOME`, `APPDATA`,
   `HOME`) are captured once at composition and injected, never read
   inside the transport.

   ```rust
   /// Where Herdr's NDJSON API listens, as Herdr's own client resolves it. Pure; no probe, no I/O.
   pub fn herdr_api_endpoint(cfg: &HerdrClientConfig, session: Option<&HerdrSession>, env: &HerdrHostEnv) -> HerdrEndpoint;
   pub struct HerdrHostEnv { pub xdg_config_home: Option<PathBuf>, pub appdata: Option<PathBuf>, pub home: Option<PathBuf>, pub platform: Platform }
   pub enum HerdrEndpoint { UnixSocket(PathBuf), NamedPipe(String) }   // NamedPipe carries the full \\.\pipe\... name
   pub(crate) struct SocketIo { cfg: HerdrClientConfig, env: HerdrHostEnv, max_line_bytes: usize /* 1 MiB */ }
   // HerdrIo::call for Socket: resolve the endpoint from (cfg, per-call session, env), open a
   // fresh connection, one request line, one response line -> HerdrEnvelope, close. No ping on
   // the hot path (AYP-R3-010): Herdr serves exactly one request per accepted connection
   // (src/api/server.rs handle_connection_with_stop, identical at v0.8.0, v0.8.2, 3a822e81).
   // server_info(session) opens its own one-request connection carrying `ping`
   // {version, protocol, capabilities} on that endpoint; doctor only.
   ```

   Pinned byte-for-byte tests: unix default, unix named session,
   `XDG_CONFIG_HOME` override, Windows default
   (`\\.\pipe\C:\Users\u\AppData\Roaming\herdr\herdr.sock`),
   Windows named session, `socket_path` override on both; AY.7 adds the
   pipe name observed on FastPC4 (AY.5 audit row) as a fixture.
2. **Protocol** per ADR-058 D3 (as amended by AY.1): **one request per
   connection** (AYP-R3-010: Herdr's `handle_connection_with_stop` reads
   one initial request and returns after serving it, at v0.8.0, v0.8.2
   and 3a822e81): fresh connection, one NDJSON request line, one
   response line, close; unknown fields tolerated. No `ping` on the nudge
   path; version and minimum checks are doctor-only through
   `HerdrIo::server_info`, which opens its own one-request connection
   for `ping` (below minimum is a doctor finding, never a refusal). Request ids
   `atm:agent:<cmd>`. `agent.prompt` with `wait` maps `timeout` and
   `agent_prompt_stalled` to the same outcome. Bounds (AYP-R3-010): one
   absolute `RequestDeadline` covers connect (including the Windows
   `ERROR_PIPE_BUSY` retry loop) and the write, flush and read of the
   single request together; exhaustion at any step is the
   existing `HerdrError::Timeout`. Lines are read with a hard cap of
   1 MiB (Herdr's own `MAX_INITIAL_REQUEST_BYTES`); an oversized line or
   a stream that closes without `\n` is the existing
   `HerdrError::InternalError` (infrastructure class, opens the breaker)
   logged with the byte count at the transport boundary. Fake-server
   cases: no newline, oversized response, stalled connect, stalled
   write, stalled read, a second request line on one connection refused
   by the fake exactly as Herdr refuses it (guard test that the
   transport never writes two), pipe busy then free (Windows lane).
3. **Compatibility matrix.** `docs/atm-herdr/herdr-versions.md` (created
   by AY.1) gains the NDJSON columns per release (ping fields, request
   shapes, error codes); keyed on `ping.version`/`capabilities`, never
   PROTOCOL_VERSION.
4. **Equivalence.** The whole ADR-058 fixture suite plus AY.2's replay
   tests run through `HerdrProcessInvoker` constructed over each
   `HerdrIo` variant with identical assertions (the public adapter is
   the only surface under test); a fake
   Herdr socket/pipe server fixture (test-only) mirrors the fake
   binary's modes and per-version replay, including server absent and
   late start, on all three CI lanes (the Windows lane exercises the
   named-pipe path).

5. **Public-item pin amendment (AYS-R3-002).** The AY.2 architecture
   test's expected list gains exactly `herdr_api_endpoint`,
   `HerdrHostEnv` and `HerdrEndpoint`, with a comment citing this
   sprint; `SocketIo` stays `pub(crate)`.

Files AY.6 edits (AYP-R3-011), compared against AY.3/AY.4/AY.5: new
`crates/atm-herdr/src/transport_socket.rs` and
`crates/atm-herdr/tests/support/fake_herdr_socket/`; edits to
`crates/atm-herdr/src/transport.rs` (the `Socket(SocketIo)` arm of
`HerdrIo` and its two match arms), `crates/atm-herdr/src/lib.rs` (one
`mod transport_socket;` line), `crates/atm-herdr/Cargo.toml` (tokio
`net` feature if not already enabled),
`crates/atm-architecture/tests/boundary_enforcement.rs` (deliverable 0),
the AY.2 public-item pin test under `crates/atm-architecture/tests/`
(deliverable 5), `boundaries/atm-herdr/herdr-process-adapter.toml` (P-E),
`docs/atm-herdr/herdr-versions.md`. None of these is touched by AY.4 or
AY.5 (their file sets are listed under "Sprint map"), so parallel_safe
holds on file evidence, not on a clean merge; AY.3 also edits
`boundary_enforcement.rs`, which is why AY.6 must_follow AY.3
(AYP-R3-011). No
composition change: `build_replacement_handler` is byte-identical to its
parent commit, and an architecture test asserts that `HerdrIo::Socket(`
is constructed only inside `transport_socket.rs` `#[cfg(test)]` modules
and `crates/atm-herdr/tests/` (path allowlist) until AY.7 removes the
allowlist. Selection and default live in AY.7.

Acceptance criteria:

1. P-E TOML diff is the first commit and matches the approved fragment.
2. Guard exemption (deliverable 0) present with the single-entry
   assertion; the AI.11 suite passes.
3. Equivalence suite green on both transports on all three CI lanes.
4. `git diff <parent>..HEAD -- crates/atm-daemon-bootstrap` is empty and
   the `HerdrIo::Socket(` construction-site allowlist test passes; the
   PR's changed-file list is a subset of the list above (fenix checks).
5. Bounds tests (deliverable 2) present on all three lanes.
6. `herdr-versions.md` has NDJSON columns for every release from 0.8.0.
7. Official benchmark run on the hot path; no regression.
8. The AY.2 public-item pin test passes with exactly `herdr_api_endpoint`,
   `HerdrHostEnv`, `HerdrEndpoint` added (AYS-R3-002).

Validation:

1. `just validate` (all three lanes via CI); `cargo test -p atm-herdr`.
2. `python3 .just/check_line_counts.py`.
3. `just benchmark-official --branch integrate/phase-ay` from the dedicated
   benchmark account per `docs/plans/phase-ao2/readiness.md` (exit 0
   against published floors; report directory cited in the PR).

Out of scope: transport selection and default flip (AY.7); live
evidence (AY.7); doctor changes (AY.7).

## Sprint AY.7: socket cutover and live evidence (size M)

Branch `feature/ay7-herdr-socket-cutover` off `integrate/phase-ay` after
AY.5 and AY.6 have both merged (join; not stacked, see stacking rule).
Dependencies: must_follow AY.5 (Windows process and installer facts
live-verified) and AY.6 (socket code). recommended_agent
arch-ctm/deep-reasoning; Windows evidence via the P-D agent.

Deliverables:

1. **Transport selection in the composition root.** The one
   `HerdrProcessInvoker::new` site from AY.3 passes
   `herdr.transport = "socket" | "cli"` (default `socket`) and the
   invoker builds the matching `HerdrIo` variant; the CLI transport is retained as a
   documented, explicit config fallback (never silent) for exactly one
   atm minor version after the version that ships the cutover, then
   removed (tracked in the project plan). No other composition change.
2. **Lifecycle tests re-validated on the socket default.** AY.3's tests
   (a) to (k) run against the socket transport with the semantics
   adapted: (a) "no `herdr` binary" becomes "no endpoint reachable and
   not configured"; (b) `server_not_running` becomes connection refused
   or endpoint absent; (d) `protocol_mismatch` cannot occur on the
   socket path, so adapted (d) is "the fake switches its recording set
   (server version) between two calls with the daemon running: both
   calls succeed, doctor shows the new version, no restart", which is
   distinct from (g) "below-minimum fake: doctor `BelowMinimum` finding,
   calls continue on Herdr's own terms", carried over unchanged
   (AYS-R2-004); (e), (f), (h), (i), (j), (k) unchanged in intent. The
   CLI variants keep running while the fallback exists.
3. **Live evidence** on macOS and Windows, same set as AY.5 deliverable
   4, through the socket transport, including the upgrade case (socket
   client is immune to `protocol_mismatch`; evidence shows nudges
   continuing across a `herdr update`).
4. **Doctor** reports, per endpoint record, the transport in use
   (`HerdrTransportKind::Socket`) and the resolved endpoint in the
   atm-core-owned `HerdrEndpointObservation::endpoint: Option<String>`
   field (AYS-R3-001): `HerdrDoctorProbe` (atm-herdr, which depends on
   atm-core) renders its own `HerdrEndpoint` through `Display` into that
   field; `HerdrEndpoint` itself never appears in atm-core or
   atm-daemon-bootstrap (the existing forbidden-edge test plus a grep
   assertion). `HerdrDoctorProbe::new` gains the transport selection from
   the same config value.

Acceptance criteria:

1. Default transport is `socket` (test) and `herdr.transport = "cli"`
   selects the CLI transport (test); no third value accepted.
2. Adapted lifecycle tests (a) to (k) pass on all three CI lanes for the
   socket default, with (d) and (g) as two distinct tests; CLI variants
   still pass.
3. Evidence committed byte-for-byte for both platforms.
4. AY.2 zero-regression oracle green on the socket transport.
5. Fallback documented with its removal version in the project plan.
6. Doctor shows transport and endpoint (test).

Validation:

1. `just validate` (all three lanes via CI).
2. Evidence `cmp` against captured artifacts before commit.
3. `just benchmark-official --branch integrate/phase-ay` per
   `docs/plans/phase-ao2/readiness.md` (exit 0; report cited in the PR).

Out of scope: any new Herdr capability adoption; removing the CLI
transport.

## Phase AY exit gate (AY.7 disposition)

Phase AY is not complete until a dated decision on AY.7 (socket cutover)
is recorded here and in `docs/project-plan.md`, chosen by Rand from
exactly these:

- **Ship**: AY.7 merged, acceptance above met.
- **Defer**: AY.7 deferred to a named phase, ADR-058 D3 amended to say
  the CLI transport is the supported design until then; AY.6 code stays
  behind explicit config.
- **Cancel**: AY.7 dropped, ADR-058 D3 rewritten to make the CLI
  transport the permanent design, and a cleanup PR merged before phase
  closure that deletes `transport_socket.rs`, the fake socket server,
  the `herdr_local_socket_client` ownership key, the AI.11 exemption
  line and the NDJSON columns; the AY.6 and AY.7 sections here marked
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
one (child env `HERDR_SESSION`, HR-CORE-006, retained); the AY.6 socket
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
(pure motion inside the crate). AY.6 adds `herdr_local_socket_client`
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

- Under the CLI transport (AY.2 through AY.5) the check is Herdr's own and
  passes whenever the CLI binary and the running server come from the
  same install. After cc88b3b8, `herdr update` keeps a compatible old
  server running and `herdr status` no longer flags PROTOCOL_VERSION
  drift as needing a restart, so "new CLI, old server, every atm nudge
  fails with `protocol_mismatch`" is a normal post-update state. atm maps
  the code (existing `HerdrError::ProtocolMismatch`), the breaker opens,
  and doctor names the remedy (restart Herdr). No atm release is
  involved.
- Under the socket transport (AY.6, AY.7) atm never sees `protocol_mismatch`.
  Compatibility becomes atm's own responsibility and is keyed on
  `HERDR_MINIMUM_VERSION` and the `ping` result (`version`,
  `capabilities`), never on PROTOCOL_VERSION. This is why AY.6's blocking
  prerequisite is the conformance matrix, not the socket code.
- ADR-058's pin of `d79fd746` / 21 is replaced by `HERDR_MINIMUM_VERSION`
  and the per-release table in `herdr-versions.md` (AY.1).

## Ordering with phase AX

Status 2026-09-05 (fenix@rand-m5): AX.1 through AX.5 are merged into
integrate/phase-ax; AX.6 (PR #1204) is in its second fix pass; AX.7
(PR #1206, stacked on AX.6) is the macOS live-proof sprint and keeps its
own accepted scope (Windows runs are out of AX.7 scope and stay so; AY.5
owns Windows evidence). One PR integrate/phase-ax -> develop follows
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
3. Orphaned herdr.exe after timeout: explicit live-verified test (AY.5).
4. Console flash per nudge: CREATE_NO_WINDOW plus visual confirmation
   (AY.5).
5. Merge collision with AX: AY branches from phase-ax after AX.6 and
   merges forward after the phase-ax PR lands (P-A).
6. Hot-path regression: official benchmark run before merge (AY.2, AY.6, AY.7).
7. AY.7 (socket cutover) quietly dropped again: the exit gate requires a dated
   Ship/Defer/Cancel decision line before the phase PR.
8. FastPC4 not ready: only AY.5 depends on it (P-C/P-D), and AY.7 waits
   for AY.5. AY.1 through AY.4 and AY.6 land without a Windows machine;
   the phase cannot close with Windows parity claimed until AY.5 evidence
   exists.
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
  `atm-dev` team on FastPC4; AY.5 dev, the Windows-observed audit rows
  and the live-evidence deliverable execute there, inside a Herdr
  session, with the daemon and Herdr installed per-user by
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
