# Sprint V.4 — Recovery Context Hardening

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.4"
status: planned
worktree: TBD
branch: TBD
estimated_scope: M
```

## Goal

Make daemon-unavailable and adjacent runtime failure paths consistently carry
actionable recovery guidance so system testing is diagnosable.

Carry-forward reference:
- `RBP-PU-001` disconnected-arm handling and `RBP-PU-002` `join_helper`
  recovery/context findings from the Phase U end-gate are concrete source
  evidence for this sprint.
- `QA-U-002` is the concrete daemon-unavailable recovery/backoff gap this
  sprint must close.

## Scope

- define which daemon/client/runtime errors must carry explicit recovery text
- add a checklist or lint strategy for `.with_recovery()` coverage on the
  required paths
- prioritize:
  - daemon unavailable
  - socket connect failures
  - daemon start failures
  - local IPC runtime failures
- document when recovery text is mandatory versus optional

## Acceptance Criteria

- the required daemon-unavailable error paths are explicitly enumerated
- `.with_recovery()` coverage is checked through a documented checklist, lint,
  or both
- required recovery text is specific and actionable rather than generic
- the resulting rule set is documented for future daemon and client work

## Out Of Scope

- redesigning the full ATM error model
- rewriting unrelated error messages with no daemon/runtime relevance
- process-only sprint-close hygiene
