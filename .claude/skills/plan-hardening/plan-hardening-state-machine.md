# Plan-Hardening State Machine

`team-lead` should be able to execute this flow by following the numbered
templates and passing the fenced JSON from one step to the next. The plan
details stay in the docs. `team-lead` routes steps and waits for the required
handoff artifact.

## Sequence

```mermaid
sequenceDiagram
    participant U as User
    participant TL as team-lead
    participant A as arch-ctm
    participant PSR as plan-scope-reviewer
    participant CPR as critical-plan-reviewer
    participant QM as quality-mgr

    U->>TL: Run /plan-hardening on current plan state

    TL->>A: 01-plan-scope-review.xml.j2
    A-->>TL: step-1 fenced JSON

    TL->>PSR: plan-scope-reviewer.md + step-1 JSON
    Note over TL,PSR: run_in_background: true
    PSR-->>TL: step-2 fenced JSON findings

    TL->>A: 02-sprint-scope-hardening.xml.j2 + step-2 JSON
    A-->>TL: step-3 fenced JSON

    TL->>CPR: critical-plan-reviewer.md + step-3 JSON
    Note over TL,CPR: run_in_background: true
    CPR-->>TL: step-4 fenced JSON findings

    TL->>A: 03-consistency-hardening.xml.j2 + step-4 JSON
    A-->>TL: step-5 fenced JSON

    TL->>TL: human critical review
    TL->>QM: focused plan QA + step-5 JSON
    QM-->>TL: QA verdict
```

## State Machine

```mermaid
stateDiagram-v2
    [*] --> CurrentPlanState
    CurrentPlanState --> Step1GuidelinesPass: 01 -> arch-ctm
    Step1GuidelinesPass --> Step2ScopeReview: step-1 JSON present
    Step1GuidelinesPass --> HardStop: step-1 JSON missing or malformed
    Step2ScopeReview --> Step3ScopeHardening: step-2 JSON present
    Step2ScopeReview --> HardStop: step-2 JSON missing or malformed
    Step3ScopeHardening --> Step4CriticalReview: step-3 JSON present
    Step3ScopeHardening --> HardStop: step-3 JSON missing or malformed
    Step4CriticalReview --> Step5ConsistencyHardening: step-4 JSON present
    Step4CriticalReview --> HardStop: step-4 JSON missing or malformed
    Step5ConsistencyHardening --> HumanReview: step-5 JSON present
    Step5ConsistencyHardening --> HardStop: step-5 JSON missing or malformed
    HumanReview --> FocusedPlanQA: plan still matches user-discussed scope
    HumanReview --> HardStop: material scope drift
    FocusedPlanQA --> ReadyForImplementation: QA pass
    FocusedPlanQA --> HardStop: QA fail
```

## Step Contract

1. `01-plan-scope-review.xml.j2`
   - send to `arch-ctm`
   - purpose: read the guidelines and make sure the plan follows them
   - output: `step-1` fenced JSON

2. `.claude/agents/plan-scope-reviewer.md`
   - launch `plan-scope-reviewer` with Agent tool
   - required input: `step-1` fenced JSON
   - execution: `run_in_background: true`
   - output: `step-2` fenced JSON findings

3. `02-sprint-scope-hardening.xml.j2`
   - send to `arch-ctm`
   - required input: `step-2` fenced JSON
   - output: `step-3` fenced JSON

4. `.claude/agents/critical-plan-reviewer.md`
   - launch `critical-plan-reviewer` with Agent tool
   - required input: `step-3` fenced JSON
   - execution: `run_in_background: true`
   - output: `step-4` fenced JSON findings

5. `03-consistency-hardening.xml.j2`
   - send to `arch-ctm`
   - required input: `step-4` fenced JSON
   - output: `step-5` fenced JSON

6. focused plan QA
   - send to `quality-mgr`
   - required input: `step-5` fenced JSON
   - output: QA verdict
