---
id: AY.8
phase: AY
sprint: AY.8
title: Direct Herdr socket and named-pipe transport without cutover
branch: feature/ay8-herdr-socket-transport
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay8-herdr-socket-transport
integration_branch: integrate/phase-ay
stack_parent: none
pr_target: integrate/phase-ay
target: integrate/phase-ay
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: socket
parallel_with: [AY.4, AY.5, AY.6, AY.7]
dependency_relations:
  - prerequisite: AY.1
    dependent: AY.8
    relation: must_follow
    rationale: ADR-058 D3 and herdr-versions.md are the protocol and compatibility authority consumed here.
  - prerequisite: AY.2
    dependent: AY.8
    relation: must_follow
    rationale: HerdrIo, HerdrClientConfig, replay recordings, and the portable fake-Herdr process must exist before the socket variant is added.
  - prerequisite: AY.3
    dependent: AY.8
    relation: must_follow
    rationale: both sprints edit boundary_enforcement.rs, so AY.3's long-lived-child guard must land before AY.8 adds the one AI.11 exemption.
  - prerequisite: AY.4
    dependent: AY.8
    relation: parallel_safe
    rationale: AY.4 owns breaker escalation and lifecycle closure while AY.8 owns the socket module, socket fixtures, enumerated boundary edits, and version-table NDJSON columns.
  - prerequisite: AY.5
    dependent: AY.8
    relation: parallel_safe
    rationale: AY.5 owns transactional Herdr entry management while AY.8 owns the direct socket implementation and its boundary/test artifacts.
  - prerequisite: AY.6
    dependent: AY.8
    relation: parallel_safe
    rationale: AY.6 owns restart and live-handoff coordination while AY.8 owns the direct socket implementation and its boundary/test artifacts.
  - prerequisite: AY.7
    dependent: AY.8
    relation: parallel_safe
    rationale: AY.7 owns transport_cli.rs Windows code, installer verification, and process-audit columns; AY.8 does not touch them.
  - prerequisite: AY.8
    dependent: AY.9
    relation: must_follow
    rationale: AY.9 alone selects and defaults to the production socket transport after this implementation merges.
---

# AY.8 — Direct Herdr socket and named-pipe transport without cutover

Implement the transport-independent NDJSON path on Unix-domain sockets and
Windows named pipes, with bounded one-request connections and fake-server
equivalence tests. The production composition root remains on CLI throughout
this sprint.

## Dispatch, parallelism, and PR topology

AY.8 is a multi-parent join. Dispatch it only after AY.1, AY.2, and AY.3 have
merged into `integrate/phase-ay` and the P-E boundary revision has been
approved. Create the branch from that integration head. It is not a child of
AY.3 and is not part of the implementation stack: `/gh-stack` stacks are linear and
cannot encode three prerequisites or a branch shared across stacks.

AY.8 runs in parallel with AY.4, AY.5, AY.6, and AY.7 because the exact changed-file
allowlist below does not intersect their owned files or public artifacts. Use
ordinary PR tooling with `pr_target: integrate/phase-ay`; do not run
`gh stack link` for AY.8, and never merge an unmerged parallel sibling into
it. Verify the standalone PR and its base with:

```sh
gh pr view feature/ay8-herdr-socket-transport --json headRefName,baseRefName,state
```

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or shape-only
completion fails the sprint.

- [ ] D1 — the P-E-approved revision to
  `boundaries/atm-herdr/herdr-process-adapter.toml` is the first commit. It adds
  only `herdr_local_socket_client` to `io_owns`; `io_forbidden` is unchanged and
  the CLI ownership keys remain while the fallback exists.
- [ ] D2 — `ai11_guarded_workspace_sources` in
  `crates/atm-architecture/tests/boundary_enforcement.rs` excludes exactly
  `crates/atm-herdr/src/transport_socket.rs`, with a rationale citing ADR-058 D3
  and AY.8. A pin test asserts that this is the only exemption. This is the
  second commit, after D1.
- [ ] D3 — add `crates/atm-herdr/src/transport_socket.rs` with crate-private
  `SocketIo` and `HerdrIo::Socket(SocketIo)`. Use
  `tokio::net::UnixStream` on Unix and
  `tokio::net::windows::named_pipe::ClientOptions` on Windows; add only Tokio's
  `net` feature if it is not already enabled. No process is spawned and no
  dependency on `interprocess` is added.
- [ ] D4 — add the pure endpoint resolver and public endpoint types in C1.
  Precedence is explicit `socket_path`, then the per-call session-derived path,
  then default. Environment values are captured once at composition and
  injected; transport code never reads ambient `XDG_CONFIG_HOME`, `APPDATA`, or
  `HOME` during a call.
- [ ] D5 — implement the C2 one-request NDJSON protocol: a fresh connection,
  exactly one request line, one response line, and close. No ping occurs on the
  nudge hot path. `server_info` opens a separate one-request connection for
  `ping`; version/minimum checks remain doctor-only.
- [ ] D6 — one absolute `RequestDeadline` covers connect, Windows
  `ERROR_PIPE_BUSY` retry, in-flight permit acquisition, write, flush, and read.
  At most 16 socket calls are in flight per invoker. A busy pipe waits 10 ms
  between attempts, clipped to the remaining absolute deadline, so it cannot
  hot-spin. The response line has a hard 1 MiB cap. Deadline exhaustion maps
  to existing `HerdrError::Timeout`; oversized or EOF-without-newline responses
  map to existing `HerdrError::InternalError` and log the observed byte count
  at the transport boundary. `SocketIo` spawns no task: dropping a pending call
  at runtime shutdown closes its stream and releases its permit by RAII.
- [ ] D7 — add a test-only fake Unix-socket/named-pipe server under
  `crates/atm-herdr/tests/support/fake_herdr_socket/`. It replays every AY.2
  version recording and covers absent endpoint, late start, no newline,
  oversized response, stalled connect/write/read, a rejected second request on
  one connection, pipe-busy-then-free with paused Tokio time on the Windows
  lane, and a 17-caller saturation case that proves the final caller waits for
  a permit or times out under its original deadline. Cancellation fixtures drop
  calls during permit wait, connect, write, and read and prove no permit,
  stream, retry timer, or task survives.
- [ ] D8 — run the entire ADR-058 adapter fixture suite through
  `HerdrProcessInvoker` over both `HerdrIo` variants with identical assertions.
  `docs/atm-herdr/herdr-versions.md` gains ping/request/response/error-code
  NDJSON columns for every release from 0.8.0, keyed on `ping.version` and
  capabilities rather than `PROTOCOL_VERSION`.
- [ ] D9 — amend the AY.2 public-item pin by adding exactly
  `herdr_api_endpoint`, `HerdrHostEnv`, and `HerdrEndpoint`; `SocketIo` remains
  `pub(crate)`.
- [ ] D10 — preserve the no-cutover guard: no change under
  `crates/atm-daemon-bootstrap`, and an architecture allowlist permits
  `HerdrIo::Socket(` construction only in `transport_socket.rs` test modules
  and `crates/atm-herdr/tests/`. AY.9 removes that temporary allowlist when it
  owns transport selection.

### Paths to delete

None.

## Code contracts

### C1 — endpoint resolution

```rust
/// Pure; performs no probe or I/O.
pub fn herdr_api_endpoint(
    cfg: &HerdrClientConfig,
    session: Option<&HerdrSession>,
    env: &HerdrHostEnv,
) -> HerdrEndpoint;

pub struct HerdrHostEnv {
    pub xdg_config_home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub platform: Platform,
}

pub enum HerdrEndpoint {
    UnixSocket(PathBuf),
    NamedPipe(String), // full \\.\pipe\... name
}

pub(crate) struct SocketIo {
    cfg: HerdrClientConfig,
    env: HerdrHostEnv,
    max_line_bytes: usize, // exactly 1 MiB
    in_flight: Arc<Semaphore>, // exactly 16 permits in production
    pipe_busy_retry_delay: Duration, // exactly 10 ms in production
}
```

The derived Unix path is `<config_dir>/herdr.sock` or
`<config_dir>/sessions/<name>/herdr.sock`. `config_dir` is
`$XDG_CONFIG_HOME/herdr` whenever set; otherwise `~/.config/herdr` on
macOS/Linux and `%APPDATA%\herdr` on Windows. Windows applies Herdr's
`GenericNamespaced` rule and prepends `\\.\pipe\` to the same path string.
Pinned byte-for-byte cases include Unix default/session/XDG override, Windows
default/session, and explicit `socket_path` on both platforms.

### C2 — connection and wire contract

```text
resolve endpoint from (config, per-call session, captured host env)
acquire one of 16 per-invoker permits under the caller's absolute RequestDeadline
connect under the caller's absolute RequestDeadline
on Windows ERROR_PIPE_BUSY, wait min(10 ms, deadline remaining), then retry
write one compact JSON request followed by one LF; flush
read through the first LF, rejecting more than 1 MiB or EOF before LF
decode HerdrEnvelope; close the connection
```

The permit is held through close and released on every success, error, and
cancellation path. Tests use paused Tokio time for busy-pipe retry and deadline
expiry; they do not sleep on wall-clock time. `SocketIo` owns no detached work:
all connect/retry/I/O futures are children of the admitted caller. During
Tokio/Axum drain, the existing runtime stops new admissions; admitted calls may
finish only within the smaller of their request deadline and runtime shutdown
deadline. Forced cancellation drops the stream, retry sleep, and semaphore
permit before the runtime join completes. The invoker and semaphore are dropped
after those tasks join, and restart constructs fresh state.

Canonical request examples, each exactly one NDJSON line:

```json
{"id":"atm:agent:prompt","method":"agent.prompt","params":{"target":"cipher","text":"read message m-42"}}
```

```json
{"id":"atm:agent:wait","method":"agent.wait","params":{"target":"cipher","until":["idle","done"],"timeout_ms":5000}}
```

```json
{"id":"atm:agent:ping","method":"ping","params":{}}
```

The response envelope is exactly one of these outer shapes; unknown nested
fields are tolerated:

```json
{"id":"atm:agent:ping","result":{"type":"pong","version":"0.8.2","protocol":20,"capabilities":{"live_handoff":true}}}
```

```json
{"id":"atm:agent:prompt","error":{"code":"agent_prompt_stalled","message":"<server text>"}}
```

Request IDs use `atm:agent:<command>`. `timeout` and
`agent_prompt_stalled` continue to map to the same existing atm outcome.

### C3 — exact changed-file allowlist

AY.8 may add or edit only:

- `crates/atm-herdr/src/transport_socket.rs`
- `crates/atm-herdr/tests/support/fake_herdr_socket/**`
- `crates/atm-herdr/src/transport.rs`
- `crates/atm-herdr/src/lib.rs` (one module declaration and exports for C1)
- `crates/atm-herdr/Cargo.toml` (Tokio `net` feature only if needed)
- `crates/atm-architecture/tests/boundary_enforcement.rs`
- the AY.2 public-item pin test under `crates/atm-architecture/tests/`
- `boundaries/atm-herdr/herdr-process-adapter.toml`
- `docs/atm-herdr/herdr-versions.md`

Any additional production path is a scope change requiring the sprint plan to
be amended and re-reviewed before implementation continues.

## Required work

1. Land the approved boundary record first and the pinned AI.11 exemption
   second; do not begin socket code until both diffs match P-E(b).
2. Implement endpoint resolution and the bounded one-request protocol as one
   transport boundary, then close every Unix-socket and named-pipe failure with
   the fake server on its owning CI lane.
3. Run the same adapter recordings through CLI and socket variants, update the
   compatibility ledger, and mechanically prove the production composition
   remains unchanged until AY.9.

## Acceptance criteria

1. D1 is the first commit and exactly matches the P-E ruling; D2 is the second
   commit and pins one exemption.
2. Endpoint-resolution byte fixtures pass for every C1 case, including the
   full Windows pipe string.
3. Both transports pass the same adapter equivalence suite on macOS, Linux,
   and Windows; the Windows lane exercises a real fake named-pipe server.
4. Every deadline, line-bound, one-request, absent-endpoint, late-start,
   pipe-busy backoff, and 16-permit saturation case in D6/D7 is deterministic
   and passes; cancellation at permit wait/connect/write/read releases every
   resource without a detached task.
5. `git diff <parent>..HEAD -- crates/atm-daemon-bootstrap` is empty;
   the construction-site allowlist passes; the PR changed-file set is a subset
   of C3.
6. `herdr-versions.md` contains complete NDJSON columns for every listed
   release from 0.8.0.
7. The public-item pin contains exactly the three D9 additions.
8. `gh pr view feature/ay8-herdr-socket-transport --json
   headRefName,baseRefName,state` reports base `integrate/phase-ay`; AY.8 is not
   linked into the implementation stack.
9. The sprint meets the common phase merge gate: zero blocking, important, or
   in-scope minor findings; quality-mgr posts PASS; all three CI lanes are green
   at merge time; no flaky-test allowance applies.

## Required validation

- `just validate` on all three CI lanes.
- `cargo test -p atm-herdr`.
- `python3 .just/check_line_counts.py`.
- Compare the PR file list mechanically against C3.

## Out of scope

- Selecting or defaulting to the socket transport (AY.9).
- Production composition changes under `atm-daemon-bootstrap` (AY.9).
- Doctor projection changes (AY.9) or live platform evidence (release
  readiness, ruling 5).
- Removing the CLI transport or its ownership keys.
- Any patch, hardening, or remodeling of the legacy synchronous daemon. The
  eventual selection point remains the Tokio/Axum `atm-http-runtime` path.
