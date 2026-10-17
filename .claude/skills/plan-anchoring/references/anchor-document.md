---
name: anchor-document
version: 0.6.0
description: The intent document, the anchors.json record of truth and its rendered anchors.md, ID rules, provenance, versioning, promotion, and retirement.
---

# The anchor documents

Two documents in `doc/plan/phase-current/` are for people; one machine record feeds everything else. Keep all of them in git with the plan.

| File | Who edits | Purpose |
| --- | --- | --- |
| `intent.md` | you, from `assets/templates/intent.md` | One page: purpose, success, out of scope. Frames the anchors; never judged |
| `anchors.json` | `scripts/jev_judge.py` only (`add`, `update`, `retire`, `promote`, `set-beads`) | Record of truth: every anchor with provenance, status, overrides, fixtures, bead IDs, history |
| `anchors.md` | nobody; rendered by `render` | Human view of `anchors.json`; re-render after every change |
| `anchor-runs/*.json` | `judge` (dry runs included; `check-fixtures` under `fixtures/`) | One file per gate run: answers, attempts, bytes sent, a hash of the questions asked; diffable, countable |

## anchors.json

```json
{
  "schema": "plan-anchoring/anchors/1",
  "plan": "phase-current",
  "intent_ref": "doc/plan/phase-current/intent.md",
  "anchors": [ { ...anchor record... } ]
}
```

Anchor record fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | `ANC-NNN` allocated by the script, or the register ID for REQ/ADR (`REQ-014`, `ADR-0007`) |
| `source_type` | `ANC` / `REQ` / `ADR` | Where the fact comes from |
| `source_ref` | string or null | The register ID for REQ/ADR; null for ANC (promotion records the ADR in `promoted_to`) |
| `statement` | string | The testable requirement Jev judges; orchestrator-written, user-approved. Reworded with `update --statement` (history keeps the old text); refused if identical to the quote |
| `quote` | string | The user's verbatim words; quoted in every failing verdict. Immutable. Required for ANC; the register text for REQ/ADR |
| `stated_by`, `stated_at`, `context` | string | Who, ISO date, short setting ("planning session", the register name). Source words belong in `quote`; move any "source words: ..." found in `context` there |
| `summary` | string | Short label for tables; never judged |
| `scope` | string | Where the anchor applies; sent as `anchor.scope` for the relevance question |
| `deterministic_check` | string | Code-level check paired with the development gate |
| `fixtures` | `{pass: [], fail: []}` | Sentences that would satisfy or violate the anchor |
| `question_overrides` | `{plan?: {...}, dev?: {...}}` | Optional wording overrides, same shape as a question-set file (`questions`, optional `roles`) |
| `version` | int | Bumped when statement, scope, summary, fixtures, overrides, or status change. Three statement rewordings mean retire and re-lock |
| `status` | `proposed` / `locked` / `promoted` / `retired` | Only `locked` and `promoted` are judged by `--all` |
| `promoted_to` | string or null | The ADR an anchor was promoted to |
| `beads` | `{anchor, plan_questions, dev_questions}` | Bead IDs of the anchor tree |
| `history` | list | Append-only events with timestamps |

Full example: `assets/examples/anchors.example.json`. Run `validate` after any change; it fails on structural problems and warns when `intent_ref` is missing or no anchor is locked. `judge` refuses a record with problems. `stated_at` defaults to today.

## IDs

- `ANC-NNN` is max existing plus one, zero-padded to three digits; never reused, even after retirement.
- REQ and ADR anchors use their register ID, so `bd-42 · REQ-014 · not addressed` points at the register.
- Bead IDs are recorded on the anchor, not the reverse; the record is the single place to look.

## Lifecycle

```
proposed  -->  locked  -->  promoted (keeps ID, gains ADR ref)
    \             \
     \             -->  retired (directive changed or withdrawn; new anchor replaces it)
      -->  retired (never confirmed)
```

- **proposed**: survives compression; not judged.
- **locked**: judged at both gates; requires the user's confirmation.
- **promoted**: `promote --id ANC-012 --adr ADR-0007`. Keeps ID and history; the ADR register gains the decision. Still judged.
- **retired**: `retire --id ANC-012 --replaced-by ANC-020 --reason "..."`. Not judged; replacement and reason land in `history`.

## Why the quote is immutable and the statement is not

The quote is evidence. The statement is an instrument: `lint`, `rubric` and the question tuner may send it back for rewording. Every run file copies the statement it judged, so rewording never changes what a past verdict meant. A changed directive is a different fact: new anchor, old one retired with `replaced_by`, both visible in `anchors.md`.

## Rendering

`render` writes `anchors.md`: a status table and one section per anchor (quote as blockquote, provenance, summary, scope, deterministic check, bead IDs, fixtures). Re-render after every `add`, `update`, `set-beads`, `promote` or `retire`; commit both files together.
