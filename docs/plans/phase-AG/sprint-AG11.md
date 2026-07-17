---
id: AG.11
title: Corrective Remote-Target Contract And Dispatch Routing
status: planned
branch: feature/pAG-s11-remote-target-contract
worktree: ../atm-core-worktrees/feature/pAG-s11-remote-target-contract
target: develop
---

# Sprint AG.11 — Corrective Remote-Target Contract And Dispatch Routing

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.11
worktree: ../atm-core-worktrees/feature/pAG-s11-remote-target-contract
branch: feature/pAG-s11-remote-target-contract
status: planned
estimated_scope: medium
```

## Goal

Close `AG-FIND-005` by making remote-target sends a first-class CLI and runtime
contract instead of a local-mailbox fallthrough.

## Deliverables

- exact supported operator syntax:
  - `atm send <agent>@<team>.<host> ...`
  - `atm send <agent>@<team> --host <host> ...`
- exact parser contract for the inline form:
  - the inline form splits on the final `.` after `@`
  - the suffix after that final `.` is the remote host
  - the prefix before that final `.` is the team name
  - agent/member names and team names must not contain `.`
  - mixed inline-host plus `--host` input is rejected instead of silently
    choosing one source
- one typed remote-target field in the send request model
- one dispatch rule:
  - empty remote-target host => local mailbox path
  - non-empty remote-target host => cross-host delivery trait boundary
- `localhost` and the sender host's own advertised or bound IP address remain
  ordinary non-empty remote-target hosts on that same remote-delivery branch
- explicit rejection/error path when a remote-target send cannot use the
  cross-host delivery path
- one delivery-result policy:
  - healthy path => wait up to `10s` for remote acceptance
  - unhealthy path => return immediate deferred-delivery result
  - daemon continues bounded retry for `60s..120s`
  - final delivery/failure receipt lands in sender inbox
- requirements / architecture / ADR updates that describe this dispatch rule
- findings-ledger linkage to `AG-FIND-005`
- one dedicated ADR (`ADR-031`) for the remote-target contract and dispatch
  boundary
- one authoritative deletion/reduction ledger for stale AG.3-AG.10 cross-host
  surfaces that must not survive the corrective line without an explicit
  justification

## Boundary And Type Contract

Illustrative implementation signatures:

```rust
pub struct RemoteTargetHost(String);

pub struct ParsedSendTarget {
    pub agent: AgentName,
    pub team: TeamName,
    pub remote_host: Option<RemoteTargetHost>,
}

pub enum SendTargetParseError {
    MissingTeam,
    MissingHost,
    InvalidAgentNameDot,
    InvalidTeamNameDot,
    MixedInlineAndExplicitHost,
    MalformedInlineRemoteTarget,
}

pub trait SendTargetParser {
    fn parse_target(
        &self,
        raw_target: &str,
        explicit_host: Option<&str>,
    ) -> Result<ParsedSendTarget, SendTargetParseError>;
}

pub enum RemoteDeliveryDecision {
    HealthyImmediateWait,
    DeferredRetry,
}

pub enum SendOutcome {
    Delivered,
    Deferred { receipt_message_id: MessageId },
    RejectedTerminal(RemoteFailureKind),
    OutcomeUnknown,
}

pub enum CrossHostDeliveryInfraError {
    RuntimeUnavailable,
    StorageUnavailable,
    InternalInvariantViolation,
}

pub trait CrossHostDelivery {
    fn deliver_remote(
        &self,
        request: &SendRequest,
        remote_host: &RemoteTargetHost,
    ) -> Result<SendOutcome, CrossHostDeliveryInfraError>;
}
```

These names are illustrative, but the sprint requires equivalent explicit
ownership so remote-target parsing and dispatch do not leak across unrelated
daemon/runtime surfaces. `RemoteTargetHost` must be constructible only through
the parser normalization path, not from arbitrary caller strings.

Trait-surface rule:

- `SendTargetParser` and `CrossHostDelivery` are repository-owned sealed
  traits, not external extension points
- the sprint requires one fixed production implementer set chosen by the
  composition root

Transport and localhost invariants:

- socket transport must reuse the same ATM wire message shapes already used on
  the other transports; AG.11 must not introduce a transport-specific socket
  message schema
- `localhost` and self-IP same-host traffic are ordinary remote hosts routed
  through the same parsing, dispatch, listener bind, and socket path as any
  other host
- no loopback-only branch, bypass flag, or localhost-special client path may be
  introduced to make AG.12 or AG.13 pass

## Paths To Delete Or Reduce

This list is authoritative for the AG.11 corrective line. Any item retained
after AG.11 must be justified explicitly in the AG.11 completion report and
must remain behind the sealed cross-host delivery or storage boundary instead
of leaking into general send/runtime code.

- delete env-driven peer endpoint selection as an operator contract:
  - `crates/atm-daemon/src/peer_transport.rs`
    - `daemon_peer_endpoint_from_env`
    - user-facing recovery/error text that instructs operators to set
      `ATM_DAEMON_PEER_ADDR`
  - `docs/plans/phase-AG/cross-host-setup-runbook.md`
    - transitional `ATM_DAEMON_PEER_ADDR` setup steps must be removed once the
      AG.4 / AG.5 SQLite-managed interface rows are the corrective authority
- delete CLI-only loopback transport compatibility paths that bypass the daemon
  runtime:
  - `crates/atm-core/src/transport/testing.rs`
    - `LoopbackClientTransport`
    - loopback-only compatibility-preflight / heartbeat fallbacks
  - `crates/atm/src/composition.rs`
    - the `loopback_transport_*_without_daemon` test line must be replaced by
      AG.12-AG.14 coverage that exercises the real remote-target path through
      the daemon boundary
- delete or reduce any cross-host parsing/classification logic that remains in
  general runtime code after the typed `remote_host` contract exists:
  - `crates/atm-daemon/src/runtime_health.rs`
    - retain only the local-vs-remote branch decision and receipt/retry
      orchestration that belongs above the delivery trait
    - remove host-shape parsing, localhost special-casing, or socket-policy
      decisions that can live inside the sealed delivery boundary instead
- reduce composition-root leakage of transport configuration:
  - `crates/atm-daemon/src/composition.rs`
    - remove direct env-based peer transport configuration loading from the
      production composition path
    - retain only sealed storage-backed interface/allowlist loading and wiring
      into the delivery runtime
- delete any surviving documentation or test language that implies:
  - loopback is a separate product mode rather than ordinary host routing
  - special localhost handling is allowed
  - socket transport may use a transport-specific message schema
  - env variables remain the intended steady-state control plane for peer
    routing

## Acceptance Criteria

- both supported syntaxes normalize to the same typed remote-target field
- the inline parser behavior around final-dot splitting, no-dot
  agent/member/team naming, and mixed host sources is explicit and
  test-covered
- a non-empty remote-target host never writes directly to the local mailbox
  path
- a malformed or unsupported remote-target input fails predictably
- local sends remain on the existing local mailbox path
- same-host remote-target values do not require a dedicated loopback-only field
  or dispatch branch
- same-host remote-target values use the same ATM wire message envelopes and
  the same listener/socket path used for any other remote host
- production composition installs a real non-test-double `CrossHostDelivery`
  implementation on the live send dispatch path
- "healthy" means the daemon has a currently usable enabled interface row, a
  resolvable outbound target, and no cached terminal-failure state for the
  target host; otherwise the send takes the deferred branch immediately
- the sprint closes only when the remote-target dispatch branch is observable in
  automated validation
- the AG.11 completion report accounts for every entry in `Paths To Delete Or
  Reduce` as either:
  - deleted
  - reduced behind the sealed delivery/storage boundary
  - or explicitly deferred with a named follow-on finding

## Required Validation

- unit tests for target parsing and normalization
- integration tests for local-path vs remote-path dispatch selection
- integration tests proving remote-target failure does not write to the local
  mailbox path
- integration tests proving deferred-delivery results and sender-inbox receipts
  follow the bounded retry policy
- composition-root validation proving AG.11 wires the real
  `CrossHostDelivery` implementation into the production send path
- AG.11-enforced tests stay narrow:
  parser and dispatch-routing tests are gated at AG.11 close, while the
  broader same-host functional matrix remains deferred to AG.12 and AG.13 and
  the Tier-3 end-to-end integration backstop is deferred to AG.14
- one timeout-enforcement test proves the `10s` healthy-wait ceiling is a hard
  upper bound rather than best-effort behavior
- one code-audit validation pass records the disposition of every entry under
  `Paths To Delete Or Reduce`
- requirements / architecture / ADR review proving the CLI contract and runtime
  branch are described consistently

## Unit-Test Plan

- `<agent>@<team>.<host>` parses with a non-empty remote-target host
- `<agent>@<team> --host <host>` parses to the same normalized target
- inline parsing splits on the final `.` after `@`
- agent/member names containing `.` fail predictably
- team names containing `.` fail predictably
- mixed inline-host plus `--host` input fails predictably
- missing team or malformed host fails predictably
- local-only target keeps `remote_host == None`
- `RemoteTargetHost` cannot be constructed outside the parser normalization path

## Integration-Test Plan

- local target dispatches only to the local mailbox path
- remote target dispatches only to the cross-host delivery trait boundary
- remote-target delivery failure returns an error and leaves the local mailbox
  untouched
- remote-target delivery success reaches the cross-host path with the normalized
  host value
- same-host values (`localhost` and self-IP) use the same cross-host delivery
  trait path as any other non-empty remote host
- unhealthy remote-target sends return a deferred result without blocking for
  the full retry window
- production-factory coverage proves the live dispatch path constructs and uses
  the real `CrossHostDelivery` implementation rather than a unit-test double
- healthy-path coverage proves the immediate-wait branch stops at the hard
  `10s` ceiling when remote acceptance does not arrive

## Smoke-Test Plan

- no second-host smoke closes this sprint
- same-host and second-host smoke are deferred to AG.12 through AG.16

## Out Of Scope

- localhost/public-interface/second-host functional proof
- transport-security closure
- copied-state release verdict

## Entry Gate

- corrective work is appended after AG.10 rather than silently rewriting
  reviewed AG.6-AG.10 sprint scope

## Ownership

- execution owner: `arch-ctm`
- verification owner: `quality-mgr`
