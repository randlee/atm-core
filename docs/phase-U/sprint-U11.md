# Sprint U.11 — ATM-Graft Cleanup

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.11"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u11-atm-graft-cleanup
branch: feature/pU-u11-atm-graft-cleanup
status: completed
estimated_scope: S
```

## Goal

Tighten `atm-graft` after `U.10` with targeted cleanup only.

## Scope Summary

This sprint narrows test-only API seams, removes redundant aliases,
consolidates duplicated daemon-bootstrap path resolution, and reduces
timing-sensitive test waits where feasible.

Lean-design rule:
- no new product capability
- no new boundary expansion
- no new public API added for cleanup convenience

## Governing Requirements

- `REQ-CORE-BOUNDARY-001`
- `REQ-CORE-TRANSPORT-001`
- `REQ-P-CONTRACT-001`

## Governing Boundaries

- `BOUNDARY-AtmProtocol`
- boundary lint must continue preventing `atm-graft` -> `atm-daemon`

## Prerequisites

- `U.10` is complete

## Non-Goals

- new daemon features
- new graft features
- production behavior changes

## Sub-Tasks

1. Narrow test-only `atm-graft` seams
   Development work:
   - narrow `with_poll_interval()` to test-only use unless a real embed seam
     must remain public
   - narrow `from_transport()` to test-only use unless a real embed seam must
     remain public
   - remove redundant `fetch_pending_nudges()` aliases if `fetch_nudges()`
     already covers the required behavior

2. Consolidate daemon bootstrap resolution
   Development work:
   - move duplicated same-host daemon endpoint/binary resolution into one
     shared private utility consumed by both `atm` and `atm-graft`
   - keep bootstrap behavior unchanged

3. Reduce timing-sensitive test waits
   Development work:
   - replace fixed wait sleeps with structural synchronization where feasible
   - for hard-to-remove timing waits, add accepted-risk comments explaining
     why the wait remains intentional

## Acceptance Criteria

- `cargo test --workspace` passes
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc` passes
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc` passes
- `just lint` passes
- `git diff --check` is clean
- `with_poll_interval()` is narrowed or test-only
- `from_transport()` is narrowed or test-only
- redundant `fetch_pending_nudges()` aliases are removed
- daemon bootstrap path resolution is shared by `atm` and `atm-graft`
- timing-sensitive waits are reduced where feasible and explicitly justified
  where they remain
- no new public API surface is added
- no production behavior changes are introduced

## Required Validation

- `cargo test --workspace`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc`
- `just lint`
- `git diff --check`

## Risks And Watchouts

- do not widen `atm-graft` boundaries while deduplicating helpers
- do not preserve redundant convenience APIs just because tests use them today
- do not replace structural synchronization with different arbitrary sleeps
