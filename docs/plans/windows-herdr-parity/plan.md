---
phase: AY
title: Phase AY, Windows Herdr parity and UDS/named-pipe transport (same command set, same feature set on macOS, Linux, Windows)
integration_branch: develop (plan) / integrate/phase-ay (dev)
status: draft
owner: fenix
authored: 2026-09-05
supersedes:
  - docs/adr/ADR-058-herdr-local-steer-backend-contract.md:576 ("Herdr on Windows ... out of AQ scope")
  - docs/atm-herdr/requirements.md:368-370 (Windows non-goal)
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
3. Untested Windows spawn concerns: `herdr.exe` discovery (PATHEXT,
   configured path), console-window flash on every nudge
   (CREATE_NO_WINDOW), kill/reap on timeout (orphaned herdr.exe), stdout
   encoding.
4. No live Windows Herdr deployment or evidence of any kind.

## Facts still to be read from Herdr source before dev starts

Herdr's full source is available at `/Volumes/Extreme Pro/github/herdr`
(confirmed by Rand 2026-09-05). Facts read from that checkout on
2026-09-05, replacing the ADR-058 pin (`/Users/randlee/Documents/github/herdr`
at d79fd746, PROTOCOL_VERSION 21):

- Checkout HEAD 6c52aad5 (`git describe`
  preview-2026-08-31-b1ff4582e968-43-g6c52aad5), 43 commits past the
  newest release tag. Tags present: v0.8.0 (2026-08-03) and v0.8.2
  (2026-08-19).
- Target release: **v0.8.2**, PROTOCOL_VERSION 20 (`src/protocol/wire.rs:16`).
  Rand's rule: target 0.8.0+ unless a needed change landed after 0.8.0.
  One did: v0.8.0 is Windows beta (PROTOCOL_VERSION 19, no Windows job
  in `release.yml`); commit 9fac5172 "feat: make Windows generally
  available" (2026-08-18) adds `x86_64-pc-windows-msvc` to `release.yml`,
  plus `windows-arm64.yml`, and ships in v0.8.2. So `herdr.exe` is a
  released artifact from v0.8.2 onward; AY.1 is not an upstream request.
- The ADR pin (d79fd746, PROTOCOL_VERSION 21) is a post-release preview
  commit, not a release, and HEAD is already at PROTOCOL_VERSION 22
  (99c23cd1, 2026-09-01). The installed rand-m4 binary reported 20, which
  matches v0.8.2. Fixture and doctor expectations pin to v0.8.2 /
  PROTOCOL_VERSION 20; the audit must diff `v0.8.2..HEAD` on the paths
  below only to note pending drift (e.g. cc88b3b8 "add stable client
  endpoint compatibility" 2026-09-02), never to adopt preview behaviour.
- Windows IPC lives in `src/ipc.rs` (`cfg(windows)` named-pipe accept,
  same-user ACL test, closed-peer detection at 238-290) and
  `src/api/client.rs:117`; endpoint env vars are `HERDR_SOCKET_PATH`
  (`src/api/mod.rs:20`) and `HERDR_SESSION` (`src/session.rs:10`).
  Platform branches in the CLI itself are limited to `src/cli/agent.rs:658-700`
  and `src/main.rs:810-820`; CLI output uses `println!`/`eprintln!`
  (`src/cli.rs`), so stdout is LF on both platforms.
- Post-0.8.0 commits relevant to the audit: 2863b715 "support remote
  attach to unix hosts" (Windows), e3c3d443 "exit quietly when output
  pipes close" (CLI, in v0.8.2).

Required reads, cited in the sprint doc before dispatch:

Step 0 of AY.1 (blocking, before any code): check out tag v0.8.2 in the
Herdr checkout above (the default HEAD is a preview), record that in
the sprint doc, and note drift from the ADR pin. Audit output is a new `docs/atm-herdr/windows-process-audit.md`,
one row per item (expected behaviour, Herdr file:line consulted, observed
on Windows, verdict: no action / production fix / upstream request).
Unanswerable rows escalate upstream; nothing is inferred.

Herdr files to read: `src/api/mod.rs:20` and `src/session.rs:96-180`
(endpoint and session resolution, Windows equivalents, pipe naming);
`src/api/client.rs:55-61` and `src/protocol/wire.rs:16` (transport,
PROTOCOL_VERSION on the Windows build); `src/cli.rs:738-844`,
`src/main.rs:568-580`, `src/cli/protocol_guard.rs`,
`src/cli/server_not_running.rs`, `src/cli/agent.rs:506-841` (exit codes,
stdout/stderr shape, connect failure); Cargo.toml and the release
workflow (does `herdr.exe` build and ship at all); plus a grep for
`cfg(windows)`, `named_pipe`, `NamedPipe`.

| Question | Needed by |
|---|---|
| Does `herdr.exe` build and ship for Windows at all | Answered: yes, since v0.8.2 (`release.yml`, 9fac5172) |
| CRLF vs LF on stdout/stderr JSON; exit codes and `server_not_running` identical on Windows | W1 |
| Does `herdr.exe` resolve the pipe from `HERDR_SESSION` the same way it resolves the socket path on Unix? (`src/session.rs`, `src/api/mod.rs`, `src/ipc.rs`) | W1 |
| Windows pipe name convention | W1 (doctor reporting), W2 |
| Framing on the pipe (same NDJSON as the socket?) and PROTOCOL_VERSION behaviour | W2 |
| Pipe ACL / impersonation model | W2 |

## Facts read from Herdr v0.8.2 (34ba52cc, 2026-09-05, IPC and session)

Read by an Explore pass at tag v0.8.2; `src/ipc.rs` and `src/session.rs`
have zero commits after v0.8.2, so these hold at HEAD too.

- Endpoint resolution order: explicit `--session` > `HERDR_SOCKET_PATH`
  > `HERDR_SESSION` > default (`src/session.rs:173-181`; env constants
  `src/api/mod.rs:20`, `src/session.rs:10-11`). Default path is
  `<config_dir>/herdr.sock`, named session
  `<config_dir>/sessions/<name>/herdr.sock` (`src/session.rs:161-171`);
  `config_dir` on Windows is `%APPDATA%\herdr` (`src/config/io.rs:30-59`),
  `herdr-dev` in debug builds. Session names: ASCII alnum plus `. _ -`,
  max 64 bytes (`src/session.rs:13,425-446`).
- Windows pipe name: the resolved path string is used verbatim as an
  `interprocess` namespaced name (`src/ipc.rs:44-51`), so the endpoint is
  `\\.\pipe\C:\Users\<user>\AppData\Roaming\herdr\herdr.sock` (or the
  `sessions\<name>\` variant, or `\\.\pipe\<HERDR_SOCKET_PATH>`). There is
  no short session-derived pipe name. The `.sock` path still exists on
  disk on Windows as a marker file holding `<pid>:<nanos>`
  (`src/ipc.rs:76,326-333`); identity checks use marker contents there
  and dev/ino on Unix (`src/ipc.rs:287-303`). Doctor reporting for W1
  prints this derived name.
- Transport: `interprocess` 2.4.2 local sockets, named pipe on Windows
  and UDS on Unix (`src/ipc.rs:10-11,137,247`). Framing is identical: one
  NDJSON request line, one JSON response line (`src/api/client.rs:158-174`);
  server caps the request at 1 MiB with a 5 s initial deadline
  (`src/api/server.rs:28-32,510-554`). PROTOCOL_VERSION 20, checked by
  `src/cli/protocol_guard.rs:16-43` (`protocol_mismatch`).
- Timeouts: the default CLI request path sets no read/write timeout
  (`src/api/client.rs:55-61`), and on Windows an `Unsupported` timeout
  failure is swallowed (`src/api/client.rs:115-120`). So atm's own
  process deadline plus kill-and-reap is the only bound on a hung
  `herdr.exe`; W1's Windows kill/reap correctness item is mandatory, not
  hygiene.
- ACL: Unix API socket is chmod 0600 (`src/server/socket_paths.rs:12`);
  on Windows `restrict_socket_permissions` is a no-op for the API pipe
  (`src/ipc.rs:342-345`); the same-user SDDL DACL exists only on the
  private remote-attach listener (`src/ipc.rs:143-168`). W2 must treat
  the Windows API pipe as reachable by any local user and record that in
  the boundary revision; W1 doctor should report it.
- Child env: every pane gets `HERDR_SOCKET_PATH`, `HERDR_BIN_PATH`,
  `HERDR_PANE_ID`, `HERDR_TAB_ID`, `HERDR_WORKSPACE_ID`
  (`src/integration/env.rs:28-33`); `HERDR_SESSION` is NOT propagated to
  panes, and `HERDR_SOCKET_PATH` outranks `HERDR_SESSION`
  (`src/session.rs:82-83`). Also `HERDR_ENV=1` blocks nested launches
  (`src/main.rs:478-482`).
- Consequence for atm (Rand, 2026-09-05): the atm daemon is a singleton
  under launchd / a Windows service, so it never inherits a pane's
  `HERDR_SOCKET_PATH`; it must own the Herdr endpoint itself. Today
  `session_environment` only sets `HERDR_SESSION` on the child
  (`crates/atm-herdr/src/lib.rs:627-629`) and works because the launchd
  environment is clean. W1 makes this explicit: the daemon resolves the
  endpoint from its configured `HerdrSession` (or an optional configured
  socket path) using Herdr's own rules above, sets `HERDR_SOCKET_PATH`
  and `HERDR_SESSION` on every child, and removes any inherited
  `HERDR_SOCKET_PATH`/`HERDR_CLIENT_SOCKET_PATH`/`HERDR_ENV`. On Windows
  the same path value is what Herdr maps to `\\.\pipe\<path>`, so one
  code path serves both platforms. Doctor reports the resolved endpoint.
  The fixture covers a polluted parent env (pane-launched daemon for
  dev/dogfood) and a clean one. This is also the endpoint field W2's
  direct client reuses.

## Facts read from Herdr v0.8.2 (CLI parity)

- Exit codes are platform-independent code: 0 success (JSON on stdout),
  1 server error, `server_not_running` or `protocol_mismatch` (JSON on
  stderr), 2 usage (`src/cli.rs:738-746`, `src/main.rs:559-570`,
  `src/cli/agent.rs:762-816`). Dead-socket classification is
  ErrorKind-only and commented as deliberately transport-neutral
  (`src/cli.rs:816-824`). `herdr status` exits 0 even with no server, so
  doctor must probe with `agent list`/ping, not `status`.
- Output is single-line compact JSON, LF only (`println!`, no CRLF
  handling anywhere under `src/cli*`): `{"id":"cli:agent:<cmd>","result":{"type":...}}`
  with `agent_list`/`agent_info`/`agent_prompted`, error
  `{"id","error":{"code","message"}}` (`src/api/schema/response.rs:30-109`).
  atm's parsers may still tolerate a trailing `\r` defensively.
- Argv is hand-rolled positional: `agent prompt <target> <text> [--wait]
  [--until S]... [--timeout MS]`, `agent wait <target> [--until S]...
  [--timeout MS]`, `agent get <target>`, `agent list` (no args). `--timeout`
  is milliseconds; omitted means wait indefinitely; `--flag=value` is not
  accepted for agent commands (`src/cli/agent.rs:424-545,757-826`). atm's
  builders (`crates/atm-herdr/src/lib.rs:631-657`) match this exactly,
  including milliseconds; the phase-ax contract text saying `--timeout
  <secs>` is a doc slip to correct in `docs/atm-herdr/requirements.md`.
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
  `%USERPROFILE%\.herdr\packages\standalone\releases` with
  `%LOCALAPPDATA%\Programs\Herdr\bin` as a junction on the user PATH.
  W1's configured-binary-path option must accept a directory-resident
  exe, never copy `herdr.exe` alone, and PATH lookup needs no PATHEXT
  work (`herdr.exe` is a real exe, not a `.cmd` shim). ARM64 Windows runs
  the x86_64 build under emulation. No code signing: SmartScreen prompts
  are expected on first run, which matters for the FastPC4 setup.

- Post-0.8.2 drift on the API paths (not adopted): cc88b3b8 stable
  client endpoint handshake, 207be3c7 client-side shell rendering,
  7916be16/8633a398 prompt-delay and wait changes, 20a500a7 lifecycle
  subscription start, and two Windows key-identity fixes; several bump
  PROTOCOL_VERSION.

## Sprint W1 (AY.1): parity via the CLI transport, proven on Windows (size M)

Sprint AY.1, branch `feature/ay1-windows-herdr-cli-parity` off
`integrate/phase-ay` (off develop). Dev and live validation
run on a Windows machine inside a Herdr session; the dev agent is itself
a Herdr-backed roster member on that box.

Blocking preconditions (no dispatch until all three are recorded in the
sprint doc with a date):

- P-A: `integrate/phase-ay` exists, cut from `integrate/phase-ax` (or
  from develop if phase-ax has already merged; see Ordering with phase
  AX).
- P-B: the FastPC4 Windows `atm-dev` team exists, has Herdr v0.8.2
  installed via the official installer, and its parked reporter agent
  has delivered one round-trip report to rand-m4 or M5 (see Windows
  machine and team). Deliverable 7 cannot be produced without it.
- P-C: deliverable 0 (the audit) is merged or in the same PR.

Deliverables:

0. **Herdr source audit** (`docs/atm-herdr/windows-process-audit.md`):
   the step-0 table described above, completed at tag v0.8.2, with the
   remaining W1 rows answered (pipe from `HERDR_SESSION`, CRLF, exit
   codes) and the facts sections of this plan folded in. Also corrects
   `docs/atm-herdr/requirements.md` HR-CORE-003 to milliseconds and
   ADR-058's PROTOCOL_VERSION pin to 20 / v0.8.2.

1. **Transport seam.** New `crates/atm-herdr/src/transport.rs`:

   ```rust
   pub struct HerdrEndpoint { pub session: HerdrSession, pub socket_path: PathBuf }
   pub enum HerdrRequest {
       Prompt { agent: AgentName, text: NudgeText },
       Wait { agent: AgentName, until: Vec<HerdrAgentStatus>, timeout: Duration },
       Get { agent: AgentName },
       List,
       Notify { title: NotifyTitle, body: NotifyBody, sound: NotifySound },
   }
   pub struct HerdrRawResponse { pub stdout_json: String, pub stderr_json: Option<String>, pub exit: HerdrExit }
   pub enum HerdrExit { Ok, ServerError, Usage, Other(i32) }
   #[async_trait]
   pub trait HerdrTransport: Send + Sync {
       fn kind(&self) -> HerdrTransportKind;            // Cli | Socket
       fn endpoint(&self) -> &HerdrEndpoint;
       async fn execute(&self, request: &HerdrRequest, deadline: Deadline) -> Result<HerdrRawResponse, HerdrError>;
   }
   ```

   `HerdrRequest -> argv` (today's builders at lib.rs:631-657) and
   `HerdrRawResponse -> AgentSnapshot / HerdrError` (parsers at
   675-770) stay in lib.rs, transport-independent. The single io::Error
   to `HerdrError` translation lives in the transport. Shape mirrors
   `Http1Acceptor` (atm-http-runtime/src/http1_server.rs:49-75): one
   trait, all policy generic over it.
2. **CLI transport.** `transport_cli.rs`: today's `run_command_with_binary`
   (lib.rs:555-625) moved verbatim. This commit is pure motion and must
   pass the entire ADR-058 fixture suite on macOS and Linux with zero
   fixture edits (zero-regression oracle).
3. **Windows process correctness** inside `transport_cli.rs` only:
   `herdr.exe` resolution honouring PATHEXT plus a configured absolute
   path override (never CWD-relative); CREATE_NO_WINDOW; kill-then-reap
   verified to leave no orphan; UTF-8 stdout handling. Missing binary is
   `ServerUnavailable` with a cause naming what was searched.
4. **Portable fixtures.** Test-only fake herdr Rust binary
   (`crates/atm-herdr/tests/support/fake_herdr/main.rs`, test-only
   `[[bin]]`, located via `CARGO_BIN_EXE_fake_herdr` so cargo supplies
   the `.exe` suffix) replaces /bin/sh, so all six process tests run on
   all three CI lanes with identical assertions. Modes: exit 0, exit 1,
   stderr JSON envelope, stdout JSON line, sleep past deadline, echo
   argv and HERDR_SESSION. Byte-exact LF output via write_all, never a
   .cmd shim, so the fixture cannot introduce the CRLF the audit is
   detecting. Two new tests: success stdout parse through a real child,
   and argv/HERDR_SESSION round trip through CreateProcess. Parsers
   tolerate a trailing `\r` regardless of the audit result. HR-TEST-006 ("CI never depends on Herdr being
   installed") still holds. No-flaky rule: deterministic fake, injected
   deadlines, hard bounds, nothing may block forever.
5. **Composition, endpoint ownership, launch verification, doctor.**
   One `HerdrCliTransport` construction site in
   `build_replacement_handler`. The daemon owns the endpoint (Rand,
   2026-09-05): `HerdrEndpoint` is resolved once at startup from the
   configured `HerdrSession` (optional explicit `socket_path` override)
   using Herdr's own rules; every child gets `HERDR_SOCKET_PATH` and
   `HERDR_SESSION` set explicitly and inherited
   `HERDR_SOCKET_PATH`/`HERDR_CLIENT_SOCKET_PATH`/`HERDR_ENV` removed.
   Herdr runs one server per user per session on a machine, so the
   daemon binds to exactly one session.

   Launch contract (Rand, 2026-09-05): the daemon and every atm agent
   run inside a Herdr pane and inherit Herdr's complete environment
   (socket path, binary path, pane/tab/workspace ids, everything else
   Herdr sets). What matters is that the daemon connects to the right
   socket, and Herdr's own client already guarantees that: with
   `HERDR_SOCKET_PATH` set (daemon in a pane) the `herdr` CLI uses it;
   with it unset (daemon under launchd or a Windows service) the CLI
   uses the default session socket in its config dir
   (src/session.rs). atm runs `herdr agent prompt|wait|get|list`
   client commands against the running server, as a user would, with
   its environment passed through unchanged.

   Herdr mode startup (Rand, 2026-09-05): the daemon runs one
   `herdr agent list`. If that reports `server_not_running`, the daemon
   starts Herdr itself with `herdr server`, Herdr's documented headless
   entry point for supervised setups (cli-reference.mdx "Server";
   src/main.rs:579 -> server::headless::run_server), detached from the
   daemon's stdio and lifetime, then waits a bounded time for
   `agent list` to answer. Still no answer, or any other probe error:
   exit with one diagnostic naming the socket and the error; the
   supervisor (launchd KeepAlive, Windows service recovery) retries.
   Herdr is expected to autostart at login ahead of the daemon, so this
   path is the power-cycle and crash fallback, not the normal case.
   Herdr keeps ownership of its socket, session state and restart;
   atm never stops it and never computes its paths. Audit rows for
   deliverable 0: does `herdr server` refuse a duplicate cleanly when
   a server is already up; what the GUI does when it launches over an
   existing headless server; stdio handling of the detached child on
   Windows. A process not launched by Herdr has no pane id and
   gets no nudges, by design. W2's socket transport connects to
   `HERDR_SOCKET_PATH` as given. Doctor herdr section reports transport
   kind, resolved binary, resolved endpoint (the `\\.\pipe\` form on
   Windows), and the launch-probe result. Architecture tests: single
   construction site; single endpoint-resolution site; no
   `cfg(unix|windows)` in atm-herdr outside the transport modules.
6. **Docs.** Delete the three scope-out statements; add parity
   requirement HR-PLAT-001 (identical command set, error table and
   breaker semantics on all platforms; transport differences never
   surface as feature differences); amend ADR-058 D3 (UDS on macOS/Linux,
   named pipe on Windows, cited from Herdr source) and record that the
   AI.11 named-pipe ban governs atm's own IPC listener, not a client of a
   third-party server (boundary_enforcement.rs:2002-2008 scope); amend
   architecture.md, HR-TEST-006, boundary TOML; mark AQ2.6 deliverable 5
   superseded (do not delete history).
7. **Live Windows evidence** (acceptance gate, AX.7 template):
   `atm doctor` herdr section including the launch probe;
   prompt/wait/get/list/notify (HR-CORE-010) round trips with observed
   argv and JSON; end-to-end nudge from another host with
   timestamps at both ends; transport-boundary structured logs; negative
   cases live (Herdr stopped, breaker opens/recovers, agent not found,
   agent blocked, slow command hits the 5s cap with no orphan in
   tasklist); nudge latency sample; explicit confirmation no console
   window flashes.

Acceptance: deliverables 0 through 7; windows-latest CI leg executes the
process-behaviour suite; official zero-regression benchmark run on the
hot path before merge (the diff should not touch it; prove it);
quality-mgr PASS with flaky-test-qa deployed.

## Sprint W2 (AY.2): direct socket/pipe client (size L)

Prerequisites: W1 merged; the boundary revision below approved by
boundary-guard before code (io_owns drops `tokio_process_spawn` and
`herdr_argv_construction`, adds `herdr_local_socket_client`; the
Windows API pipe has no DACL, so the revision records that any local
user can reach it).

Deliverables:

1. **Socket transport.** `crates/atm-herdr/src/transport_socket.rs`
   implementing `HerdrTransport` with no child process:
   `tokio::net::UnixStream` on unix, `tokio::net::windows::named_pipe`
   on Windows (tokio already allowed; no new crate edge). Endpoint comes
   from the same `HerdrEndpoint` W1 resolved.
2. **Protocol.** ADR-058 D3 in our code: fresh connection per command,
   ping, PROTOCOL_VERSION equality against a supported set, one NDJSON
   request line, one response line, explicit bounded read deadline
   (Herdr's own client has none). Request ids `atm:agent:<cmd>`.
3. **Compatibility matrix.** Supported Herdr releases and protocol
   numbers (starts at v0.8.2 / 20) with a fast actionable
   `protocol_mismatch` failure and a doctor row.
4. **Equivalence.** The whole ADR-058 fixture suite plus W1's fake
   binary tests run through both transports with identical assertions;
   a fake Herdr socket/pipe server fixture (test-only) mirrors the fake
   binary's modes.
5. **Cutover.** Composition-root default flips to the socket transport;
   CLI transport retained one release as a documented, explicit config
   fallback (never silent).
6. **Live evidence** on macOS and Windows, same set as W1 deliverable 7.

Acceptance: all six deliverables; boundary revision merged; W1
zero-regression oracle green on both transports; official benchmark run
on the hot path; quality-mgr PASS with flaky-test-qa and boundary-guard
deployed.

## Phase AY exit gate (W2 disposition)

Phase AY is not complete until a dated decision on W2 is recorded here
and in `docs/project-plan.md`, chosen by Rand from exactly these:

- **Ship**: W2 merged, acceptance above met.
- **Defer with re-approval**: W2 deferred to a named phase, ADR-058 D3
  amended to say the CLI transport is the supported design until then,
  and the deferral acknowledged by Rand in this file (date, initials).
- **Cancel**: W2 dropped, ADR-058 D3 rewritten to make the CLI transport
  the permanent design, the W2 sections here marked superseded.

quality-mgr's phase-ending gate must refuse the integrate/phase-ay to
develop PR while this section has no dated decision. This is the
forcing function that AQ2.6 and ADR-058 lacked (see AW-READY-W1).

## Request set the transport must carry (from phase AX contract, fenix@rand-m5 2026-09-05)

Source of truth: feature/ax6-lead-notification-doctor (PR #1204).
Every request carries the session (today child env `HERDR_SESSION`,
HR-CORE-006); a socket design needs an equivalent session field on every
request, still derived from roster data, never from the daemon's own env.

| Requirement | Operation | Today's argv |
|---|---|---|
| HR-CORE-002 | prompt | `herdr agent prompt <AgentName> <text>` (rendered nudge template only) |
| HR-CORE-003 | wait | `herdr agent wait <AgentName> [--until <status>]... --timeout <secs>` |
| HR-CORE-004 | get | `herdr agent get <AgentName>` (BreakerPolicy::Bypass allowed) |
| HR-CORE-005 | list | `herdr agent list` |
| HR-CORE-010 (AX.6) | notify | `herdr notification show <title> --body <body> --sound request`; mail body forbidden (HR-SAFE-003) |

Responses: HR-CORE-007 AgentSnapshot from `result.agent`; HR-CORE-008
closed HerdrError enum keyed by Herdr error codes; HR-CORE-009 and
HR-SAFE-005..007 breaker on infrastructure-class failures (connect/IO
class on a socket). HR-SAFE-001 no send-keys fallback; HR-SAFE-002 every
call bounded; HR-SAFE-004 no durable Herdr state in atm-herdr.

Boundary: `boundaries/atm-herdr/herdr-process-adapter.toml` io_owns
`tokio_process_spawn` and `herdr_argv_construction`. W2 replaces both and
requires a new boundary revision, not an exception. forbidden_edges stay
(no atm-core/atm-storage/rusqlite into atm-herdr; no atm-herdr into
daemon/runtime crates).

## PROTOCOL_VERSION pinning

Herdr's client compares PROTOCOL_VERSION with the server on every
command and any inequality is fatal (ADR-058:245-253). The supported
Herdr release for this plan is v0.8.2 (PROTOCOL_VERSION 20); ADR-058's
pin of 21 is a preview commit and must be corrected when the ADR is
revised. Under W1 that
risk stays inside Herdr's own binary (client and server are one
release); atm only maps the `protocol_mismatch` code, and doctor makes
it legible. W2 would transfer that liability to atm's release cadence,
so W2's blocking prerequisite is a compatibility strategy (negotiated
range or an explicit supported-version matrix with a fast actionable
failure), not the socket code.

## Ordering with phase AX

Status 2026-09-05 (fenix@rand-m5): AX.1 through AX.5 are merged into
integrate/phase-ax; AX.6 merges when its QA gate clears; then one PR
integrate/phase-ax -> develop follows AX.7 and the phase-ending review.

Decision (Rand, 2026-09-05): `integrate/phase-ay` is cut from
`integrate/phase-ax` if phase-ax has not yet merged to develop at the
moment AY starts; from develop otherwise. Either way AY.1 takes the AX
contract, including HR-CORE-010 notify, as its baseline. When cut from
phase-ax, `integrate/phase-ay` is retargeted to develop and merged
forward once the phase-ax PR lands, before the phase-ay PR opens. AX
does not wait on AY.1. AX.7's evidence scope is widened to a Windows
Herdr run once AY.1 lands.

## Risks

1. Herdr facts drift after v0.8.2: the audit pins v0.8.2 and the
   compatibility matrix (W2.3) makes later versions an explicit decision.
2. Transport extraction silently changes macOS/Linux behaviour: pure-motion
   commit, unedited fixtures.
3. Orphaned herdr.exe after timeout: explicit live-verified test.
4. Console flash per nudge: CREATE_NO_WINDOW plus visual confirmation.
5. Merge collision with AX: AY branches from phase-ax and merges
   forward after the phase-ax PR lands (P-A).
7. W2 quietly dropped again: the Phase AY exit gate requires a dated
   Ship/Defer/Cancel decision before the phase PR.
6. Hot-path regression: official benchmark run before merge.

## Windows machine and team (Rand, 2026-09-05)

- Windows testing runs on **FastPC4**. Rand will set up a Windows
  `atm-dev` team on FastPC4; sprint dev, the six fixture ports, and the
  AY.1 live-evidence deliverable execute there, inside a Herdr session.
- Cross-host messaging from FastPC4 has been unreliable because of VPN
  issues. Design: park one agent on the FastPC4 team whose only job is
  to report back regularly (on a fixed cadence and on every sprint
  event) to either this team (`atm-dev` on rand-m4) or the M5 team,
  whichever is reachable, so work continues even when a direct
  cross-host session is down. Reports carry SHA, test results, and
  evidence paths; the FastPC4 team is the source of truth for Windows
  evidence.
- The Windows CI job stays the merge gate; FastPC4 evidence is the
  release-readiness gate for Windows Herdr parity.
- cwin remains routed through Rand; nothing is dispatched to FastPC4
  directly from this team until the FastPC4 team exists.
