# Phase Z Smoke Findings Review

## Purpose

Authoritative review queue for smoke findings that are too large to fix inside
the active smoke execution sprint.

This artifact exists to separate:

- minor smoke-blocking fixes that should be corrected inside `Z.19`, `Z.20`, or
  `Z.21`
- larger rework items that need explicit follow-on planning and review

## Canonical Finding Record

Each promoted finding entry must use this canonical serialization format:

```json
{
  "finding_id": "SMOKE-FIND-001",
  "smoke_level": "fast",
  "flow_or_command": "atm send --requires-ack z1-recipient \"hello\" --json",
  "observed_behavior": "send succeeded but retained logs omitted delivery_policy.new_message.primary_nudge",
  "expected_behavior": "successful ack-required send emits the primary_nudge event in retained logs",
  "root_cause": "smoke-fast debug logging path does not emit the expected nudge event on the durable happy path",
  "disposition": "promote-follow-on",
  "recommended_sprint": "Z.22",
  "owner": "atm-dev",
  "notes": "capture retained log artifact path and follow-on logging fix recommendation"
}
```

## Disposition Rules

- `fix-in-active-sprint`
  - use when the issue is small, local, and needed to make the smoke run
    predictable
- `promote-follow-on`
  - use when the issue is larger rework and should not be silently absorbed by
    the active smoke sprint

## Initial State

No promoted smoke findings are recorded yet on the planning line.
