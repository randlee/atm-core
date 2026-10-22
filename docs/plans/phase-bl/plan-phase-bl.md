# Phase bl plan index (anchor planning-gate input)

Sprint table for `plan_source.py`; the phase plan is [phase-bl-plan.md](phase-bl-plan.md).

| Sprint | Authoritative plan | Tracker | Relation | Closure |
| --- | --- | --- | --- | --- |
| bl-1 | [Phase BL contract](sprint-bl-1-phase-contract.md) | `-` | `-` | contract |
| bl-2 | [jev_client: request array in, typed JSONL out, busy-only retry](sprint-bl-2-jev-client.md) | `-` | `must_follow:bl-1` | boundary |
| bl-3 | [event dispatch](sprint-bl-3-event-dispatch.md) | `-` | `must_follow:bl-1` | boundary |
| bl-4 | [QA reviewer data: `metadata.reviewers` on the QA bead and on finding beads](sprint-bl-4-qa-reviewer-data.md) | `-` | `must_follow:bl-1` | boundary |
| bl-5 | [sanity state and close mechanics](sprint-bl-5-sanity-flow.md) | `-` | `must_follow:bl-1` | boundary |
| bl-6 | [dashboard render](sprint-bl-6-dashboard-render.md) | `-` | `must_follow:bl-1` | boundary |
| bl-7 | [Quality tab and Jev presentation](sprint-bl-7-quality-tab.md) | `-` | `must_follow:bl-1` | boundary |
| bl-8 | [collector implementation and snapshot builder](sprint-bl-8-collector-implementation.md) | `-` | `must_follow:bl-1` | boundary |
| bl-10 | [problem-source derivation](sprint-bl-10-problem-sources.md) | `-` | `must_follow:bl-1` | boundary |
| bl-12 | [Phase BL composition](sprint-bl-12-phase-composition.md) | `-` | `must_follow:bl-2,bl-3,bl-4,bl-5,bl-6,bl-7,bl-8,bl-10,bl-13` | integration |
| bl-13 | [DAG renderer](sprint-bl-13-dag-render.md) | `-` | `must_follow:bl-1` | boundary |
