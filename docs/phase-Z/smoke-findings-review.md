# Phase Z Smoke Findings Review

## Purpose

Authoritative review queue for smoke findings that are too large to fix inside
the active smoke execution sprint.

This artifact exists to separate:

- minor smoke-blocking fixes that should be corrected inside `Z.19`, `Z.20`, or
  `Z.21`
- larger rework items that need explicit follow-on planning and review

## Finding Schema

Each finding entry must record:

- `finding_id`
- `smoke_level`
- `flow_or_command`
- `observed_behavior`
- `expected_behavior`
- `root_cause`
- `disposition`
- `recommended_sprint`
- `owner`
- `notes`

## Disposition Rules

- `fix-in-active-sprint`
  - use when the issue is small, local, and needed to make the smoke run
    predictable
- `promote-follow-on`
  - use when the issue is larger rework and should not be silently absorbed by
    the active smoke sprint

## Initial State

No promoted smoke findings are recorded yet on the planning line.
