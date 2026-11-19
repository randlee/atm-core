# task.close — `atm-bd-task-hook.py`

Closes the caller's own bead with evidence, or gives it back. At most one write. A completed re-close of an already closed bead revalidates the transition and returns only its close action, without a bead write; ATM checks that the task remains active. Reads: phase file, the bead, the ledger (stack bead), poured finding beads; refreshes the finding beads first. Writes nothing to the ledger; assigns nothing; creates nothing.

What the agent did before calling close is in its template's fenced blocks (dev: stack block; QA: pour block). The hook verifies the result of those blocks; it never performs them.

```mermaid
flowchart TD
    A([task.close]) --> P{phase task?}
    P -- no --> NA[not-applicable]
    P -- yes --> L{phase file is lite?}
    L -- no --> R0[refused PHASE_UNREADABLE / PROFILE_MISMATCH]
    L -- yes --> G{bead found, outcome valid?}
    G -- no --> R1[refused BEAD_NOT_FOUND / REQUEST_INVALID]
    G -- yes --> O{caller is assignee?}
    O -- no --> R2[refused TASK_NOT_ASSIGNEE]
    O -- yes --> S{status in_progress or closed?}
    S -- no --> R3[refused BEAD_CLOSED / BEAD_NOT_READY]
    S -- yes --> T{transition = outcome × stage}
    T -- GIVE_BACK --> RS{in_progress and reason present?}
    RS -- no --> R4[refused CLOSE_REASON_EMPTY]
    RS -- yes --> W1[write reopen, guarded] --> H1[handled: close --template refuse --vars reason]
    T -- WORK_CLOSE --> RP{report present, ledger has layer for work.branch?}
    RP -- no --> R5[refused CLOSE_REPORT_NOT_INLINE / STACK_NOT_RECORDED]
    RP -- yes --> W2[if in_progress write close with layer evidence] --> H2[handled: close --template dev-close --vars layer]
    T -- QA_PASS --> V1{verdict valid, sha == target head?}
    V1 -- no --> R6[refused QA_VERDICT / QA_NOT_ON_STACK / QA_NOTHING_TO_REVIEW]
    V1 -- yes --> W3[if in_progress write close with sha] --> H3[handled: close --template qa-pass --vars sha]
    T -- QA_FINDINGS --> V2{verdict valid, sha matches, findings refreshed,<br/>every blocking finding poured and blocking the sprint bead,<br/>every non-blocking one blocking the phase bead?}
    V2 -- no --> R7[refused QA_VERDICT / QA_NOTHING_TO_REVIEW / QA_NOT_ON_STACK / FINDINGS_NOT_POURED / CACHE_UNAVAILABLE]
    V2 -- yes --> W4[if in_progress write close with sha + finding ids] --> H4[handled: close --template qa-findings --vars sha, ids]
    T -- LAND --> V3{valid merged SHA?}
    V3 -- no --> R8[refused LAND_EVIDENCE]
    V3 -- yes --> W5[if in_progress write close with merge SHA] --> H5[handled: close --template land-close --vars sha]
```

## Transitions (one row per match arm)
| id | transition | outcome | stage | hook checks | write | returns |
|---|---|---|---|---|---|---|
| T-CLOSE-01 | GIVE_BACK | refused, cancelled | any | reason present | reopen: open, assignee '' (guarded) | `atm task close --template <refuse> --vars reason` |
| T-CLOSE-02 | WORK_CLOSE | completed | dev, fix | report present; ledger has a layer for `work.branch` | close, evidence = layer (branch, head, pr, base) | `atm task close --template <dev-close> --vars layer` |
| T-CLOSE-03 | QA_PASS | completed | qa, no findings | checked_bead present; verdict sha == target layer head | close, evidence = sha | `atm task close --template <qa-pass> --vars sha` |
| T-CLOSE-04 | QA_FINDINGS | completed | qa, findings | checked_bead present; sha matches; each blocking finding's poured bead exists and blocks the sprint bead; each non-blocking one blocks the phase bead | close, evidence = sha + finding ids | `atm task close --template <qa-findings> --vars sha, ids` |
| T-CLOSE-05 | LAND | completed | land | report contains one fenced JSON block with a 40-hex merged SHA | close, evidence = merge SHA | `atm task close --template <land-close> --vars sha` |

For T-CLOSE-02 through T-CLOSE-05, an already closed bead skips the write and returns the same validated close action. T-CLOSE-01 refuses a closed bead.

Target layer (T-CLOSE-03/04): the layer recorded for the bead named by the QA bead's `checked_bead`. If it is missing, refuse QA_NOTHING_TO_REVIEW with the QA bead id and the missing field before looking up the checked bead. Findings are matched to beads by the id the pour returned, listed in the QA report.

## Facts
| fact | lookup |
|---|---|
| profile, templates | phase file |
| bead status, assignee, stage, work.branch, stack_bead, blockers, checked_bead | cache.get(bead) |
| ledger layers | cache.get(stack_bead) |
| poured finding beads and their blocks | cache.refresh once (finding ids + sprint bead, + phase bead if any finding is non-blocking), then cache.get(id) per finding id in the report |
| outcome, report, reason, verdict | ATM request |

## Errors
| code | owner | fix |
|---|---|---|
| STACK_NOT_RECORDED | dev | "<branch> is not on the ledger. Run the stack block in your task, then close again." |
| CLOSE_REPORT_NOT_INLINE / CLOSE_REASON_EMPTY | dev | "Run `atm task close <bead> <outcome> --stdin` with the report (or reason)." |
| TASK_NOT_ASSIGNEE | dev | "<bead> is assigned to <agent>, not you." |
| BEAD_CLOSED | dev | "<bead> is already closed and cannot be given back." |
| BEAD_NOT_READY | dev | "<bead> is <status>; `atm task start` it first." |
| QA_VERDICT | qa | "End the report with one fenced block {\"verdict\": PASS or FINDINGS, \"sha\": …, \"findings\": [{\"id\": …, \"blocking\": true or false}]}." |
| LAND_EVIDENCE | lead | "Include one fenced JSON block with merged and a 40-hex merge SHA." |
| QA_NOT_ON_STACK | qa | "Your verdict is for <sha>; <branch> is at <head>. Review <head>, then close again." |
| FINDINGS_NOT_POURED | qa | "Findings <ids> are not poured or do not block <sprint>. Run the pour block, then close again." |
| QA_NOTHING_TO_REVIEW, REQUEST_INVALID, BEAD_NOT_FOUND, BEAD_RACED, PHASE_UNREADABLE, PROFILE_MISMATCH, CACHE_UNAVAILABLE | lead | "Not yours to fix: <what failed>. The lead has it." |
