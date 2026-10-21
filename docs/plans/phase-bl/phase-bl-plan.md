# Phase BL — atm-bd-orchestration 0.18.5

## Goal and authority

Phase bl implements the scoped 0.18.5 work with collectors → current JSON
state → debounced/aggregated event requests → separately scheduled ATM
delivery. No database is introduced. `design.md` contains Rand's verbatim
authority; deterministic controlled-input tests are required.

## Boundaries

| Boundary | Producer | Consumers | Artifact |
| --- | --- | --- | --- |
| Collectors | bl-1 | bl-3,6,7,10,12 | per-source records, stamps, locks |
| JevResult | bl-2 | bl-5,12 | JSONL and enum |
| EventRequests | bl-3 | bl-5,12 | debounced requests and delivery history |
| QaReviewers | bl-4 | bl-7,12 | reviewer metadata |
| SanityState | bl-5 | bl-7,12 | D5 state and event request |
| DashboardRender | bl-6 | bl-7,12 | persisted-state render |
| ConsoleApp | bl-7 | bl-12 | collector requests and tabs |
| BdDeadline | bl-8 | none until D1 | command seam |
| ProblemSources | bl-10 | bl-3,12 | source records and derivation |
| SanityMechanics | bl-11 | none | close/stack mechanics |

## Three waves

| Wave | Sprints | Hard artifact dependency |
| --- | --- | --- |
| 1 | bl-1, bl-2, bl-4, bl-8, bl-10, bl-11 | none; bl-8 remains D1-gated |
| 2 | bl-3, bl-5, bl-6, bl-7 | bl-3 ← bl-1 records; bl-5 ← bl-2 JSONL + bl-3 requests; bl-6 ← bl-1 aggregate; bl-7 ← bl-1 records + bl-4 metadata |
| 3 | bl-12 | bl-3 requests + bl-4 metadata + bl-5 sanity state + bl-7 display |

There are 11 sprints; maximum width is six in wave 1. Critical paths are
`bl-1 → bl-3 → bl-7 → bl-12` and `bl-2 → bl-5 → bl-12`.
`phase-bl.jsonl` and `.sprints/BL/structure.ttl` describe the same DAG.

## Decisions and integration exit

D1–D4 retain their existing ruled-work limits; D5 remains the bl-5 mechanical
selection rule. bl-12 proves Jev/reviewer inputs enter current state, the TUI
displays it, and transitions create requests; controlled delivery time proves
messages are independent of cron/TUI collection.
