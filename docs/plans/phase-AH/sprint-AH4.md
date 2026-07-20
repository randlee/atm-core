---
id: AH.4
title: Hermes Launchd Bridge Processes + Operational Runbook
status: planned
branch: feature/pAH-s4-launchd-bridge-deployment
worktree: ../atm-core-worktrees/feature/pAH-s4-launchd-bridge-deployment
target: develop
---

# Sprint AH.4 — Hermes Launchd Bridge Processes + Operational Runbook

```yaml
plan_type: sprint_plan
phase: AH
sprint: AH.4
worktree: ../atm-core-worktrees/feature/pAH-s4-launchd-bridge-deployment
branch: feature/pAH-s4-launchd-bridge-deployment
status: planned
estimated_scope: small
```

## Goal

Add per-profile launchd plists for the Hermes graft-bridge process so each
Hermes profile's bridge starts alongside its gateway and is supervised by
launchd. Produce an operational runbook for the new processes.

This sprint closes the deployment story for the atm-graft + Hermes
integration. AH.3 proved the protocol; AH.4 ships it on the host with
proper launchd supervision and documentation.

## Hard Dependencies

- AH.3 is `PASS` — the bridge process + webhook adapter integration is
  verified
- Hermes Agent 0.17.0+ (or the version that has the webhook adapter change
  from AH.3) on all four Hermes profiles (`default`, `grecon`,
  `alpha-prime`, `skillrx`)

## Exact Targets

- `~/Library/LaunchAgents/ai.hermes.bridge.plist` (default profile)
- `~/Library/LaunchAgents/ai.hermes.bridge-grecon.plist`
- `~/Library/LaunchAgents/ai.hermes.bridge-alpha-prime.plist`
- `~/Library/LaunchAgents/ai.hermes.bridge-skillrx.plist`
- Hermes gateway config template (or env-driven flag) to auto-enable the
  webhook platform on loopback when the bridge is configured per-profile
- Hermes profile env files updated with `ATM_TEAM` / `ATM_IDENTITY` /
  `HERMES_SESSION_KEY` for the bridge
- operational runbook at `docs/plans/phase-AH/hermes-integration-runbook.md`

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims.

- four Hermes launchd plists:
  - `ai.hermes.bridge{,-grecon,-alpha-prime,-skillrx}.plist`
  - each plist:
    - starts the bridge process after the corresponding Hermes gateway
      starts (via a startup delay or readiness probe)
    - sets `ATM_TEAM="hermes"`, `ATM_IDENTITY=<profile>`, and
      `HERMES_SESSION_KEY` in its process environment (profile-specific
      values per plist)
    - logs to `/Users/randlee/.hermes/logs/bridge-<profile>.log`
    - restarts on crash via launchd `KeepAlive = true`
    - shuts down cleanly on launchd unload
    - stops cleanly when the corresponding Hermes gateway stops (a
      lifecycle coupling that is either a launchd dependency or a
      `KeepAlive` condition in the plist)
- Hermes gateway config change ensuring the webhook platform is enabled on
  loopback when the bridge is configured; documented in the runbook as a
  required one-time setup per profile
- operational runbook covering:
  - how to install and verify the bridge processes are running
  - how to start/stop/restart a bridge process for a specific profile
  - how to view bridge logs
  - how to diagnose "bridge up but nudge not arriving at Hermes" failures
  - how to recover after a Hermes gateway restart
  - how to add a new Hermes profile to the bridge set
- acceptance test coverage:
  - bridge starts with launchd
  - bridge registers with atm-daemon after gateway is up
  - bridge shuts down cleanly when launchd stops the corresponding gateway
  - bridge restarts automatically after a crash (simulated kill)
  - atm-daemon routes nudges to the correct bridge for each profile

## Required Work

### Launchd plists

Each plist follows the existing pattern used by the Hermes gateway plists:

- `ProgramArguments` pointing to the bridge binary (or `python3` with the
  bridge script, depending on AH.3's shape decision)
- `EnvironmentVariables` with `ATM_TEAM`, `ATM_IDENTITY`,
  `HERMES_SESSION_KEY`, and the profile-specific webhook port
- `KeepAlive` configuration so the bridge restarts on crash and shuts down
  with launchd
- `StandardOutPath` / `StandardErrorPath` pointing to
  `~/.hermes/logs/bridge-<profile>.log`
- explicit `WorkingDirectory` set to the profile's home-dir

### Hermes config

The Hermes webhook platform must be enabled per-profile on loopback. This
sprint adds:

- a config flag or env var that turns on the webhook platform for
  loopback-only binding when a bridge is configured
- documented one-time setup in the runbook for operators

### Runbook

The runbook at `docs/plans/phase-AH/hermes-integration-runbook.md` is the
authoritative operational document. It lives inside the phase-plan directory
until the work is stable; once stable, it may move to
`docs/atm-graft/hermes-integration-runbook.md`.

The runbook must be executable by a `hermes-operator` role without hidden
Hermes-side knowledge. Every command and config edit is concrete.

### Non-Closure

This sprint does not:

- validate any end-to-end story (AH.5)
- change Hermes Telegram/Discord channel behavior
- validate cross-host delivery (out of scope for all of AH)

## Acceptance Criteria

- each of the four Hermes profiles has a launchd plist that starts the
  bridge alongside its gateway
- the bridge processes for each profile are independently supervised
  (one profile's bridge crashing does not affect other profiles)
- the runbook is concrete enough that an operator with no prior knowledge
  of the setup can install and validate it
- the bridge restarts after simulated crash within a bounded time
  (measured in the acceptance test)

## Required Validation

- `launchctl load` for each plist starts the bridge successfully
- `launchctl list | grep hermes.bridge` shows the bridge processes running
- manual kill of a bridge process → launchd restarts it within the
  acceptable window (measured, target ≤30s post-crash)
- bridge registration with atm-daemon observable via `atm doctor`
- end-to-end nudge path works for each profile after launchd start
