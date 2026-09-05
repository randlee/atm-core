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
   When atm is configured to work with Herdr (herdr backend enabled in
   daemon config and the roster has Herdr harness members), the service
   installer (`daemon-switch`) also installs a Herdr start-at-login entry
   so Herdr is normally up before the daemon. When atm is not configured
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
corrects the ADR-058 Herdr pin, and (AY.3 only) revises the boundary TOML
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
   (lib.rs:1095, 1111, 1130, 1153, 1171, 1191; /bin/sh fixtures). The
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

Ruling (Rand, 2026-09-05, recorded in randlee/atm-core#1217): Herdr drifts
outside our control, and atm-core is responsible for supporting **every
Herdr version at or above `HERDR_MINIMUM_VERSION`**, following the same
schema-management pattern as the HTTP API semver rules. Rand's minimum
today is **0.8.0**. This replaces the previous revision's "pin v0.8.2"
approach.

Plan-declared schema rules (checked by the `schema-reviewer` agent, PR
#1218, at plan review and phase-ending review):

1. `crates/atm-herdr` declares `HERDR_MINIMUM_VERSION = 0.8.0` (Herdr
   release version, semver, as reported by `ping.version` and `herdr
   --version`). One daemon build supports every Herdr version at or above
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
   `PROTOCOL_VERSION`, the observed argv/JSON for the five commands atm
   uses, and the drift items; the fake-herdr fixture (AY.1 deliverable 3)
   replays each version's recorded responses, so the full ADR-058 suite
   runs once per supported version on every CI lane.
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

Today `crates/atm-herdr` has no version constant and a mismatch surfaces
only as `HerdrError::ProtocolMismatch`. AY.1 lands rules 1 to 5 for the
CLI transport; AY.3 extends the same matrix to the direct socket client.

### Mechanism: versioned protocol adapters, selected per call (Rand's question, 2026-09-05)

Version handling is a second seam next to the transport, not a set of
`if version >= x` branches scattered through the client:

- **Two traits.** `HerdrTransport` moves bytes (CLI child process today,
  socket/pipe in AY.3). `HerdrProtocolAdapter` gives those bytes their
  meaning for one Herdr behaviour epoch: request -> argv/NDJSON,
  response -> `AgentSnapshot`/`HerdrError`, and the `--wait` outcome
  rules. One adapter per epoch, not per patch release: `v0_8_0` (0.8.0
  through 0.8.2 semantics) and `v0_8_3` (master after 8633a398/7916be16)
  today; a new adapter is added only when the drift check finds a
  behaviour atm must absorb. Adapters are ordinary injectable
  implementations, so a test can inject a fake adapter or a fake
  transport independently.
- **All supported adapters are compiled into one daemon build** (rule 1)
  and registered in a `HerdrAdapterRegistry` at composition time. What is
  injected at startup is the registry and a `HerdrVersionResolver`, not
  a chosen version.
- **Selection is per call, from the live Herdr.** Every call already
  spawns a fresh `herdr` process (or, in AY.3, opens a fresh connection
  and pings), so there is no session state that would pin a version.
  CLI transport: the version is the resolved binary's `herdr --version`,
  which equals the server's whenever a call can succeed at all (Herdr's
  own CLI/server equality check, see "PROTOCOL_VERSION and
  compatibility"); it is re-read whenever the resolved binary path or
  its mtime changes, after any `protocol_mismatch`, and on every breaker
  half-open transition, otherwise cached. Socket transport: `ping.version`
  on each connection. The resolver maps the version to the newest
  adapter whose epoch floor it meets; a version below
  `HERDR_MINIMUM_VERSION` is a `ServerUnavailable`-class failure with the
  version in the cause and a doctor finding.
- **Consequence for upgrades:** Herdr upgrading, or downgrading, while
  the daemon runs requires **no daemon restart**. The next call after the
  change selects the matching adapter; the only visible effect is the
  transient `protocol_mismatch` window Herdr itself creates when the CLI
  binary and the still-running old server differ, which the breaker and
  doctor already cover. A Herdr release newer than every known epoch is
  served by the newest adapter (additive adoption, rule 3); the drift
  check, not the daemon, decides whether a new epoch is needed.
- **Tests (AY.1):** the conformance suite runs every adapter against its
  per-version fake (rule 4); a lifecycle test switches the fake Herdr's
  reported version between two calls with the daemon running and asserts
  the adapter changes with no restart and no breaker trip; a
  below-minimum fake asserts the doctor finding and the failure class.

Windows note: 0.8.0 is the minimum for macOS and Linux. `herdr.exe` is a
released artifact from v0.8.2 onward (9fac5172 "make Windows generally
available", 2026-08-18, `release.yml` `x86_64-pc-windows-msvc`,
`windows-arm64.yml`); v0.8.0 shipped Windows as beta without a release
job. The Windows platform floor is therefore 0.8.2, recorded as a platform
fact in `herdr-versions.md`, not as a raised minimum.

### Reference checkout and release state (2026-09-05)

| Item | Value |
|---|---|
| Reference checkout | `/Users/randlee/Documents/github/herdr`, branch master, HEAD `3a822e81` (2026-09-05) |
| Releases in range | v0.8.0 = `857196de` (2026-08-03, PROTOCOL_VERSION 19); no v0.8.1 tag; v0.8.2 = `34ba52cc` (2026-08-19, PROTOCOL_VERSION 20) |
| After v0.8.2 | 92 commits to 3a822e81; `Cargo.toml` still 0.8.2; newest tag `preview-2026-08-31-b1ff4582e968` (HEAD is 41 past it, untagged); preview channel publishes from master; no sign of an imminent release |
| Superseded pins | ADR-058 cites `d79fd746` / PROTOCOL_VERSION 21; the previous revision of this plan pinned v0.8.2 / 20. Both replaced by the minimum-version rule (AY.1 deliverable 0 amends ADR-058) |

### Drift v0.8.0 -> v0.8.2 (to be completed by AY.1 deliverable 0)

`--timeout MS` on `agent prompt`/`agent wait`, `notification show <title>
[--body TEXT] [--sound none|done|request]` and the `--wait` activity gate
(5000 ms state-change requirement, `agent_prompt_stalled`) all exist at
v0.8.0 (`src/cli/spec.rs:294-357`, `src/cli/notification.rs:28` at the
tag). Between v0.8.0 and v0.8.2 the client-facing files changed by
544/104 lines (`src/cli/agent.rs`, `src/cli.rs`, `src/cli/spec.rs`,
`src/api/server.rs`, `src/api/schema/response.rs`, `src/ipc.rs`,
`src/integration/env.rs`), and `src/cli/agent.rs` gained the
`agent_not_ready`, `agent_pane_busy` and `agent_status` codes. AY.1
deliverable 0 records the per-command diff for the five commands atm
uses and the full error-code delta, so the v0.8.0 conformance fixture is
grounded in the tag rather than assumed.

### Drift v0.8.2 -> master 3a822e81 (two background agents, 2026-09-05)

| Area | Change | Effect on atm |
|---|---|---|
| `agent prompt --wait` | Activity gate now requires an observed `working` or `blocked` status (8633a398, 7916be16; `src/api/wait.rs:177-320,516`); a prompt into an already-working agent skips the gate; done/idle churn without `working` ends in `timeout`; blocked-after-submit returns success with `agent_status: blocked`; `agent_prompt_stalled` message text changed (`wait.rs:662-664`); on Windows the dispatch is bounded by the caller timeout and returns `timeout`, not `server_unavailable` | Material for HR-CORE-002/003: the same nudge can end in `timeout` on a newer Herdr where v0.8.2 returned `agent_prompt_stalled`. atm keys on codes, never message text; AY.1 deliverable 0 re-verifies the `HerdrError` mapping and the queue-pump/reminder behaviour under both outcomes (rule 3) |
| Update model | `herdr update` keeps a compatible old server running (cc88b3b8, `CompatibleServerKept`); `herdr status` `restart_needed` keys on endpoint generation, not PROTOCOL_VERSION | An updated CLI binary against a still-running old server fails every CLI command with `protocol_mismatch`, and that state is now normal and sticky; doctor must make it legible; direct NDJSON (AY.3) is immune |
| PROTOCOL_VERSION | 20 -> 22 (d79fd746, 99c23cd1); versions the bincode client socket, not the NDJSON API | See "PROTOCOL_VERSION and compatibility" |
| Windows PATH | Installer prepends the versioned release dir to the user PATH (8162e265, `distribution/install.ps1:881-904`); `%LOCALAPPDATA%\Programs\Herdr\bin` stays as a junction alias | Never persist a resolved `herdr.exe` path; resolve per spawn; configured `binary_path` may point at the alias |
| CRT | x86_64 build statically links the CRT (73825652) | No VC++ redistributable needed; arm64 not covered |
| `--no-session` | Removed (207be3c7) | Never passed by atm; nothing to do |
| `herdr server` | Entry unchanged (`src/main.rs:542`); `run_server` moved to `src/server/headless/bootstrap.rs` (207be3c7) with identical duplicate detection and restore | No change |
| Endpoint resolution: `ipc.rs`, `session.rs`, `api/client.rs`, `socket_paths.rs` | Byte-identical across the range | No change |
| NDJSON envelope, `agent.*` shapes, limits, `notification show`, CLI argv and exit codes | No change; additive methods and fields only (`ping.capabilities.endpoint_protocol_generation`, `workspace.close.close_group`, `worktree.*.trust_repository`, new `pane.*`, `command.invoke`, `integration.list`) | Parsers must tolerate unknown fields; AY.1 asserts it |
| `events.subscribe` | Starts at the live sequence, no replay (20a500a7) | Irrelevant unless AY.3 subscribes; documented |
| Autostart | Still none at HEAD (grep for login item / LaunchAgent / RunAtLoad / autostart / schtasks / Register-ScheduledTask / systemd / SMAppService: only SSH keepalive hits) | Confirms the installer-owned start-at-login entry design |
| API pipe ACL | Unchanged: `restrict_socket_permissions` is a no-op on Windows (`src/ipc.rs:342-345`); the SDDL DACL at `ipc.rs:141-167` serves only the remote-attach bridge | AY.3 boundary revision records the API pipe as default-DACL |

Summary for the five commands atm uses (`agent prompt`, `agent wait`,
`agent get`, `agent list`, `notification show`): argv, flags, JSON
shapes, exit codes and error-code set are unchanged from v0.8.2 to
master. The one material behaviour change is the `--wait` gate.

## Herdr facts (IPC, session, environment; verified at v0.8.2 and master 3a822e81)

Read from `src/ipc.rs` and `src/session.rs`; both files are byte-identical
between v0.8.2 and 3a822e81. AY.1 deliverable 0 adds the v0.8.0 column.

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
  the Windows kill/reap correctness item (AY.2) is mandatory, not hygiene.
- ACL: Unix API socket is chmod 0600 (`src/server/socket_paths.rs:12`);
  on Windows `restrict_socket_permissions` is a no-op for the API pipe
  (`src/ipc.rs:342-345`); the same-user SDDL DACL exists only on the
  private remote-attach listener (`src/ipc.rs:143-168`). AY.3 must treat
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
  codes, never message text; AY.1 deliverable 0 confirms that for every
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

Open audit rows (answered by AY.1 deliverable 0, none of them gates a
design decision in this plan):

| Question | Needed by |
|---|---|
| CRLF vs LF on stdout/stderr JSON observed on a real Windows run | AY.2 evidence |
| Detached-child stdio and console behaviour of `herdr.exe` spawned by the daemon | AY.2 |
| Pipe ACL / impersonation model in practice (same-user vs any local user) | AY.3 boundary revision |

## Startup, environment and failure model (normative)

This section is the contract every AY sprint implements and every QA pass
checks. It applies to all three platforms.

### Configured or not

atm is **Herdr-configured** on a host when the daemon config enables the
herdr backend and the roster has at least one member on the Herdr
harness. Otherwise atm is **not Herdr-configured** and nothing in this
section applies: no Herdr traits are built, nothing Herdr-related is
installed, doctor prints one line "herdr: not configured".

### Installer (`daemon-switch`, service install)

- Herdr-configured: alongside the atm daemon service entry, the installer
  installs a **Herdr start-at-login entry** that runs `herdr server`
  (Herdr's headless entry point). macOS: a separate LaunchAgent with
  `RunAtLoad` and no `KeepAlive` (an attempt at login, not supervision;
  a duplicate exits 1 once and nothing loops when a GUI Herdr is already
  up). Linux: the equivalent user unit. Windows: a per-user logon task
  under the same account as the atm daemon. The atm entry is installed
  after the Herdr entry; launchd and Windows give no ordering guarantee,
  and none is needed (below).
- Not Herdr-configured: the installer installs and removes nothing for
  Herdr. Switching a host from configured to not configured removes the
  Herdr entry atm installed; atm never removes anything it did not
  install.
- Windows deployment is **per-user** (logon task in the user's session,
  same account as Herdr). A Windows service in session 0 or under a
  different account resolves a different default pipe and is out of
  scope; `daemon-switch` refuses that configuration with an actionable
  message rather than installing it.
- The installer never starts Herdr itself and never checks whether Herdr
  is running. Herdr owns its socket, session state and restart.

### Daemon startup

- The daemon builds the Herdr traits from configuration and roster data
  at composition time (the single `HerdrProcessInvoker::new` site in
  `atm-daemon-bootstrap/src/replacement_handler.rs` today), including the
  adapter registry for every supported Herdr epoch. No probe, no wait, no
  launch, and no version is chosen at startup: the adapter is selected
  per call from the live Herdr, so a Herdr upgrade while the daemon runs
  needs no daemon restart. Readiness does not depend on Herdr in any way.
- Normal case: Herdr started at login and owns the default session
  socket; the daemon starts under launchd afterwards in the same user
  session with no Herdr variables in its environment; the `herdr` client
  it spawns falls back to Herdr's default socket, which is the socket
  Herdr just created. Nothing to detect, nothing to resolve.
- Explicit configuration (optional, daemon config and roster):
  `herdr.binary_path` (absolute path or directory containing `herdr` /
  `herdr.exe`; default: PATH lookup by name as today) and `herdr.session`
  (Herdr session name; default: none). The session reaches the child as
  `HERDR_SESSION` exactly as HR-CORE-006 specifies today; when none is
  configured the child inherits the daemon's ambient environment
  unmodified. HR-CORE-006 is **retained unchanged**; the previous
  revision's deletion of it is withdrawn. atm never reads
  `HERDR_SESSION` or `HERDR_SOCKET_PATH` from its own environment to
  synthesize a choice (requirements.md:153-160).

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
- Doctor `herdr` section reports: configured or not; transport kind;
  binary resolution (path found, or not found with the search list and
  provenance "configured"/"PATH"); configured session (or "default");
  last probe result from one bounded `herdr agent list` executed by
  doctor itself (JSON result, or the Herdr error code and the endpoint
  Herdr's error names); the server `version` and `protocol` from
  Herdr's `ping` when the probe succeeds, with a "below
  HERDR_MINIMUM_VERSION" finding when applicable, and an explicit
  "CLI/server mismatch: restart Herdr" line when the probe fails with
  `protocol_mismatch` (the state `herdr update` now leaves behind on
  purpose); breaker state and last transition time. Doctor does **not**
  compute Herdr's socket or pipe path; it reports what Herdr says.
- Architecture and lifecycle tests (AY.1): (a) daemon starts and reaches
  ready with no `herdr` binary on PATH and none configured, and a tmux
  nudge and a hermes graft nudge still succeed in that state; (b) same
  with a fake `herdr` that always returns `server_not_running`: the
  breaker opens, doctor reports it, the daemon stays up; (c) no code path
  in atm-daemon-bootstrap or atm-http-runtime spawns `herdr server` or
  any long-lived Herdr child (grep guard in boundary_enforcement.rs); (d)
  `daemon-switch` in the not-configured case writes no Herdr entry.

### Herdr starts after atm has been running

No special handling exists or is needed; the plan states the three
mechanisms that make late start self-healing, and AY.1 tests them:

1. Every call spawns a fresh `herdr` client process (AY.3: a fresh
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

## Sprint AY.1: transport seam, portable fixtures, startup/failure model (size M, no Windows machine)

Branch `feature/ay1-herdr-transport-seam` off `integrate/phase-ay`.
Runs on macOS/Linux with all three CI lanes as the merge gate. Requires
no Windows machine and no live Herdr; nothing here can stall on FastPC4.

Blocking preconditions (recorded in the sprint doc with a date before
dispatch):

- P-A: `integrate/phase-ay` exists, cut from `integrate/phase-ax` after
  PR #1204 (AX.6, HR-CORE-010 notify) has merged into it (owner: fenix).
- P-B: this plan is approved by Rand (dated line in this file).

Deliverables:

0. **Herdr source audit** (`docs/atm-herdr/windows-process-audit.md`):
   one row per item (expected behaviour, Herdr file:line at v0.8.2,
   observed, verdict: no action / production fix / upstream request),
   folding in the facts sections of this plan and answering the open
   rows above where a macOS/Linux read suffices; Windows-observed columns
   are filled by AY.2. Includes one row per drift-table entry above and
   the v0.8.0 -> v0.8.2 per-command diff, in particular the
   `agent prompt --wait` semantics against HR-CORE-002/003 and the
   `HerdrError` mapping (codes only, never message text). Creates
   `docs/atm-herdr/herdr-versions.md` (rule 4) with rows for v0.8.0,
   v0.8.2 and the master commit the dev hosts run, and lands
   `HERDR_MINIMUM_VERSION` in `crates/atm-herdr` (rule 1). Replaces
   ADR-058's PROTOCOL_VERSION pin with the minimum-version rule and the
   compatibility statement below. Does not touch HR-CORE-003 (already
   correct).
1. **Transport seam.** New `crates/atm-herdr/src/transport.rs`:

   ```rust
   /// Explicit configuration, never inferred from the daemon's environment.
   pub struct HerdrClientConfig {
       pub binary_path: Option<PathBuf>,   // file or directory; None = PATH lookup by name
       pub session: Option<HerdrSession>,  // reaches the child as HERDR_SESSION (HR-CORE-006)
   }
   pub enum HerdrRequest {
       Prompt { agent: AgentName, text: NudgeText },
       Wait { agent: AgentName, until: Vec<HerdrAgentStatus>, timeout: Duration }, // --timeout <ms>
       Get { agent: AgentName },
       List,
       Notify { title: NotifyTitle, body: NotifyBody },  // argv fixed: --sound request (HR-CORE-010)
   }
   pub struct HerdrRawResponse { pub stdout_json: String, pub stderr_json: Option<String>, pub exit: HerdrExit }
   pub enum HerdrExit { Ok, ServerError, Usage, Other(i32) }
   #[async_trait]
   pub trait HerdrTransport: Send + Sync {
       fn kind(&self) -> HerdrTransportKind;            // Cli | Socket
       fn config(&self) -> &HerdrClientConfig;
       async fn execute(&self, request: &HerdrRequest, deadline: Deadline) -> Result<HerdrRawResponse, HerdrError>;
       async fn observed_version(&self) -> Result<HerdrVersion, HerdrError>; // `herdr --version` (Cli) / ping.version (Socket)
   }
   /// One implementation per Herdr behaviour epoch; all compiled in, selected per call.
   pub trait HerdrProtocolAdapter: Send + Sync {
       fn epoch_floor(&self) -> HerdrVersion;           // v0_8_0: 0.8.0, v0_8_3: first release after 8633a398
       fn encode(&self, request: &HerdrRequest) -> HerdrEncodedRequest;      // argv or NDJSON line
       fn decode_snapshot(&self, raw: &HerdrRawResponse) -> Result<AgentSnapshot, HerdrError>;
       fn decode_wait(&self, raw: &HerdrRawResponse) -> Result<WaitOutcome, HerdrError>; // epoch-specific --wait rules
   }
   pub struct HerdrAdapterRegistry { adapters: Vec<Box<dyn HerdrProtocolAdapter>> } // newest epoch whose floor <= version
   pub const HERDR_MINIMUM_VERSION: HerdrVersion = HerdrVersion::new(0, 8, 0);
   // HerdrError: the existing closed enum in lib.rs, unchanged
   // (AgentBlocked, AgentNotFound, AgentNotReady, AgentTargetAmbiguous, AgentNotRunning,
   //  AgentPromptStalled, ServerNotRunning, ProtocolMismatch, Timeout, InvalidAgentName,
   //  EmptyAgentPrompt, ServerUnavailable, InternalError, TimedOut, Unavailable{retry_after}, Advisory{code}).
   ```

   `HerdrRequest -> argv` (today's builders at lib.rs:631-657) and
   `HerdrRawResponse -> AgentSnapshot / HerdrError` (parsers at 675-770)
   move into the `v0_8_0` adapter unchanged (pure motion) and stay
   transport-independent; the `v0_8_3` adapter differs only in
   `decode_wait`. The io::Error to `HerdrError`
   translation lives in the transport. Notify has no sound field: the
   argv stays exactly `notification show <title> --body <body> --sound
   request` (HR-CORE-010, boundary note). Shape mirrors `Http1Acceptor`
   (atm-http-runtime/src/http1_server.rs:49-75).
2. **CLI transport.** `transport_cli.rs`: today's `run_command_with_binary`
   (lib.rs:555-625) and `session_environment` moved verbatim. Pure
   motion: the entire ADR-058 fixture suite, including
   `session_environment_is_only_present_for_an_explicit_session`
   (lib.rs:1235) and every atm-herdr check in
   `crates/atm-architecture/tests/boundary_enforcement.rs`, passes on
   macOS and Linux with zero fixture edits (zero-regression oracle).
   Binary resolution honours `binary_path` (file or directory) before
   PATH; missing binary is `ServerUnavailable` naming what was searched.
3. **Portable fixtures.** Test-only fake herdr Rust binary
   (`crates/atm-herdr/tests/support/fake_herdr/main.rs`, test-only
   `[[bin]]`, located via `CARGO_BIN_EXE_fake_herdr` so cargo supplies
   the `.exe` suffix) replaces /bin/sh, so all six process tests run on
   all three CI lanes with identical assertions. Modes: exit 0, exit 1,
   stderr JSON envelope (`server_not_running`, `agent_prompt_stalled`,
   `timeout`, `protocol_mismatch`), stdout JSON line, sleep past deadline,
   echo argv and HERDR_SESSION, and a per-version replay mode that serves
   the recorded responses from `herdr-versions.md` for each supported
   Herdr version (rule 4); the suite runs once per version. Byte-exact LF output
   via write_all, never a .cmd shim. Two new tests: success stdout parse
   through a real child; argv/HERDR_SESSION round trip. Parsers tolerate
   a trailing `\r`. HR-TEST-006 ("CI never depends on Herdr being
   installed") still holds. No-flaky rule: deterministic fake, injected
   deadlines, hard bounds, nothing may block forever.
4. **Startup/failure model.** Implements the normative section: one
   `HerdrCliTransport` construction site in `build_replacement_handler`
   fed by `HerdrClientConfig`, alongside the `HerdrAdapterRegistry` and
   `HerdrVersionResolver` (all epochs registered; selection per call, no
   restart on Herdr upgrade); `daemon-switch` Herdr start-at-login
   entry (install/remove, per-user only, refuse session-0/service on
   Windows); doctor `herdr` section as specified; lifecycle tests (a)-(d)
   as real-startup integration tests with negative proof, per the
   daemon-composed-feature rule.
5. **Docs.** Delete the three scope-out statements; add HR-PLAT-001
   (identical command set, error table and breaker semantics on all
   platforms; transport differences never surface as feature
   differences) and HR-LIFE-001 (daemon never depends on Herdr for
   startup or readiness; Herdr failures are per-call); amend ADR-058 D3
   (UDS on macOS/Linux, named pipe on Windows, cited from v0.8.2 and
   3a822e81) and its Herdr pin (`HERDR_MINIMUM_VERSION`) and
   record that the AI.11 named-pipe ban governs atm's own IPC listener,
   not a client of a third-party server (boundary_enforcement.rs:2002-2008
   scope); amend architecture.md, HR-TEST-006, the atm-herdr boundary
   TOML `[contracts]` notes (no io_owns change in AY.1); mark AQ2.6
   deliverable 5 superseded (do not delete history).

Acceptance: deliverables 0 through 5; windows-latest CI leg executes the
process-behaviour suite; official zero-regression benchmark run on the
hot path before merge (the diff should not touch it; prove it);
quality-mgr PASS with flaky-test-qa deployed. Merge gate 0/0/0 in scope
plus green CI.

## Sprint AY.2: Windows process correctness and live evidence (size M, FastPC4)

Branch `feature/ay2-windows-herdr-evidence` off `integrate/phase-ay`
after AY.1 merges. This is the only sprint that needs a Windows machine.

Blocking preconditions (recorded in the sprint doc with a date before
dispatch; AY.2 does not dispatch while any is unset):

- P-C: AY.1 merged into `integrate/phase-ay`.
- P-D: the FastPC4 Windows `atm-dev` team exists with Herdr v0.8.2
  installed via the official installer, the atm daemon and the Herdr
  start-at-login entry installed per-user by `daemon-switch`, and its
  parked reporter agent has delivered one round-trip report to rand-m4
  or rand-m5. Owner: Rand. Target date: set by Rand in this line when
  known. Until then AY.2 is blocked and the phase proceeds with AY.1 and
  AY.3 planning only.
- P-E: the Windows dev agent is named here (ATM identity on the FastPC4
  team, agent kind) by Rand, and the outbound dispatch path is stated
  (fenix sends j2 assignments to `<agent>@atm-dev.fastpc4`; if the
  cross-host link is down the reporter agent relays the assignment id
  and fenix re-sends when the link returns; the reporter's cadence
  report is the liveness signal).

Deliverables:

1. **Windows process correctness** inside `transport_cli.rs` only:
   `herdr.exe` resolution via `binary_path` (file or directory) or PATH
   on every spawn, never cached, with the alias directory as the
   documented default configuration; CREATE_NO_WINDOW; kill-then-reap verified to leave no orphan in
   `tasklist`; UTF-8 stdout handling. No `cfg(windows)` outside the
   transport module (architecture test).
2. **Per-user installer on Windows.** `daemon-switch` registers the atm
   daemon and the Herdr start-at-login entry as logon tasks in the user's
   session, refuses service/session-0 installs, and doctor reports the
   account and session in which each runs.
3. **Audit completion.** Windows-observed columns of
   `docs/atm-herdr/windows-process-audit.md` filled from the live run
   (CRLF observation, detached stdio, console flash, pipe name Herdr
   reports).
4. **Live Windows evidence** (acceptance gate; reuses the AX.7 evidence
   table format, owned entirely by AY): `atm doctor` herdr section;
   prompt/wait/get/list/notify (HR-CORE-010) round trips with observed
   argv and JSON; end-to-end nudge from another host with timestamps at
   both ends; transport-boundary structured logs; negative cases live
   (Herdr stopped: daemon stays up, tmux/hermes paths unaffected,
   breaker opens and recovers when Herdr returns, agent not found, agent
   blocked, slow command hits the 5 s cap with no orphan); late-start
   case (start daemon first, Herdr second, backlog drains); nudge latency
   sample; explicit confirmation no console window flashes. Evidence is
   captured by the FastPC4 team and committed byte-for-byte; agents
   never author evidence records.

Acceptance: deliverables 1 through 4; quality-mgr PASS with flaky-test-qa
deployed; the Windows CI job stays the merge gate and FastPC4 evidence is
the release-readiness gate for Windows Herdr parity.

## Sprint AY.3: direct socket/pipe client (size L)

Prerequisites: AY.1 merged; the boundary revision below approved by
boundary-guard before code (io_owns drops `tokio_process_spawn` and
`herdr_argv_construction`, adds `herdr_local_socket_client`; the Windows
API pipe has no DACL, so the revision records that any local user can
reach it). AY.2 evidence is required before AY.3's cutover (deliverable
5), not before its code.

Deliverables:

1. **Socket transport.** `crates/atm-herdr/src/transport_socket.rs`
   implementing `HerdrTransport` with no child process:
   `tokio::net::UnixStream` on unix, `tokio::net::windows::named_pipe`
   on Windows (tokio already allowed in atm-herdr; no new crate edge).
   The endpoint is the configured session's socket as Herdr's own client
   would resolve it, computed by one function in the transport module
   whose output doctor reports; when no session is configured the
   default is used. This is the one place atm derives a Herdr path, and
   it exists only because a socket client cannot delegate resolution to
   the `herdr` binary.
2. **Protocol.** ADR-058 D3 in our code: fresh connection per command,
   ping, minimum-version check on `ping.version` (deliverable 3), one
   NDJSON request line, one response line, explicit bounded read deadline
   (Herdr's own client has none), unknown response fields tolerated.
   Request ids `atm:agent:<cmd>`. `agent.prompt` with `wait` handles both
   the v0.8.x and the post-v0.8.2 gate semantics (rule 3).
3. **Compatibility matrix.** The `herdr-versions.md` matrix extended to
   the socket transport: every version at or above
   `HERDR_MINIMUM_VERSION`, keyed on the `ping` result (`version`,
   `capabilities`), with a fast actionable failure below the minimum and
   a doctor row. PROTOCOL_VERSION is not part of the check: the NDJSON
   server does not enforce it (`src/api/server.rs:333-372` only echoes it).
4. **Equivalence.** The whole ADR-058 fixture suite plus AY.1's fake
   binary tests run through both transports with identical assertions;
   a fake Herdr socket/pipe server fixture (test-only) mirrors the fake
   binary's modes and per-version replay, including the failure model
   (server absent, late start).
5. **Cutover.** Composition-root default flips to the socket transport;
   the CLI transport is retained as a documented, explicit config
   fallback (never silent) for exactly one atm minor version after the
   version that ships the cutover, then removed.
6. **Live evidence** on macOS and Windows, same set as AY.2 deliverable 4.

Acceptance: all six deliverables; boundary revision merged; AY.1
zero-regression oracle green on both transports; official benchmark run
on the hot path; quality-mgr PASS with flaky-test-qa and boundary-guard
deployed.

## Phase AY exit gate (AY.3 disposition)

Phase AY is not complete until a dated decision on AY.3 is recorded here
and in `docs/project-plan.md`, chosen by Rand from exactly these:

- **Ship**: AY.3 merged, acceptance above met.
- **Defer**: AY.3 deferred to a named phase, ADR-058 D3 amended to say
  the CLI transport is the supported design until then.
- **Cancel**: AY.3 dropped, ADR-058 D3 rewritten to make the CLI
  transport the permanent design, the AY.3 sections here marked
  superseded.

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
one (child env `HERDR_SESSION`, HR-CORE-006, retained); the AY.3 socket
transport derives the endpoint from that same configured session.

| Requirement | Operation | Today's argv |
|---|---|---|
| HR-CORE-002 | prompt | `herdr agent prompt <AgentName> <text>` (rendered nudge template only) |
| HR-CORE-003 | wait | `herdr agent wait <AgentName> [--until <status>]... --timeout <ms>` |
| HR-CORE-004 | get | `herdr agent get <AgentName>` (BreakerPolicy::Bypass allowed) |
| HR-CORE-005 | list | `herdr agent list` |
| HR-CORE-010 (AX.6) | notify | `herdr notification show <title> --body <body> --sound request`; mail body forbidden (HR-SAFE-003); sound fixed |

Responses: HR-CORE-007 AgentSnapshot from `result.agent`; HR-CORE-008
closed HerdrError enum keyed by Herdr error codes (unchanged by AY);
HR-CORE-009 and HR-SAFE-005..007 breaker on infrastructure-class failures
(connect/IO class on a socket). HR-SAFE-001 no send-keys fallback;
HR-SAFE-002 every call bounded; HR-SAFE-004 no durable Herdr state in
atm-herdr.

Boundary: `boundaries/atm-herdr/herdr-process-adapter.toml` io_owns
`tokio_process_spawn` and `herdr_argv_construction`. AY.1 changes neither
(pure motion inside the crate). AY.3 replaces both and requires a new
boundary revision, not an exception. forbidden_edges stay (no
atm-core/atm-storage/rusqlite into atm-herdr; no atm-herdr into
daemon/runtime crates).

## PROTOCOL_VERSION and compatibility

Corrected on 2026-09-05 from the drift review. `PROTOCOL_VERSION`
(`src/protocol/wire.rs:20`, 22 at 3a822e81) versions Herdr's bincode
client socket (`herdr-client.sock`), not the NDJSON API. The NDJSON server
never checks it; it only echoes it in `ping` (`src/api/server.rs:333-372`).
What rejects a command is the `herdr` CLI itself, which compares its
compiled constant with `ping.protocol` before every request
(`src/cli.rs:762-796`, `src/cli/protocol_guard.rs:16-44`) and exits 1 with
`protocol_mismatch` on any inequality.

Consequences for atm:

- Under the CLI transport (AY.1, AY.2) the check is Herdr's own and
  passes whenever the CLI binary and the running server come from the
  same install. After cc88b3b8, `herdr update` keeps a compatible old
  server running and `herdr status` no longer flags PROTOCOL_VERSION
  drift as needing a restart, so "new CLI, old server, every atm nudge
  fails with `protocol_mismatch`" is a normal post-update state. atm maps
  the code (existing `HerdrError::ProtocolMismatch`), the breaker opens,
  and doctor names the remedy (restart Herdr). No atm release is
  involved.
- Under the socket transport (AY.3) atm never sees `protocol_mismatch`.
  Compatibility becomes atm's own responsibility and is keyed on
  `HERDR_MINIMUM_VERSION` and the `ping` result (`version`,
  `capabilities`), never on PROTOCOL_VERSION. This is why AY.3's blocking
  prerequisite is the conformance matrix, not the socket code.
- ADR-058's pin of `d79fd746` / 21 is replaced by `HERDR_MINIMUM_VERSION`
  and the per-release table in `herdr-versions.md` (AY.1 deliverable 0).

## Ordering with phase AX

Status 2026-09-05 (fenix@rand-m5): AX.1 through AX.5 are merged into
integrate/phase-ax; AX.6 (PR #1204) is in its second fix pass; AX.7
(PR #1206, stacked on AX.6) is the macOS live-proof sprint and keeps its
own accepted scope (Windows runs are out of AX.7 scope and stay so; AY.2
owns Windows evidence). One PR integrate/phase-ax -> develop follows
AX.7 and the phase-ending review.

Decision (Rand, 2026-09-05): `integrate/phase-ay` is cut from
`integrate/phase-ax` after PR #1204 has merged into it, or from develop if
phase-ax has already merged to develop at that moment. Either way AY.1
takes the AX contract, including HR-CORE-010 notify, as its baseline.
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
3. Orphaned herdr.exe after timeout: explicit live-verified test (AY.2).
4. Console flash per nudge: CREATE_NO_WINDOW plus visual confirmation
   (AY.2).
5. Merge collision with AX: AY branches from phase-ax after AX.6 and
   merges forward after the phase-ax PR lands (P-A).
6. Hot-path regression: official benchmark run before merge (AY.1, AY.3).
7. AY.3 quietly dropped again: the exit gate requires a dated
   Ship/Defer/Cancel decision line before the phase PR.
8. FastPC4 not ready: only AY.2 depends on it (P-D/P-E). AY.1 lands the
   transport seam, fixtures, startup/failure model and docs without a
   Windows machine; AY.3 code can proceed; the phase cannot close with
   Windows parity claimed until AY.2 evidence exists.
9. Two launchers race at login (atm-installed Herdr entry vs a GUI Herdr
   the user opens): the entry has no KeepAlive, the loser exits 1 once,
   the GUI attaches as a client to whichever server won, and the daemon
   is indifferent to which one it was. Doctor shows the probe result.
10. Herdr not running for a long stretch: nudges on the Herdr harness
    fail fast under the open breaker while mail still queues; the
    reminder cycle drains the backlog when Herdr returns. Operators see
    it in doctor, not in a dead daemon.

## Windows machine and team (Rand, 2026-09-05)

- Windows testing runs on **FastPC4**. Rand will set up a Windows
  `atm-dev` team on FastPC4; AY.2 dev, the Windows-observed audit rows
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
  to the named dev agent (P-E); when the link is down, the reporter's
  next report carries the last assignment id it saw and fenix re-sends.
- The Windows CI job stays the merge gate; FastPC4 evidence is the
  release-readiness gate for Windows Herdr parity.
- cwin remains routed through Rand; nothing is dispatched to FastPC4
  directly from this team until the FastPC4 team exists.

## Review ledger

- qa-pr1214-plan r1 (73f79aa4f, FAIL) and r2 (04685bfbf, PASS on the
  blocking contradiction; Important "W1 bundles eight closure types"
  open): addressed here by the AY.1/AY.2/AY.3 split, the exit-gate
  decision line, and P-D/P-E.
- critical-plan-reviewer AY-PLAN-HOSTILE-R1 on 9e7577143 (FAIL, 5
  blocking / 7 important / 2 minor): AYP-001 (HR-CORE-006 retained),
  AYP-002 (daemon launch path removed; installer entry instead),
  AYP-003 (HerdrError unchanged, no NotRunning), AYP-004 (notify sound
  fixed), AYP-005/006 (P-D/P-E with owner, risk 8, outbound dispatch),
  AYP-007 (AX.7 widening dropped), AYP-008 (open-rows table reduced to
  genuinely open rows), AYP-009 (single v0.8.2 pin, SHAs reconciled),
  AYP-010 (HR-CORE-003 claim withdrawn, table fixed), AYP-011 (grep-able
  decision line), AYP-012 (fallback retention unit), AYP-M1 (risks
  renumbered), AYP-M2 (citation line fixed).
- Herdr drift review (two background agents, v0.8.2..3a822e81,
  2026-09-05): findings in "Herdr minimum version, drift and schema
  management"; PROTOCOL_VERSION section corrected; prompt-wait semantics
  and Windows PATH change added to AY.1/AY.2 deliverables.
- Rand, 2026-09-05 (via fenix@rand-m4, randlee/atm-core#1217): Herdr IPC
  follows the HTTP schema-management pattern; support every Herdr version
  at or above `HERDR_MINIMUM_VERSION` (0.8.0 today); raising it needs
  Rand's recorded sign-off; new capability adopted additively; a
  conformance fixture per supported version; `schema-reviewer` (PR #1218)
  checks this at plan and phase-end review. Landed in AY.1 (rules 1-5),
  extended by AY.3.
- Rand, 2026-09-05 (questions on API change management): answered by
  "Mechanism: versioned protocol adapters, selected per call": registry
  of epoch adapters injected at composition, version observed per call,
  no daemon restart on Herdr upgrade.
- Rand, 2026-09-05: plan not approved yet; startup and environment
  concerns; atm must not own Herdr; no hard gate; launchd installs Herdr
  entry only when configured, mirrored in daemon-switch; late Herdr
  start must be handled; daemon must work and diagnose when Herdr
  cannot start. All recorded in "Design rulings" and the normative
  section.
