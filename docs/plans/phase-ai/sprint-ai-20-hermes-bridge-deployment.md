---
id: AI.20
title: Hermes Bridge Deployment and Runbook
status: planned
branch: feature/pAI-s20-hermes-bridge-deployment
worktree: ../atm-core-worktrees/feature/pAI-s20-hermes-bridge-deployment
target: integrate/phase-AI
---

# Sprint AI.20 — Hermes Bridge Deployment and Runbook

## Goal

Deploy one AI.19 bridge per Hermes profile under launchd and publish a concrete
operator runbook. Launchd supervises a process; it never participates in
message identity or routing.

## Hard Dependencies

- AI.19 is `PASS`.
- The required Hermes profiles and their configuration are available to the
  operator.

## Deliverables

- Per-profile launchd plists for `default`, `grecon`, `alpha-prime`, and
  `skillrx`, each with its `ATM_TEAM`, `ATM_IDENTITY`, `HERMES_SESSION_KEY`,
  bridge configuration, and log path.
- A runbook covering install, status, restart, log collection, failure
  diagnosis, and adding a profile.
- Tests/probes proving independent supervision, restart after controlled
  termination, registered receiver availability, and correct qualified chat
  mapping for each profile.

## Exact Targets

- `docs/plans/phase-ai/hermes-graft-runbook.md` — install and recovery
  commands, expected logs, and proof commands.
- `~/Library/LaunchAgents/ai.hermes.atm-graft-{default,grecon,alpha-prime,skillrx}.plist`
  — operator-installed launchd plists retained as release evidence; templates
  live beside the runbook.

Every plist sets `ATM_TEAM`, `ATM_IDENTITY`, `HERMES_SESSION_KEY`, the bridge
module path, and an explicit profile log path. `KeepAlive` is allowed only for
the bridge process; it cannot restart or own the daemon. A readiness probe
must confirm the Hermes gateway is ready before the bridge activates.

## Boundary and Non-Goals

This sprint adds no daemon lifecycle control, no message routing policy, no
address parser, no transport protocol, and no cross-host feature.

## Closure

- Each profile starts and stops independently through launchd.
- The runbook reproduces the validation without hidden configuration.
- `just lint`, `just test`, and `git diff --check` pass for repository work;
  retained launchd evidence accompanies the operator-owned artifacts.
