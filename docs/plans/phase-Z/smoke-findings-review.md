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
  "recommended_sprint": "Z.19",
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

## Current State

No promoted smoke findings are recorded on the accepted `Z.19` through `Z.21`
execution line.

Accepted smoke execution heads:

- `Z.19 @ fa36120d`
- `Z.20 @ a26b5e99`
- `Z.21 @ 5dbcd3c3`

Disposition summary:

- small logging, determinism, and report-surface gaps discovered while
  implementing `just smoke fast`, `just smoke`, and `just smoke thorough` were
  fixed inside the active execution sprints
- no validated smoke discrepancy remained large enough to require promotion
  into this major-rework queue

## Readiness And Binary Baseline Linkage

- `docs/plans/phase-Z/readiness.md` is the authoritative release-signoff ledger for
  accepted smoke execution heads and later canary/release verdicts.
- this queue records only promoted major smoke findings; when it remains empty,
  the corresponding readiness rows still carry the accepted smoke binary SHAs
  that were validated on the execution line:
  - `Z.19 @ fa36120d`
  - `Z.20 @ a26b5e99`
  - `Z.21 @ 5dbcd3c3`
- if a future smoke sprint promotes a larger finding, the promoted record in
  this queue must cite the originating smoke level and the accepted readiness
  row for the binary under test.
- canary (`Z.3`) and final release (`Z.4`) evidence must reference both:
  - the accepted readiness rows for the smoke execution heads
  - any promoted finding records in this queue that remain open or deferred
