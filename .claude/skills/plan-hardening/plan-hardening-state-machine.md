# Plan-Hardening State Machine

This flow must be simple enough for a low-context `team-lead` to execute by
following the numbered templates in order. `team-lead` is a router, not the
planning authority. Plan details must come from the docs and references, not
from `team-lead` paraphrase.

## Routing Table

1. `team-lead -> plan-scope-reviewer`
   - render: `01-plan-scope-review.xml.j2`
   - input authority: current planning docs and references
   - output: fenced JSON findings

2. `team-lead -> arch-ctm`
   - render: `02-sprint-scope-hardening.xml.j2`
   - required input: fenced JSON from step 1
   - output: fenced JSON resolution report

3. `team-lead -> critical-plan-reviewer`
   - render: `03-critical-plan-review.xml.j2`
   - required input: fenced JSON from step 2
   - output: fenced JSON findings

4. `team-lead -> arch-ctm`
   - render: `04-consistency-hardening.xml.j2`
   - required input: fenced JSON from step 3
   - output: fenced JSON resolution report

5. `team-lead -> quality-mgr`
   - render: existing focused plan-QA template
   - required input: fenced JSON from step 4
   - output: QA verdict

## Sequence

```mermaid
sequenceDiagram
    participant U as User
    participant TL as team-lead
    participant PSR as plan-scope-reviewer
    participant A as arch-ctm
    participant CPR as critical-plan-reviewer
    participant QM as quality-mgr

    U->>TL: Run /plan-hardening on current plan state
    TL->>PSR: 01-plan-scope-review.xml.j2
    Note over PSR: Read plan docs directly
    PSR-->>TL: Fenced JSON findings

    TL->>A: 02-sprint-scope-hardening.xml.j2 + step 1 JSON
    Note over A: Create missing sprint docs, split sprints, tighten scope
    A-->>TL: Fenced JSON resolution report

    TL->>CPR: 03-critical-plan-review.xml.j2 + step 2 JSON
    Note over CPR: Review architecture, boundaries, false closure
    CPR-->>TL: Fenced JSON findings

    TL->>A: 04-consistency-hardening.xml.j2 + step 3 JSON
    Note over A: Fix contradictions, ambiguity, ADR/boundary drift
    A-->>TL: Fenced JSON resolution report

    TL->>TL: Human critical review against discussed scope
    TL->>QM: Focused plan QA + step 4 JSON
    QM-->>TL: QA verdict
```

## State Machine

```mermaid
stateDiagram-v2
    [*] --> CurrentPlanState
    CurrentPlanState --> ScopeReview: Step 1\n01-plan-scope-review.xml.j2
    ScopeReview --> ScopeHardening: Step 2\nstep 1 fenced JSON present
    ScopeReview --> HardStop: Step 1 output missing or malformed
    ScopeHardening --> CriticalReview: Step 2 fenced JSON present
    ScopeHardening --> HardStop: unresolved scope/split issues\nor malformed output
    CriticalReview --> ConsistencyHardening: Step 3 fenced JSON present
    CriticalReview --> HardStop: Step 3 output missing or malformed
    ConsistencyHardening --> HumanReview: Step 4 fenced JSON present
    ConsistencyHardening --> HardStop: contradictions remain\nor malformed output
    HumanReview --> FocusedPlanQA: scope still matches user discussion
    HumanReview --> HardStop: material scope drift
    FocusedPlanQA --> ReadyForImplementation: QA pass
    FocusedPlanQA --> HardStop: QA fail
```

## Invariants

- Every step is routed by `team-lead`.
- Every step after step 1 requires the fenced JSON output from the previous
  step.
- Missing or malformed fenced JSON is a hard stop.
- Material scope drift from what the user discussed is a hard stop.
- If `team-lead` must explain the plan for a step to succeed, the docs or
  prompts are not hardened enough.
