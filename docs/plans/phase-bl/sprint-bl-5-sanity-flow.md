# Sprint bl-5 — sanity state transitions and mechanical D5 selection

`BOUNDARY-SanityState`; wave 2; must follow bl-2 JSONL and bl-3 event requests.
Store Jev/LLM records unchanged, evaluate D5 from current state, and emit an
event request only; delivery remains scheduled in bl-3. It never couples
evaluation to collection timing or interprets verdict prose.

> "if jev-api failed (i.e. jev did not respond), the parallel llm evaluator's response is used. If both llm+jev respond a failure on either is failure on iteration #1. for iteration > 1, sanity-agent to triage (which should be done w/ background agent if sanity-agent has queue backlog)."

Controlled fixtures prove D5, preserved history, transition-to-request, and
absence of an ATM call before scheduled delivery.
