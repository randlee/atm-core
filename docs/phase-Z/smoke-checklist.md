# Phase Z Smoke Checklist

## Purpose

Authoritative executable smoke matrix for `Z.1` and `Z.2`.

This checklist is frozen at the start of `Z.1`, executed on the approved
real-binary baseline, and rerun without scope drift in `Z.2`.

## Record Schema

Each row must record:

- `flow_id`
- `operator_flow`
- `command_or_entrypoint`
- `expected_result`
- `recovery_or_corner_case`
- `z1_verdict`
- `z2_revalidation_verdict`
- `notes`

## Required Flow Coverage

The frozen checklist must include at least:

- daemon bring-up on the approved baseline under test
- retained CLI command coverage that `Phase Z` treats as ship-critical
- one recovery path for each operator flow where the command or daemon can fail
- one negative-path or corner-case exercise per feature area claimed by `Z.1`
- explicit coverage rows for these retained recovery/corner-case categories:
  - daemon startup / readiness failure or degraded-start behavior
  - notification delivery failure or degraded-notification behavior
  - reconcile interruption, shutdown, or retry-visible behavior
  - retained CLI command error reporting and operator recovery guidance

## Ownership

- created and frozen in `Z.1`
- rerun without widening or narrowing in `Z.2`
- any proposed scope change after `Z.1` freeze must be documented as a separate
  plan correction, not silently edited into the active checklist
