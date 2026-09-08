---
id: AY.9
phase: AY
sprint: AY.9
title: Herdr socket cutover, doctor projection, and lifecycle validation
branch: feature/ay9-herdr-socket-cutover
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay9-herdr-socket-cutover
integration_branch: integrate/phase-ay
stack_parent: none
pr_target: integrate/phase-ay
target: integrate/phase-ay
status: complete
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: join
parallel_with: [AY.10]
dependency_relations:
  - prerequisite: AY.9
    dependent: AY.10
    relation: parallel_safe
    rationale: AY.10 changes only atm-herdr, its fixtures, the architecture pin, and Herdr docs; AY.9 owns the composition, config reader, and doctor files. Neither reads the other's diff; the shared `crates/atm-herdr/src/lib.rs`, `transport.rs`, and `crates/atm-architecture/tests/boundary_enforcement.rs` edits are composed under the P-E rule in phase-ay-plan.md.
  - prerequisite: AY.9
    dependent: AY.11
    relation: must_follow
    rationale: AY.11 spawns the shadow task only on the socket composition AY.9 selects.
  - prerequisite: AY.7
    dependent: AY.9
    relation: must_follow
    rationale: Windows CLI process and installer facts must be verified before the final socket-default release-readiness run.
  - prerequisite: AY.8
    dependent: AY.9
    relation: must_follow
    rationale: the direct socket and named-pipe implementation and equivalence suite must merge before production composition selects it.
---

# AY.9 — Herdr socket cutover, doctor projection, and lifecycle validation

Select the AY.8 socket transport in the Tokio/Axum production composition,
make it the default with the CLI transport as the permanent explicit
alternative, and close its
configuration, doctor, and automated lifecycle contracts on all three CI
lanes. AY.9 is the socket-cutover join; AY.10 runs beside it and AY.11
(and AY.12 if Rand decides to drop the poll) follow it before the phase
lands on develop. Live macOS/Windows operator proof is release readiness
after the phase lands on develop (ruling 5); the Ship/Defer/Cancel
disposition of the cutover is taken on AY.9's automated gates, and phase
completion is defined under "Phase AY exit gate" in phase-ay-plan.md.

## Dispatch and PR topology

AY.9 dispatches only after AY.7 and AY.8 have both merged into
`integrate/phase-ay`; create its branch from that merged integration head. It
is not stacked on either parent. `/gh-stack` is linear, so a two-parent join
cannot be represented as a child without hiding one prerequisite or sharing a
branch across stacks. The implementation stack must already have merged bottom to
top under its sprint documents; AY.8 is an independent parallel PR. AY.9 uses
ordinary PR tooling with `pr_target: integrate/phase-ay` and never merges an
unmerged parent branch into itself.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or shape-only
completion fails the sprint.

- [ ] D1 — extend AY.4's `daemon_herdr_config` reader and
  `DaemonHerdrConfig` in `crates/atm-daemon-bootstrap/src/herdr_config.rs` to
  accept exactly the new optional `transport` key while retaining
  `deny_unknown_fields`. At the single production `HerdrProcessInvoker::new`
  composition site in `crates/atm-daemon-bootstrap/src/replacement_handler.rs`,
  pass the C1 transport selection as part of `HerdrClientConfig`. Inside
  `atm-herdr`, the invoker factory builds `HerdrIo::Socket` or `HerdrIo::Cli`;
  the crate-private enum never crosses the crate boundary. Default to `socket`;
  reject every other string with `AtmErrorCode::ConfigParseFailed`. Replace
  AY.8's `socket_variant_constructed_only_in_tests` pin
  (`crates/atm-herdr/tests/socket_construction_pin.rs`) with a pin that permits the one
  production factory inside `atm-herdr`. No second composition site is
  introduced.
- [ ] D2 — retain CLI as a permanent, explicit, documented fallback (rework
  2026-09-06). The transport is chosen exactly once at daemon start by the
  D1 factory from `herdr.transport`; `cli` is selected only by explicit
  config; doctor reports the active transport; the choice never changes
  while the daemon runs and runtime failure never falls back from socket
  to CLI (a socket connect failure is a Herdr-unavailable breaker
  event with the same mapping as a CLI spawn failure). No removal release is
  scheduled and no ownership-key cleanup is recorded.
- [ ] D3 — rerun AY.4's authoritative lifecycle matrix L1–L12 against the
  socket default on all three CI lanes. Adapt only: no binary becomes endpoint
  not configured or
  unreachable; `server_not_running` becomes refused/absent endpoint; the old
  protocol-mismatch case becomes a server-recording/version switch between
  calls with no daemon restart. Keep the distinct below-minimum case and keep
  CLI variants green permanently (both transports are supported). L5 shutdown/drain and resource
  release, L10 restart dedup, L11 flapping suppression, and L12 stalled-
  notification deadline are explicitly included after the transport swap.
- [ ] D4 — extend doctor without crossing crate boundaries: each endpoint
  reports `transport: socket` and a privacy-preserving display form of the
  resolved Unix socket or Windows pipe in the existing atm-core-owned
  `HerdrEndpointObservation.endpoint`. `HerdrDoctorProbe::new` consumes the
  same C1 selection as the invoker. `HerdrEndpoint` never appears in atm-core
  or atm-daemon-bootstrap, and raw endpoint paths never enter doctor DTOs,
  human/JSON output, snapshots, or logs.
- [ ] D5 — update the Herdr configuration reference and operator documentation
  with the closed C1 values, socket default, explicit CLI fallback, no-silent-
  fallback rule, and doctor transport/endpoint fields. The docs name the
  release-readiness checklist as the live gate; they do not claim live proof
  in AY.9.
- [ ] D6 — tests and automated gates under Required validation pass on the
  branch and all three CI lanes. AY.9 contains no operator-authored live
  evidence and does not record the phase disposition.

### Paths to delete

- AY.8's temporary test-only architecture allowlist for
  `HerdrIo::Socket(` construction. Replace it with the production-factory pin
  in D1; do not delete the underlying architecture assertion.

No other path is deleted in the Ship case. The CLI transport is never
removed (D2).

## Code and configuration contracts

### C1 — closed transport selection

```toml
[herdr]
transport = "socket" # optional; this is the default when omitted
binary_path = "/absolute/path-or-directory" # used only by the CLI fallback
socket_path = "/absolute/herdr-api-endpoint" # optional endpoint override
```

```rust
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)] // illustrative; the canonical DTO is AY.3's
#[serde(rename_all = "snake_case")]
pub enum HerdrTransportKind {
    Cli,
    Socket,
}

pub struct HerdrClientConfig {
    transport: HerdrTransportKind,
    binary_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,
}

impl HerdrClientConfig {
    pub fn try_new(
        transport: HerdrTransportKind,
        binary_path: Option<PathBuf>,
        socket_path: Option<PathBuf>,
    ) -> Result<Self, AtmError>;

    pub fn transport(&self) -> &HerdrTransportKind;
    pub fn binary_path(&self) -> Option<&Path>;
    pub fn socket_path(&self) -> Option<&Path>;
}

impl Default for HerdrClientConfig {
    fn default() -> Self {
        Self {
            transport: HerdrTransportKind::Socket,
            binary_path: None,
            socket_path: None,
        }
    }
}

// Exact behavior across the composition/factory boundary:
// composition passes a successfully constructed HerdrClientConfig to atm-herdr
// omitted or "socket" => atm-herdr's private factory builds HerdrIo::Socket(SocketIo)
// "cli"               => atm-herdr's private factory builds HerdrIo::Cli(CliIo)
// any other value      => ConfigParseFailed; no daemon startup
// a socket call failure => returned as HerdrError; never retry through CLI
```

The actual enum remains atm-core's existing DTO. The configuration reader uses
that type or a crate-local deserialization enum and must not create a third
public transport type. AY.9 extends AY.3's fallible constructor with the
transport argument; fields remain private, and the deny-unknown-fields reader
cannot bypass `try_new`. Its no-file/no-table defaults now select `socket`; its
fixture matrix adds omitted, `socket`, `cli`, unknown string, and unknown key.
The unknown string and unknown key both fail with `ConfigParseFailed`, with the
file and offending key/value named according to AY.3's error contract.

### C1a — changed-file allowlist

- `crates/atm-herdr/src/transport.rs` (the one production factory; the
  AY.9 pin names this file)
- `crates/atm-herdr/src/lib.rs` (factory export only)
- `crates/atm-herdr/tests/socket_construction_pin.rs` (AY.8 D10 allowlist
  replaced by the production-factory pin)
- `crates/atm-daemon-bootstrap/src/herdr_config.rs` (closed transport enum,
  `socket_path`, validation matrix)
- `crates/atm-daemon-bootstrap/src/replacement_handler.rs` (composition
  selects the factory)
- `crates/atm-daemon-bootstrap/src/herdr_lifecycle_tests.rs` (L1–L12 under
  socket default and explicit CLI)
- `crates/atm-core/src/doctor/**` files that AY.3 D4 created for the Herdr
  section (transport/endpoint fields and snapshots)
- `crates/atm-herdr/src/doctor_probe.rs` (the one assignment/call site that
  hard-codes doctor transport CLI and endpoint None; added by fenix
  2026-09-07 as a C1a plan defect found during AY.9 D4, arch-ctm msg
  01M1Y9F78Z4Q1W4K5NHHA294AQ; conversion stays in transport.rs so
  `HerdrEndpoint` never crosses the crate)
- `boundaries/atm-core/herdr-endpoint-doctor.toml` (composition root and
  status notes updated so `atm_herdr::transport` is the recorded caller of
  `HerdrEndpointDisplay::from_relative` in place of `doctor_probe`; added
  by fenix 2026-09-07 for AY9-QA-001, the doctor_probe.rs amendment above
  relocated the sanitizer call but left the boundary record stale;
  boundary-guard review required as for any boundary-policy change)
- `crates/atm-architecture/tests/boundary_enforcement.rs` (forbidden-edge
  grep for `HerdrEndpoint`, factory pin)
- `docs/atm-herdr/architecture.md`, `docs/atm-herdr/requirements.md`,
  `docs/project-plan.md`, user configuration reference
- `.sprints/AY/events.ttl`

No file under `crates/atm-daemon` changes; `transport_socket.rs`,
`transport_cli.rs`, and `status_stream.rs` do not change.

### C2 — doctor projection

The existing endpoint schema is extended by populated values, not new keys:

```json
{
  "herdr": {
    "configured": true,
    "endpoints": [
      {
        "session": "default",
        "provenance": "herdr_default",
        "transport": "socket",
        "endpoint": "$HOME/.config/herdr/herdr.sock",
        "binary": null,
        "state": {"kind": "ok", "version": "0.8.2", "protocol": 20},
        "remedy": "none",
        "capabilities": {"live_handoff": true},
        "members": [{"name": "cipher", "outcome": {"kind": "visible"}}]
      }
    ],
    "breaker": {
      "state": "closed",
      "retry_after_ms": null,
      "consecutive_failures": 0
    }
  }
}
```

The committed snapshot pins this exact representation. `state` remains the
internally tagged object required by data-bearing `HerdrDoctorState` variants;
the `kind`, provenance, transport, and outcome-kind names are snake_case; the
outcome remains the tagged object defined by AY.3. Every
`members[]` object has exactly `name` and `outcome`; there is no `ordinal` or
`member` wrapper, and there is no aggregate `herdr.state` or `herdr.remedy`.
Default precedes sessions sorted bytewise. Before populating any endpoint-
bearing doctor field, atm-herdr converts its public `HerdrEndpoint` type—which
remains owned inside atm-herdr in this flow—into AY.3's validated
`HerdrEndpointDisplay`: a matching
captured root becomes `$XDG_CONFIG_HOME`, `$HOME`, or
`%APPDATA%`; an explicit path outside those roots becomes
`<configured>/<file-name>`. On Windows the `\\.\pipe\` prefix is retained and
the path portion is sanitized by the same rule. Tests use synthetic usernames
and assert that neither those names nor raw home/config roots occur anywhere in
serialized JSON, human output, snapshots, or transport logs.

## Required work

1. Extend AY.3's deny-unknown-fields reader and validation matrix with the
   closed transport enum before changing the one production invoker factory.
2. Replace the temporary AY.8 construction allowlist with the single
   production-factory pin, then run socket-default and explicit-CLI lifecycle
   suites on every CI platform.
3. Align doctor snapshots and operator docs with the canonical tagged schema,
   document the permanent explicit CLI fallback, and keep all live evidence
   out of this sprint (ruling 5).

## Acceptance criteria

1. Omitted transport and `transport = "socket"` both select Socket; `"cli"`
   selects CLI; any other value fails startup with `ConfigParseFailed`; a
   socket error never invokes CLI.
2. AY.4 lifecycle tests L1–L12 pass on macOS, Linux, and Windows after the
   socket-default swap, with L5 and L10–L12 explicitly re-verified, the server-
   version switch and below-minimum behavior retained as distinct tests, and
   CLI variants also green.
3. Doctor snapshots prove socket transport and sanitized endpoint display on
   Unix and Windows, deterministic endpoint order, exact member-entry keys,
   no aggregate state/remedy, and no raw username/home/config root. A
   forbidden-edge grep proves `HerdrEndpoint` does not enter atm-core or
   atm-daemon-bootstrap.
4. The AY.2 zero-regression oracle passes through the socket default.
5. User docs and `docs/project-plan.md` record the CLI transport as a
   permanent, explicitly selected alternative (`herdr.transport = "cli"`),
   the cutover release, and that no removal release and no CLI ownership-key
   cleanup are scheduled.
6. The temporary AY.8 test-only allowlist is replaced and the one production
   Socket factory inside `atm-herdr` is pinned; `HerdrIo` does not cross its
   crate boundary.
7. The AY.3 config-reader matrix includes omitted, `socket`, `cli`, unknown
   string, and unknown key; only the two supported values parse through
   `HerdrClientConfig::try_new`, its fields remain private, and no removal
   release for the CLI transport is recorded anywhere in the plan.
8. `gh pr view feature/ay9-herdr-socket-cutover --json
   headRefName,baseRefName,state` reports base `integrate/phase-ay` after both
   parent PRs merged; AY.9 is not linked into the implementation stack.
9. No path under `docs/plans/phase-ay/evidence/` is added or changed by AY.9;
   no sprint carries live evidence.
9a. The PR file set is a subset of C1a (`git diff --name-only <base>..HEAD`
    compared mechanically), and `git diff <base>..HEAD -- crates/atm-daemon
    crates/atm-herdr/src/transport_socket.rs crates/atm-herdr/src/transport_cli.rs
    crates/atm-herdr/src/status_stream.rs` is empty.
10. The sprint meets the common phase merge gate: zero blocking, important, or
    in-scope minor findings; quality-mgr posts PASS; all three CI lanes are
    green at merge time; no flaky-test allowance applies.

## Required validation

- `just validate` on all three CI lanes.
- Socket-default and CLI-fallback lifecycle suites on all three lanes.

## Out of scope

- New Herdr capabilities or a change to Herdr's protocol.
- Consuming the AY.10 status stream or changing the queue-wake poll
  (AY.11, AY.12). AY.9 selects the transport; it does not hold a stream.
- Removing or deprecating the CLI transport (no removal release exists).
- Live macOS/Windows proof (release readiness) and the phase disposition
  (Rand, on AY.9's automated gates and the phase-ending review).
- Any patch, hardening, or remodeling of the legacy synchronous daemon. D1 is
  exclusively the Tokio/Axum `atm-http-runtime` composition path planned for
  the AL.5–AL.7 cutover; legacy dispatch remains frozen for Phase AM deletion.
