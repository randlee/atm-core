---
phase: AY
sprint: AY.2
title: Private transport foundation, CLI pure motion, and portable fake Herdr
branch: feature/ay2-herdr-transport-seam
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay2-herdr-transport-seam
integration_branch: integrate/phase-ay
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_track: core
parallel_with: [AY.1]
stack_parent: integrate/phase-ay
pr_target: integrate/phase-ay
dependency_relations:
  - prerequisite: AY.1
    dependent: AY.2
    relation: parallel_safe
    rationale: AY.1 owns audit, version, requirements, ADR, architecture, and AQ-history documents; AY.2 owns only atm-herdr Rust and fixture paths, so their file sets, public contracts, and artifacts do not intersect.
  - prerequisite: AY.2
    dependent: AY.3
    relation: must_follow
    rationale: AY.3 promotes the private default-only HerdrClientConfig into the validated public composition input and adds all doctor DTOs, probe behavior, boundary inventory, and public-surface pins on top of this transport and fixture foundation.
  - prerequisite: AY.2
    dependent: AY.8
    relation: must_follow
    rationale: AY.8 adds Socket(SocketIo) to the private transport seam and extends the conformance fixture established here.
---

# AY.2 — Private transport foundation, CLI pure motion, and portable fake Herdr

Introduce the private transport boundary that permits AY.8 to add a direct
UDS/named-pipe client while preserving the complete Phase AX CLI behaviour.
It replaces Unix-only shell fixtures with one portable fake-Herdr binary while
leaving the crate's public surface and boundary inventory unchanged. It does
not implement doctor behavior, expose client configuration, activate a socket
transport, or change daemon composition.

## Delivery topology and `/gh-stack`

AY.2 is the bottom of the only linear Phase AY implementation stack:

```text
integrate/phase-ay <- AY.2 <- AY.3 <- AY.4 <- AY.5 <- AY.6 <- AY.7
```

The implementer must use the `/gh-stack` skill for this chain. Create AY.2 from
`integrate/phase-ay`; create each child from its immediate parent. As child PRs
open, record the stack and inspect its actual PR bases noninteractively:

```bash
gh stack link --base integrate/phase-ay \
  feature/ay2-herdr-transport-seam \
  feature/ay3-herdr-endpoint-doctor-config
gh pr view feature/ay3-herdr-endpoint-doctor-config \
  --json headRefName,baseRefName,state
```

Append AY.4 through AY.7 with `gh stack link <stack-number> <branch>` when their
PRs exist. Parent-development push, not QA completion, triggers a merge-forward
from the parent branch into every active child before a development or fix
round. Parent PRs merge first, in stack order. Repository policy permits only
merge commits and forbids force-pushes, `gh stack rebase`, `gh stack sync`, and
`gh stack merge`; merge each PR with `gh pr merge --merge`.

AY.1 remains an independent parallel branch. AY.8 is a multi-parent join and is
created from `integrate/phase-ay` only after AY.1, AY.2, and AY.3 have merged;
it must not be appended to this stack.

## Preconditions

- P-A — `integrate/phase-ay` is cut from the Phase AX integration branch after
  AX.6 and contains develop merge `a7aebefb8` (PR #1218). Verify with
  `git merge-base --is-ancestor a7aebefb8 integrate/phase-ay`.
- P-B — the Phase AY plan has dated approval from Rand.
- Development runs on macOS or Linux; all three CI lanes, including
  `windows-latest`, are merge gates. No Windows machine and no live Herdr
  service are required for normal development.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or shape-only
completion fails the sprint.

- [ ] D1 — Add the crate-private transport foundation in
  `crates/atm-herdr/src/transport.rs` exactly as Contract C1 specifies.
  `HerdrProcessAdapter` remains the only public trait, and all its existing
  per-call `session` and `RequestDeadline` parameters are unchanged. Move
  transport-independent envelope-to-domain parsing without changing its result
  or error semantics.
- [ ] D2 — Move today's process execution and session-environment logic into
  `crates/atm-herdr/src/transport_cli.rs` as `CliIo`. This is pure motion:
  preserve every ADR-058 argv, environment, timeout, error, breaker, and
  process-lifecycle result. AY.2 constructs only the default private config and
  continues to resolve `herdr` by name exactly as the Phase AX code does.
- [ ] D3 — Replace every `/bin/sh` process fixture with a Rust fake-Herdr test
  binary at `crates/atm-herdr/tests/support/fake_herdr/main.rs`, exposed only as
  a test `[[bin]]` and located with `CARGO_BIN_EXE_fake_herdr`. It supports the
  exact modes in Contract C2, so the process-behaviour suite executes with
  identical assertions on macOS, Linux, and Windows.
- [ ] D4 — Add the version recordings under
  `crates/atm-herdr/tests/fixtures/herdr-versions/`: a byte-exact v0.8.2
  recording and `manifest.json`, plus a v0.8.0 blocked-prompt delta mode. Run
  the conformance suite for every recording directory and delta. Perform the
  one-off ADR-058 suite against the published real v0.8.0 and v0.8.2 macOS
  artifacts and cite the results in the PR.
- [ ] D5 — Add deterministic focused tests for child stdout parsing, argv and
  `HERDR_SESSION` round-trip with two sessions through one invoker, binary
  resolution through the existing Phase AX path, version replay between calls,
  unknown JSON fields, CRLF tolerance, and the full fake-Herdr mode set.
- [ ] D6 — Prove the refactor is a zero-regression change: the complete ADR-058
  fixture suite and existing atm-herdr architecture/boundary tests pass without
  fixture edits, the public item list is unchanged, and both boundary inventory
  files remain byte-identical to `integrate/phase-ay`.

### Paths that must not change

- `crates/atm-core/src/doctor/**`.
- `crates/atm-herdr/src/doctor_probe.rs`.
- `boundaries/atm-herdr/herdr-process-adapter.toml`.
- `docs/atm-herdr/boundaries.md`.
- Public exports from `crates/atm-herdr`; AY.3 owns the public config/probe
  surface and its matching boundary update.

### Paths to delete

None.

## Code contracts

### C1 — private default-only transport foundation

```rust
// crates/atm-herdr/src/transport.rs
#[derive(Clone, Debug, Default)]
pub(crate) struct HerdrClientConfig {
    binary_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,
}

pub(crate) enum HerdrOp<'a> {
    Prompt {
        agent: &'a AgentName,
        text: &'a str,
    },
    Wait {
        agent: &'a AgentName,
        until: &'a [HerdrAgentStatus],
        timeout: Duration,
    },
    Get { agent: &'a AgentName },
    List,
    Notify { title: &'a str, body: &'a str },
}

pub(crate) struct HerdrEnvelope {
    pub result: Option<serde_json::Value>,
    pub error: Option<HerdrErrorEnvelope>,
}

pub(crate) struct HerdrErrorEnvelope {
    pub code: String,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

pub(crate) enum HerdrIo {
    Cli(CliIo),
}

impl HerdrIo {
    pub(crate) async fn call(
        &self,
        op: HerdrOp<'_>,
        session: Option<&HerdrSession>,
        deadline: RequestDeadline,
    ) -> Result<HerdrEnvelope, HerdrError>;
}
```

Only `HerdrClientConfig::default()` is constructed in AY.2; both optional fields
are `None`, preserving PATH lookup and the Phase AX session behavior. The type
is not exported, has no validation API, and supplies no user configuration in
this sprint. AY.3 makes it public, adds validated explicit fields, and owns all
composition use. `HerdrProcessInvoker` contains `{ breaker, io: HerdrIo }` and
selects `HerdrIo::Cli`. `HerdrError` remains the existing closed enum.

`HerdrIo`, `HerdrOp`, `HerdrEnvelope`, `HerdrErrorEnvelope`,
`HerdrClientConfig`, and `CliIo` are all `pub(crate)`. There is no server-info
operation, doctor probe, public config, second public trait, `async_trait`, or
process-shaped public response in AY.2.

### C2 — portable fake-Herdr modes

The fake binary supports deterministic selection of:

```text
exit-success
exit-failure
stderr-envelope: server_not_running | agent_prompt_stalled | timeout | protocol_mismatch
stdout-json-line
sleep-past-deadline
echo-argv-and-herdr-session
status-server-json:<version,protocol,capabilities>
replay:<fixture-directory>
v0.8.0-blocked-prompt
```

It writes byte-exact LF output with `write_all`, accepts the platform's `.exe`
suffix through Cargo, and is never implemented with a `.cmd` shim. Injected
deadlines and hard bounds make timeout tests deterministic.

### C3 — unchanged public and boundary surface

`HerdrProcessAdapter` remains atm-herdr's only public trait and its Phase AX
method signatures are byte-for-byte unchanged. No public item is added. The
existing boundary TOML, narrative boundary documentation, public exports, and
architecture pins remain unchanged. AY.3 owns the single coordinated change to
all of those surfaces.

## Required work

1. Extract `HerdrOp`, envelope parsing, and CLI execution in reviewable pure
   moves before adding configuration behaviour; keep all observable Phase AX
   results stable.
2. Move the existing session-environment helper without extending it. An
   explicit per-call session still produces `HERDR_SESSION`; otherwise AY.2
   leaves endpoint selection to Herdr's default exactly as Phase AX does.
3. Keep `HerdrEnvelope -> AgentSnapshot / HerdrError` parsing transport-neutral.
   Treat `timeout` and `agent_prompt_stalled` as the same ATM-visible outcome;
   key on error codes, never message text.
4. Preserve notify argv exactly as `notification show <title> --body <body>
   --sound request`.
5. Build fixtures from the Herdr version ledger/manifests without requiring a
   live Herdr service in CI. Parsers must tolerate unknown fields and a trailing
   carriage return.
6. Keep every doctor, public-config, boundary-inventory, and composition concern
   out of the diff so AY.3 can land them as one coordinated boundary change.

## Acceptance criteria

1. The Phase AX zero-regression oracle is green on macOS and Linux: the entire
   ADR-058 suite and atm-herdr boundary-enforcement checks pass with no existing
   fixture edits.
2. The `windows-latest` CI lane executes, rather than cfg-skips, every
   process-behaviour test and produces the same assertions.
3. Replay passes for every `fixtures/herdr-versions/*/manifest.json` and every
   declared delta mode. The one-off real-v0.8.0 and real-v0.8.2 results are
   cited in the PR.
4. A diff of the `HerdrProcessAdapter` trait block is empty. One invoker routes
   two successive calls to two explicit sessions correctly.
5. No `async_trait` dependency is added. Every type introduced by Contract C1
   is `pub(crate)`, the public item list is unchanged, and no protocol type
   leaks publicly.
6. CLI argv, PATH resolution, `HERDR_SESSION`, timeout, kill/reap, error mapping,
   and breaker behavior are byte-identical to the Phase AX oracle. No
   `HERDR_SOCKET_PATH` config behavior or config-validation API is introduced.
7. Removed (r24): the official benchmark runs once at release readiness on
   the develop build (Rand, 2026-09-05); no sprint or phase gate.
8. `crates/atm-core/src/doctor/**`, `doctor_probe.rs`, both boundary inventory
   files, and atm-herdr's public exports are unchanged from the sprint base.
9. The sprint meets the common phase merge gate: zero blocking, important, or
   in-scope minor findings; quality-manager PASS on the PR; CI green at merge;
   no flaky-test allowance; no Tokio added to atm-core.

## Required validation

- `cargo test -p atm-herdr`
- `just validate`
- `python3 .just/check_line_counts.py`
- `gh pr view feature/ay2-herdr-transport-seam --json
  headRefName,baseRefName,state` after the first child is linked, verifying
  AY.2 is the stack bottom and its PR targets `integrate/phase-ay`.
- `git diff --exit-code integrate/phase-ay...HEAD --
  boundaries/atm-herdr/herdr-process-adapter.toml
  docs/atm-herdr/boundaries.md crates/atm-core/src/doctor`.
- Existing architecture checks prove Contract C3 and the unchanged public item
  list; no new public-item/signature pin is authored in AY.2.

## Non-closure and out of scope

- No daemon composition or transport-selection change.
- No doctor DTO, probe, port, error mapping, config-file reader, report assembly,
  or CLI rendering; AY.3 owns all of those surfaces.
- No public `HerdrClientConfig`, validation API, explicit binary/socket
  configuration, public boundary inventory, or public-item/signature pin; AY.3
  owns their coordinated production-complete change.
- No installer or Windows-specific production code.
- No UDS/named-pipe socket transport and no direct-client cutover; AY.8 adds the
  transport and AY.9 activates it.
- No live Windows evidence campaign.
- No change to the frozen legacy synchronous daemon.
