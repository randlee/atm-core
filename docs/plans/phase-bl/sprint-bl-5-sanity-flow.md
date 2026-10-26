# Sprint bl-5 — sanity record: Jev JSONL consumer and mechanical D5 selection

closure_type `boundary`; target_boundary `BOUNDARY-SanityRecord`. Branch
`sprint/bl-5-sanity-flow`, PR → `sprint/bl-11-sanity-mechanics`. Wave 2.
`external_ref`: `gh-365,gh-497,gh-498,gh-126`.

## Contract and dependency

`must_follow` bl-2. This sprint consumes `jev_client.py --requests`, ordered
JSONL response lines (`success`, `data`, `receipt`, `error.kind`, `retries`,
`elapsed_s`), and the closed `JevErrorKind` enum. It never parses message text
or an evaluation choice to direct behavior.

Rand's ruled D5 is implemented mechanically:

> "if jev-api failed (i.e. jev did not respond), the parallel llm evaluator's response is used. If both llm+jev respond a failure on either is failure on iteration #1. for iteration > 1, sanity-agent to triage (which should be done w/ background agent if sanity-agent has queue backlog)."

## Scope and owned paths

This is the Jev consumer and history boundary. It owns:

- `agents/dev-sanity.md`, `templates/dev-sanity-template.xml.j2`, and
  `templates/sanity-run-record.json.j2` plus their examples;
- `scripts/sanity-merge`, `scripts/sanity-run-history`, and `scripts/assignment-gates.py`;
- `scripts/tests/test_sanity_merge.py`, `test_sanity_run_history.py`,
  `test_templates.py`, `test_handoff_contracts.py`, `test_assignment_gates.py`,
  and the distinct `test_dev_sanity_prompt.py`.

It does not own task close, rebase linking, or stack mechanics; bl-11 owns
those files and their tests. `assignment-gates.py` is part of this record
consumer because it consumes the rendered dev-sanity state. It does not edit bl-2's
`test_jev_client_prompts.py`.

## Public signature and stored data

```python
def select_iteration(*, iteration: int, deliverable_id: str,
                     jev_lines: Sequence[JevLine], llm: LlmReply) -> SelectedEvaluation | IterationFailure | TriageRoute: ...
```

```json
{"iteration": 1, "jev_results": [{"success": false, "error": {"kind": "TRANSPORT"}}], "llm": {"success": true}, "selection": "llm"}
```

The sanity record stores the LLM-authored request array and each parsed Jev
JSONL line in order and unchanged. It sums only numeric `elapsed_s` and
`retries`; it validates `error.kind` against the closed enum. At iteration 1,
an absent Jev evaluation uses the parallel LLM response; when both return,
a failure on either fails. Later iterations route to triage as D5 says.

## Deterministic criteria

- A controlled JSONL fixture is written to and read from sanity history
  byte-for-byte, with no aggregate client envelope.
- Controlled fixtures cover D5's LLM fallback, iteration-1 failure, and
  later-iteration triage selection without interpreting a verdict.
- The rendered dev-sanity prompt writes an LLM-authored request array, runs
  its controlled bootstrap before the assignment gate, and never changes the
  client response.
- Old records without the new arrays remain readable.
