---
name: plan-anchor-questions
version: 0.6.1
description: The planning-gate Jev question set, what each question isolates, why the set is orthogonal, and how to word overrides.
---

# Planning-gate questions

The wording of record is `assets/questions/plan.json`; copy question text from there, not from here. The defaults must be proven adequate per anchor before a gate run (`question-tuning.md`).

## The state Jev sees

One anchor and one planning bead, nothing else. The bead text is its title, description and acceptance criteria (`compose-item --for plan`); design and notes are measured by `shape` but not sent (measured-behaviour.md):

```json
{
  "anchor": {"id": "ANC-012", "statement": "<verbatim>", "scope": "<where it applies>"},
  "item":   {"id": "bd-42", "title": "<bead title>", "text": "<bead description and acceptance criteria>"}
}
```

Unrelated material degrades accuracy, so the agent never sends several beads, the intent document, or other anchors. A bead is never shortened at this gate; one that does not fit is refused with `PLAN.TOO_LARGE` (`jev-api.md`, `plan-size-and-shape.md`).

## The three questions

| ID | Role | Asks | Isolates |
| --- | --- | --- | --- |
| `relevant` | relevance | Does the bead's work touch the subject the anchor talks about? | Scope. An anchor that does not govern this bead must not count as a failure |
| `addressed` | compliance | Does the bead explicitly carry the directive's specific content? | "Forgotten": the compression failure |
| `contradicted` | conflict | Does the bead propose something the directive rules out, or a different approach? | "Drifted": did something other than what was asked |

Each is a `choice` with `yes`, `no`, and `unclear`, so every answer carries a distribution and a confidence. `unclear` gives Jev a way out when neither option fits; without it an ambiguous bead forces a confident-looking wrong answer.

## What the defaults can and cannot do

The generic wording decided few cells on a real plan, mostly false passes; `relevant` limited as often as `addressed` (measured-behaviour.md). Before its first gate run each anchor gets its own `addressed` wording, naming the directive's concrete content and homonyms, and its own `relevant` wording listing bead-level signals (`question-tuning.md`). Prohibitions and structural anchors are not judged by `addressed` (`verdict-gating.md`).

## Why the questions are orthogonal

Jev answers the questions in one request independently; none sees another's answer. Each isolates one failure mode, and the combination happens in code (`verdict-gating.md`):

- `relevant` judges subject overlap only and says "do not judge whether the item obeys the anchor".
- `addressed` judges presence of the directive's specific content only and says "not whether the item conflicts".
- `contradicted` judges conflict only and says "silence is not a contradiction".

If `addressed` and `contradicted` were one question ("does the bead comply?"), a forgotten and a drifted anchor would get the same verdict, and the planning agent would not know whether to add a sentence or rewrite the approach.

## Wording rules for overrides

Jev answers the question you wrote, not the one you meant. When overriding in a plan-questions bead:

- One judgment per question. Do not fold two checks into one.
- Point at fields with backticks: `item.text`, `anchor.statement`. Avoid "the former" or "a property of a property".
- State scoping words, negations, and implied conditions explicitly. No double negatives.
- Keep `instructions` and `criteria` consistent; contradictions degrade answers.
- Describe situations in the criteria, not degrees ("the item names the legacy repo's crates as the source", not "mostly addresses").
- Keep the question IDs and the `yes`/`no`/`unclear` options so the gating code still applies.
- Never ask Jev to count, compare numbers, check dates, or look across beads.

A typical override: for "copy the legacy crates as the starting point", `addressed` names the content: "Does `item.text` state that the runtime crates are copied from the legacy repo rather than written new?"

## Example request (dry run)

```bash
python3 "$S" judge --anchors "$A" --gate plan --anchor ANC-012 \
  --item-id bd-42 --item-title "Sprint 2 runtime" --item-file "$W/item.txt" --dry-run
```

The run file's `requests[0]` is the exact body that would be sent and `totals.bytes` its size. Review it the first time an anchor is judged; run `check-fixtures --gate plan --id ANC-012` after changing wording.
