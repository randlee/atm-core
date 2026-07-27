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

Publish a parameterized launchd template and concrete operator runbook for one
AI.19 bridge per Hermes profile. A Hermes profile is an operator-registered
name plus its `ATM_TEAM`, `ATM_IDENTITY`, `ATM_CHAT_ID`, bridge
configuration, and log path. Launchd supervises a process; it never
participates in message identity or routing.

## Hard Dependencies

- Before AI.20 starts, the operator supplies the raw profile inputs out of
  band: profile name, `ATM_TEAM`, `ATM_IDENTITY`, `ATM_CHAT_ID`, bridge
  configuration, and log path for each intended bridge instance.
- AI.19 must be `PASS` before AI.20 deployment validation or closure; the
  narrower `FROZEN` record governs drafting only.

## Parallel Execution

AI.20 drafting may start only after the AI.19 `FROZEN` readiness record names
its exact commit and stabilized bridge module, configuration keys, and
readiness probe. If AI.19 changes that surface before `PASS`, AI.20 rebases
the draft to the replacement frozen record. Deployment validation and `PASS`
remain blocked on AI.19 `PASS`.

## Deliverables

- One parameterized `ai.hermes.atm-graft-PROFILE.plist` template, rendered
  once for each profile listed in the operator-provided registry.
- A runbook covering install, status, restart, log collection, failure
  diagnosis, and adding a profile.
- Tests/probes proving independent supervision, restart after controlled
  termination, registered receiver availability, and correct qualified chat
  mapping for every registry entry.

## Exact Targets

- `docs/plans/phase-ai/hermes-graft-runbook.md` — install and recovery
  commands, expected logs, proof commands, and the authoritative
  profile registry rendered from the operator-supplied inputs.
- `docs/plans/phase-ai/templates/ai.hermes.atm-graft-PROFILE.plist` —
  checked-in parameterized template. Operator-rendered plist files and their
  local paths are evidence, not repository targets.
- `scripts/phase-ai/run-hermes-bridge-probes.sh` and the
  `just verify-hermes-bridge-deployment <profile-registry-path>` recipe —
  launchd probes for every registry entry: independent supervision, restart
  after controlled termination, receiver availability, and qualified chat
  mapping.

Every plist sets `ATM_TEAM`, `ATM_IDENTITY`, `ATM_CHAT_ID`, the bridge
module path, and an explicit profile log path. Its ordered
`ProgramArguments` are a profile-owned gate executable, a Hermes-gateway
readiness executable, and the bridge runner. The gate must run the readiness
executable and `exec` the bridge runner only after zero exit. `KeepAlive` is
allowed only for the bridge process; it cannot restart or own the daemon.

## Boundary and Non-Goals

This sprint adds no daemon lifecycle control, no message routing policy, no
address parser, no transport protocol, and no cross-host feature.

## Closure

- Each registry profile starts and stops independently through launchd.
- The runbook reproduces the validation without hidden configuration.
- `just verify-hermes-bridge-deployment <profile-registry-path>` passes for
  the recorded operator registry; `just test` alone is insufficient for the
  launchd probes.
- `just lint`, `just test`, and `git diff --check` pass for repository work;
  retained launchd evidence accompanies the operator-owned artifacts.

## Draft Validation Limit

The operator profile registry is not yet available and AI.19 is `FROZEN`, not
`PASS`. This sprint closes the checked-in deployment material only; it cannot
run active launchd probes or claim AI.20 `PASS` until both prerequisites are
met.
