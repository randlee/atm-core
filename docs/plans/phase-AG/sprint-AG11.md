---
id: AG.11
title: Corrective Remote-Target Contract And Dispatch Routing
status: planned
branch: docs/cross-host-remote-target-contract
worktree: ../atm-core-worktrees/docs/cross-host-remote-target-contract
target: develop
---

# Sprint AG.11 — Corrective Remote-Target Contract And Dispatch Routing

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.11
worktree: ../atm-core-worktrees/docs/cross-host-remote-target-contract
branch: docs/cross-host-remote-target-contract
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

## Boundary And Type Contract

Illustrative implementation signatures:

```rust
pub struct RemoteTargetHost(pub String);

pub struct ParsedSendTarget {
    pub agent: AgentName,
    pub team: TeamName,
    pub remote_host: Option<RemoteTargetHost>,
}

pub trait SendTargetParser {
    fn parse_target(
        &self,
        raw_target: &str,
        explicit_host: Option<&str>,
    ) -> Result<ParsedSendTarget, AtmError>;
}

pub trait CrossHostDelivery {
    fn deliver_remote(
        &self,
        request: &SendRequest,
        remote_host: &RemoteTargetHost,
    ) -> Result<SendOutcome, AtmError>;
}
```

These names are illustrative, but the sprint requires equivalent explicit
ownership so remote-target parsing and dispatch do not leak across unrelated
daemon/runtime surfaces.

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
- the sprint closes only when the remote-target dispatch branch is observable in
  automated validation

## Required Validation

- unit tests for target parsing and normalization
- integration tests for local-path vs remote-path dispatch selection
- integration tests proving remote-target failure does not write to the local
  mailbox path
- integration tests proving deferred-delivery results and sender-inbox receipts
  follow the bounded retry policy
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
