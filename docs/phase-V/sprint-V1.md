# Sprint V.1 — Observability Boundary And Event Model

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.1"
status: planned
worktree: TBD
branch: TBD
estimated_scope: M
```

## Goal

Define the final daemon observability boundary so observability sits at the
bottom of the stack and subsystem semantics stay in the owning subsystem.

Carry-forward reference:
- `RULE-002` / `ARCH-PU-002` from `docs/phase-U/sprint-U11.md` are the direct
  source findings for this sprint.

## Scope

- define the injected daemon observability trait shape
- define the daemon subsystem event model
- make `subsystem` explicit on daemon events
- make `team` and message-context fields per-event payload rather than injected
  logger state
- define the hard boundary rule:
  observability depends on no daemon subsystem types
- update daemon requirements, architecture, and boundary docs to the final
  ownership model

## Acceptance Criteria

- the final daemon observability boundary is documented
- observability is explicitly bottom-of-stack
- daemon subsystems depend on the injected observability trait, never the
  reverse
- `team`, `agent`, `sender`, `recipient`, `message_id`, and `task_id` are
  documented as per-event fields when relevant
- the final subsystem/event model is clear enough to implement without central
  daemon event reconstruction

## Out Of Scope

- code migration into subsystems
- deleting old mapping code
- general runtime failure recovery work outside observability
