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

Herdr's full source is available (ADR-058 pinned
`/Users/randlee/Documents/github/herdr` at d79fd746 / v0.8.2; current
location to be confirmed with Rand). Required reads, cited in the sprint
doc before dispatch:

| Question | Needed by |
|---|---|
| Does `herdr.exe` ship for Windows, and does it resolve the pipe from `HERDR_SESSION` the same way it resolves the socket path on Unix? | W1 |
| Windows pipe name convention | W1 (doctor reporting), W2 |
| Framing on the pipe (same NDJSON as the socket?) and PROTOCOL_VERSION behaviour | W2 |
| Pipe ACL / impersonation model | W2 |

## Sprint W1 (AY.1): parity via the CLI transport, proven on Windows (size M)

Sprint AY.1, branch `feature/ay1-windows-herdr-cli-parity` off
`integrate/phase-ay` (off develop). Dev and live validation
run on a Windows machine inside a Herdr session; the dev agent is itself
a Herdr-backed roster member on that box.

Deliverables:

1. **Transport seam.** New `crates/atm-herdr/src/transport.rs` with
   `trait HerdrTransport` (`execute(request, deadline) -> HerdrRawResponse`),
   `HerdrRequest`, `HerdrRawResponse`, and the single io::Error to
   `HerdrError` translation. Shape mirrors `Http1Acceptor`
   (atm-http-runtime/src/http1_server.rs:49-75): one trait, all policy
   generic over it.
2. **CLI transport.** `transport_cli.rs`: today's `run_command_with_binary`
   (lib.rs:555-625) moved verbatim. This commit is pure motion and must
   pass the entire ADR-058 fixture suite on macOS and Linux with zero
   fixture edits (zero-regression oracle).
3. **Windows process correctness** inside `transport_cli.rs` only:
   `herdr.exe` resolution honouring PATHEXT plus a configured absolute
   path override (never CWD-relative); CREATE_NO_WINDOW; kill-then-reap
   verified to leave no orphan; UTF-8 stdout handling. Missing binary is
   `ServerUnavailable` with a cause naming what was searched.
4. **Portable fixtures.** Test-only fake herdr helper binary
   (`crates/atm-herdr/tests/support/fake_herdr_bin.rs`, behind the
   test-utils feature) replaces /bin/sh, so all six process tests run on
   all three CI lanes. HR-TEST-006 ("CI never depends on Herdr being
   installed") still holds. No-flaky rule: deterministic fake, injected
   deadlines, hard bounds, nothing may block forever.
5. **Composition and doctor.** One `HerdrCliTransport` construction site
   in `build_replacement_handler`; doctor herdr section reports transport
   kind and resolved binary/endpoint. Architecture tests: single
   construction site; no `cfg(unix|windows)` in atm-herdr outside the
   transport modules.
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
   `atm doctor` herdr section; prompt/wait/get/list round trips with
   observed argv and JSON; end-to-end nudge from another host with
   timestamps at both ends; transport-boundary structured logs; negative
   cases live (Herdr stopped, breaker opens/recovers, agent not found,
   agent blocked, slow command hits the 5s cap with no orphan in
   tasklist); nudge latency sample; explicit confirmation no console
   window flashes.

Acceptance: all seven deliverables; windows-latest CI leg executes the
process-behaviour suite; official zero-regression benchmark run on the
hot path before merge (the diff should not touch it; prove it);
quality-mgr PASS with flaky-test-qa deployed.

## Sprint W2 (AY.2): direct socket/pipe client (size L, blocked on Herdr facts)

`transport_socket.rs` implementing `HerdrTransport` with no child
process: `tokio::net::UnixStream` on unix, `tokio::net::windows::named_pipe`
on Windows (tokio is already an allowed dependency; no new crate edge).
Implements ADR-058 D3 in our code (fresh connection per command,
protocol ping, one NDJSON request, strict PROTOCOL_VERSION equality) with
an explicit bounded read deadline that Herdr's own client lacks.
Prove byte-equivalence by running the whole ADR-058 fixture suite through
both transports; add a Herdr PROTOCOL_VERSION compatibility matrix; flip
the composition-root default with the CLI transport retained one release
as documented fallback. Not started until the pipe framing and ACL model
are read from Herdr source.

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

## Ordering with phase AX

No AX sprint assumes Windows Herdr works (AX.7 excludes it). AX.2 and
AX.6 both edit `HerdrProcessAdapter` and ADR-058. Land W1 before AX.2 and
AX.6: W1 changes no argv and no trait method, and once it lands every
later argv change is validated on Windows by the platform-neutral argv
contract test. If AX.2 lands first, W1 rebases and re-runs the full
Windows evidence set. Widen AX.7 to capture a Windows Herdr run. AX.6's notify
(HR-CORE-010) must be in the platform-neutral argv contract test before
W1 evidence is captured, or W1 captures it in a follow-up evidence round.

## Risks

1. Herdr facts never materialise: W1 still delivers parity without them.
2. Transport extraction silently changes macOS/Linux behaviour: pure-motion
   commit, unedited fixtures.
3. Orphaned herdr.exe after timeout: explicit live-verified test.
4. Console flash per nudge: CREATE_NO_WINDOW plus visual confirmation.
5. Merge collision with AX.2/AX.6: order W1 first.
6. Hot-path regression: official benchmark run before merge.

## Windows machine

Sprint dev and validation need a Windows box running a Herdr session.
Candidate: FastPC4 via Rand (cwin is routed through Rand). Provisioning
decision is Rand's.
