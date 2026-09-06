---
phase: AY
sprint: AY.9
title: Herdr socket cutover, doctor projection, and lifecycle validation
branch: feature/ay9-herdr-socket-cutover
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay9-herdr-socket-cutover
integration_branch: integrate/phase-ay
stack_parent: none
pr_target: integrate/phase-ay
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: join
parallel_with: []
dependency_relations:
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
make it the default with an explicit one-minor CLI fallback, and close its
configuration, doctor, and automated lifecycle contracts on all three CI
lanes. AY.9 is the phase's last sprint; live macOS/Windows operator proof
is release readiness after the phase lands on develop (ruling 5), and the
phase disposition is taken on AY.9's automated gates.

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

- [ ] D1 — extend AY.3's `deny_unknown_fields` reader in
  `crates/atm-daemon-bootstrap/src/herdr_config.rs` to accept exactly the new
  optional `transport` key. At the single production `HerdrProcessInvoker::new`
  composition site in `crates/atm-daemon-bootstrap/src/replacement_handler.rs`,
  pass the C1 transport selection as part of `HerdrClientConfig`. Inside
  `atm-herdr`, the invoker factory builds `HerdrIo::Socket` or `HerdrIo::Cli`;
  the crate-private enum never crosses the crate boundary. Default to `socket`;
  reject every other string with `AtmErrorCode::ConfigParseFailed`. Replace
  AY.8's test-only construction-site allowlist with a pin that permits the one
  production factory inside `atm-herdr`. No second composition site is
  introduced.
- [ ] D2 — retain CLI as an explicit, documented fallback through atm 1.5.x
  and remove it in atm 1.6.0. It is selected only by
  `herdr.transport = "cli"`; runtime failure never silently falls back from
  socket to CLI. Add the 1.6.0 removal and ownership-key cleanup to
  `docs/project-plan.md`.
- [ ] D3 — rerun AY.4 lifecycle tests (a)–(k) against the socket default on all
  three CI lanes. Adapt only: no binary becomes endpoint not configured or
  unreachable; `server_not_running` becomes refused/absent endpoint; the old
  protocol-mismatch case becomes a server-recording/version switch between
  calls with no daemon restart. Keep the distinct below-minimum case and keep
  CLI variants green while fallback exists.
- [ ] D4 — extend doctor without crossing crate boundaries: each endpoint
  reports `transport: socket` and the display form of the resolved Unix socket
  or full Windows pipe in the existing atm-core-owned
  `HerdrEndpointObservation.endpoint`. `HerdrDoctorProbe::new` consumes the
  same C1 selection as the invoker. `HerdrEndpoint` never appears in atm-core
  or atm-daemon-bootstrap.
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

No other path is deleted in the Ship case. CLI transport removal is the
atm 1.6.0 follow-up recorded by D2, not part of AY.9.

## Code and configuration contracts

### C1 — closed transport selection

```toml
[herdr]
transport = "socket" # optional; this is the default when omitted
binary_path = "/absolute/path-or-directory" # used only by the CLI fallback
socket_path = "/absolute/herdr-api-endpoint" # optional endpoint override
```

```rust
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)] // illustrative; the canonical DTO is AY.3's
#[serde(rename_all = "snake_case")]
pub enum HerdrTransportKind {
    Cli,
    Socket,
}

pub struct HerdrClientConfig {
    pub transport: HerdrTransportKind,
    pub binary_path: Option<PathBuf>,
    pub socket_path: Option<PathBuf>,
}

// Exact behavior across the composition/factory boundary:
// composition passes HerdrClientConfig { transport, ... } to atm-herdr
// omitted or "socket" => atm-herdr's private factory builds HerdrIo::Socket(SocketIo)
// "cli"               => atm-herdr's private factory builds HerdrIo::Cli(CliIo)
// any other value      => ConfigParseFailed; no daemon startup
// a socket call failure => returned as HerdrError; never retry through CLI
```

The actual enum remains atm-core's existing DTO. The configuration reader uses
that type or a crate-local deserialization enum and must not create a third
public transport type. Its no-file/no-table defaults now select `socket`; its
fixture matrix adds omitted, `socket`, `cli`, unknown string, and unknown key.
The unknown string and unknown key both fail with `ConfigParseFailed`, with the
file and offending key/value named according to AY.3's error contract.

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
        "endpoint": "/Users/operator/.config/herdr/herdr.sock",
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
Default precedes sessions sorted bytewise. On Windows `endpoint` is the full
`\\.\pipe\...` display string.

## Required work

1. Extend AY.3's deny-unknown-fields reader and validation matrix with the
   closed transport enum before changing the one production invoker factory.
2. Replace the temporary AY.8 construction allowlist with the single
   production-factory pin, then run socket-default and explicit-CLI lifecycle
   suites on every CI platform.
3. Align doctor snapshots and operator docs with the canonical tagged schema,
   record the atm 1.6.0 fallback-removal obligation, and keep all live evidence
   out of this sprint (ruling 5).

## Acceptance criteria

1. Omitted transport and `transport = "socket"` both select Socket; `"cli"`
   selects CLI; any other value fails startup with `ConfigParseFailed`; a
   socket error never invokes CLI.
2. Adapted lifecycle tests (a)–(k) pass on macOS, Linux, and Windows, with the
   server-version switch and below-minimum behavior retained as distinct tests;
   CLI variants also pass.
3. Doctor snapshots prove socket transport and endpoint on Unix and Windows,
   deterministic endpoint order, exact member-entry keys, and no aggregate
   state/remedy. A forbidden-edge grep proves `HerdrEndpoint` does not enter
   atm-core or atm-daemon-bootstrap.
4. The AY.2 zero-regression oracle passes through the socket default.
5. The CLI fallback, cutover release, exact removal release, and later deletion
   of CLI ownership keys are recorded in user docs and `docs/project-plan.md`.
6. The temporary AY.8 test-only allowlist is replaced and the one production
   Socket factory inside `atm-herdr` is pinned; `HerdrIo` does not cross its
   crate boundary.
7. The AY.3 config-reader matrix includes omitted, `socket`, `cli`, unknown
   string, and unknown key; only the two supported values parse and 1.6.0 is
   the pinned CLI-removal release.
8. Removed (r24): the official benchmark runs once at release readiness on
   the develop build (Rand, 2026-09-05); no sprint or phase gate.
9. `gh pr view feature/ay9-herdr-socket-cutover --json
   headRefName,baseRefName,state` reports base `integrate/phase-ay` after both
   parent PRs merged; AY.9 is not linked into the implementation stack.
10. No path under `docs/plans/phase-ay/evidence/` is added or changed by AY.9;
   no sprint carries live evidence.

## Required validation

- `just validate` on all three CI lanes.
- Socket-default and CLI-fallback lifecycle suites on all three lanes.
- quality-mgr Final Quality Report: 0 blocking, 0 important, 0 minor in scope.

## Out of scope

- New Herdr capabilities or a change to Herdr's protocol.
- Removing the CLI fallback in the same release as cutover.
- Live macOS/Windows proof (release readiness) and the phase disposition
  (Rand, on AY.9's automated gates and the phase-ending review).
- Any patch, hardening, or remodeling of the legacy synchronous daemon. D1 is
  exclusively the Tokio/Axum `atm-http-runtime` composition path planned for
  the AL.5–AL.7 cutover; legacy dispatch remains frozen for Phase AM deletion.
