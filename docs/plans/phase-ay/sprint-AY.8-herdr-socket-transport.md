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
recommended_agent: cipher
recommended_model: fast
rework: 2026-09-06 (transport isolation ruling; see phase-ay-plan.md "Rework record")
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
    relation: parallel_safe
    rationale: AY.8 changes no behavior and touches only atm-herdr internals behind `HerdrProcessAdapter`; the AI.11 gate it once had to exempt was retired on AY.3. P-E(b) was ruled on 2026-09-06 (boundary-guard, TOML diff approved unchanged). AY.8's D1 TOML edit layers onto whatever contract inventory AY.3 lands; resolve at merge-forward, never by waiting.
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
  - prerequisite: AY.8
    dependent: AY.10
    relation: must_follow
    rationale: AY.10 stacks on this branch and reuses SocketIo, endpoint resolution, framing, and the fake socket server for the held events.subscribe stream.
---

# AY.8 — Direct Herdr socket and named-pipe transport without cutover

Implement the transport-independent NDJSON path on Unix-domain sockets and
Windows named pipes, with bounded one-request connections and fake-server
equivalence tests. The production composition root remains on CLI throughout
this sprint.

## Dispatch, parallelism, and PR topology

AY.8 is the independent transport track. Dispatch it as soon as AY.1 and AY.2
have merged into `integrate/phase-ay` and P-E(b) is approved (both true as of
2026-09-06). Create the branch from that integration head. It is not a child
of AY.3 and is not part of the implementation stack. Governing rule (Rand,
2026-09-06): the CLI to UDS/named-pipe swap changes no functionality and is
completely hidden behind `HerdrProcessAdapter`; every other phase feature is
developed independently of it, and AY.8 must not wait on, or be waited on by,
any feature sprint.

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
- [x] D2 — removed 2026-09-06. The AI.11 retired-Windows-transport gate was
  deleted on AY.3 (PR #1273): the repo has no named pipes other than the ones
  Herdr requires, so there is nothing to exempt. AY.8 does not edit
  `boundary_enforcement.rs`.
- [ ] D3 — add `crates/atm-herdr/src/transport_socket.rs` with crate-private
  `SocketIo` and `HerdrIo::Socket(SocketIo)`. Use
  `tokio::net::UnixStream` on Unix and
  `tokio::net::windows::named_pipe::ClientOptions` on Windows; add only Tokio's
  `net` feature if it is not already enabled. No process is spawned and no
  dependency on `interprocess` is added.
- [ ] D4 — add the pure endpoint resolver and crate-private endpoint types in
  C1. Byte fixtures for every C1 case live in a `#[cfg(test)]` module inside
  `transport_socket.rs`, so no item needs to be public for testing.
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
- [ ] D9 — no net additions to the AY.2 public-item pin: `herdr_api_endpoint`,
  `HerdrHostEnv`, `HerdrEndpoint`, and `SocketIo` are all `pub(crate)`. The
  only public item of `atm-herdr` remains `HerdrProcessAdapter` and its
  existing contract types.
- [ ] D10 — preserve the no-cutover guard: no change under
  `crates/atm-daemon-bootstrap`, and the construction-site pin
  `socket_variant_constructed_only_in_tests` in
  `crates/atm-herdr/tests/socket_construction_pin.rs` scans
  `crates/atm-herdr/src/**/*.rs` and permits `HerdrIo::Socket(` only inside
  `#[cfg(test)]` modules of `transport_socket.rs` and under
  `crates/atm-herdr/tests/`. AY.9 rewrites that same test to permit the one
  production factory when it owns transport selection.

### Paths to delete

None.

## Code contracts

### C1 — endpoint resolution

```rust
/// Pure; performs no probe or I/O.
pub(crate) fn herdr_api_endpoint(
    cfg: &HerdrClientConfig,
    session: Option<&HerdrSession>,
    env: &HerdrHostEnv,
) -> HerdrEndpoint;

pub(crate) struct HerdrHostEnv {
    pub(crate) xdg_config_home: Option<PathBuf>,
    pub(crate) appdata: Option<PathBuf>,
    pub(crate) home: Option<PathBuf>,
    pub(crate) platform: Platform,
}

pub(crate) enum HerdrEndpoint {
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
- `crates/atm-herdr/tests/socket_construction_pin.rs` (D10 pin)
- `crates/atm-herdr/src/transport.rs`
- `crates/atm-herdr/src/lib.rs` (one module declaration; no new public exports)
- `crates/atm-herdr/Cargo.toml` (Tokio `net` feature only if needed)
- `boundaries/atm-herdr/herdr-process-adapter.toml`
- `docs/atm-herdr/herdr-versions.md`

Any additional production path is a scope change requiring the sprint plan to
be amended and re-reviewed before implementation continues.

### Size and pre-declared split

AY.8 is one crate, one new module, one fixture server and one test file, so
it is expected to fit one context window despite ten deliverables. If it does
not, the split point is fixed in advance: sprint AY.8a lands D1–D6 and C1–C2
(TOML, `SocketIo`, endpoint resolver, NDJSON framing, cancellation and
permits, macOS/Linux UDS lane) and AY.8b lands D7–D8 (fake socket server,
Windows named-pipe lane, equivalence suite) stacked on AY.8a. D9 and D10
ride AY.8a. If the split is exercised: AY.8a keeps this sprint's branch
(`feature/ay8-herdr-socket-transport`, target `integrate/phase-ay`, stack
parent none) and AY.8b is `feature/ay8b-herdr-socket-equivalence`
(`stack_parent` AY.8a, `pr_target` AY.8a's branch, linked with `gh stack
link --base integrate/phase-ay`); the P-E(b) composed-TOML rule applies at
AY.8a only (D1 rides there; AY.8b edits no boundary TOML); C3 splits by
deliverable (AY.8a: Cargo.toml, boundary TOML, transport_socket.rs,
transport.rs, lib.rs, tests/socket_construction_pin.rs, herdr-versions.md;
AY.8b: tests/support/fake_herdr_socket/** and the equivalence tests); and
AY.9's `must_follow` retargets to AY.8b, because AY.9 needs the equivalence
suite. Exercising the split is a plan amendment PR that updates this
section's status, AY.9's dependency_relations, and the sprint map and wave
tables in phase-ay-plan.md in the same commit. No other split is permitted
without a plan amendment.

## Required work

1. Land the approved boundary record first; do not begin socket code until
   the diff matches P-E(b).
2. Implement endpoint resolution and the bounded one-request protocol as one
   transport boundary, then close every Unix-socket and named-pipe failure with
   the fake server on its owning CI lane.
3. Run the same adapter recordings through CLI and socket variants, update the
   compatibility ledger, and mechanically prove the production composition
   remains unchanged until AY.9.

## Acceptance criteria

1. D1 is the first commit and exactly matches the P-E ruling.
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
7. The AY.2 public-item pin is unchanged by this sprint; `cargo doc` or the pin
   test shows no new public items in `atm-herdr`.
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
- Streaming methods (`events.subscribe`): AY.10, stacked on this branch.
  AY.8's fake server needs no streaming mode; AY.10 adds it.
- Any patch, hardening, or remodeling of the legacy synchronous daemon. The
  eventual selection point remains the Tokio/Axum `atm-http-runtime` path.
