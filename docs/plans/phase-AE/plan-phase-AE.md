---
title: Phase AE Plan
status: planned
branch: plan/phase-AE
worktree: /Users/randlee/Documents/github/atm-core-worktrees/plan/phase-AE
---

# Phase AE Plan

## Goal

Close the four active Phase AE issues and establish the ATM v1.3 stable baseline as
the foundation for harness integration (schook plugin, sc-mux consolidation, atm-graft
support) and market readiness.

Phase AE follows Phase AD's corrective simplification. Where AD removed dead code and
restored correct runtime behavior, AE adds the missing features and release quality
needed before ATM can serve as a stable dependency for schook, sc-mux, and external
users.

## Baseline

- planning branch: `plan/phase-AE`
- execution integration branch: `integrate/phase-AE`
- prerequisite accepted line:
  - Phase AD closed (all 11 sprints merged, QA pass)
  - current `develop` as of 2026-07-04

## Active Issues

| # | Title | Category |
|---|---|---|
| 423 | `atm teams remove-member` subcommand | Missing feature |
| 448 | Send pipeline: CLI-local inputs, --stdin, payload-format | Missing feature |
| 435 | Preflight: crate metadata for crates.io | Release quality |
| 461 | pi-agent-atm + codex-atm as atm-graft harnesses | Integration |

Issues addressed by Phase AD (close when AD lands):
- #421 Daemon-mediated identity
- #440 Post-send notification silent failure

## Design Rules

Phase AE builds on AD's corrected foundation. The governing rules are:

- all caller-context rules from AD remain in force
- new features must use the shared caller-context resolver from AD.1
- new CLI surfaces must fail closed on missing caller context
- post-send behavior remains event-driven via the AD.6-8 emitter contract
- `atm-graft` integration is additive — it registers new emitter implementations
  under the existing `PostSendHookEmitter` trait

## Scope Rules

Phase AE may:

- add the `atm teams remove-member` subcommand with full caller-context enforcement
- fix the send pipeline to resolve CLI-local inputs before IPC dispatch
- add preflight validation ensuring all publishable crates have required metadata
- register pi-agent-atm and codex-atm as supported atm-graft harness implementations
- add or update smoke tests covering new CLI surfaces
- update doctor diagnostics for new subcommands

Phase AE must not:

- change the post-send emitter contract from Phase AD
- introduce new daemon subsystems or queue/worker patterns
- add features not directly tied to the four AE issues
- change the shared backend contract

## Scope Deferred to Later Phases

Phase AE explicitly defers:

- schook ATM plugin v2 (daemon RPC, ATM DB settings) → Phase AF
- sc-mux ↔ ATM DB consolidation → Phase AF (study in parallel)
- beads alchemy integration (handoff, context injection) → Phase AF
- website, onboarding tooling, documentation → Phase AG
- Windows code-signing (#90) → backlogs
- `atm task` subcommand design (#19) → backlogs

## Execution Order

Phase AE sprints execute in dependency order:

1. [AE.1 `atm teams remove-member` Subcommand And Send Pipeline Fix](./sprint-AE1.md)
2. [AE.2 Preflight Crate Metadata Validation](./sprint-AE2.md)
3. [AE.3 pi-agent-atm And codex-atm atm-graft Harness Support](./sprint-AE3.md)
4. [AE.4 Smoke Closeout And Release Readiness](./sprint-AE4.md)

Sprints execute back-to-back in merge-forward order:
`AE.1 → AE.2 → AE.3 → AE.4`

## Phase Exit Criteria

Phase AE closes when:

- `atm teams remove-member <team> <name>` removes a member from the roster
- `atm send --stdin` and `atm send --file <path>` resolve payloads at the CLI before IPC
- `cargo publish --dry-run` succeeds for every publishable crate
- pi-agent-atm is a registered atm-graft harness that can receive post-send nudges
- codex-atm is a registered atm-graft harness that can receive post-send nudges
- smoke coverage proves all new CLI surfaces on the accepted line
- `cargo test --workspace` passes
- `cargo clippy --workspace -- -D warnings` passes
- `python3 .just/run_lint.py all` passes
