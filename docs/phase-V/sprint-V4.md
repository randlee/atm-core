# Sprint V.4 — Sprint-Close Hygiene Gate

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.4"
status: planned
worktree: TBD
branch: TBD
estimated_scope: S
```

## Goal

Turn recurring sprint-close bookkeeping misses into an explicit gate before QA
handoff.

Carry-forward reference:
- `ATM-QA-PU-001` through `ATM-QA-PU-005` are the motivating Phase U findings
  for this sprint-close gate.

## Scope

- require sprint doc status updates before QA handoff
- require relevant plan index or project-plan updates before QA handoff
- define the minimum closeout checklist for a sprint branch:
  status, references, merge-forward notes, and validation state
- document whether enforcement is doc-check, lint, CI policy, or a combined
  gate

## Acceptance Criteria

- the required sprint-close documentation updates are explicitly listed
- QA handoff requirements are documented as a hard gate rather than a soft
  reminder
- the project-plan / sprint-doc relationship is clear enough that later phases
  do not drift silently
- no new product behavior is introduced

## Out Of Scope

- general project-management process changes outside sprint-close hygiene
- rewriting historical sprint docs unless needed to establish the rule
- non-document release engineering unrelated to the documented gate
