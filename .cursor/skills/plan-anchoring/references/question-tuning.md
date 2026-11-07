---
name: question-tuning
version: 0.10.1
description: When and how to replace the generic Jev questions with anchor-specific wording, where the overrides live, how to test a question set with fixtures and known beads before trusting it, and when to stop tuning.
---

# Tuning the questions

The default sets (`assets/questions/plan.json`, `assets/questions/dev.json`) ask the same generic questions of every anchor; they are a starting point. Every anchor's plan-questions and dev-questions beads start as `{}` (defaults); an independent higher-reasoning evaluator tunes and tests wording when the defaults fail its intent-derived oracle.

## First: is the anchor testable?

Before tuning, run the rubric:

```bash
python3 "$S" rubric --anchors "$A" --id ANC-012            # a locked anchor
python3 "$S" rubric --statement "<candidate>" --quote "<words>" --scope "<scope>"   # before add
```

When an approved clarification is needed, explicitly select it with
`rubric --approved-intent-file <JSON>`. The file contains exactly
`{id, version, sha256, content}`; `sha256` hashes the UTF-8 content. The
integer or nonblank string version identifies the recorded approval packet.
The request includes this object as `state.intent`, alongside the unchanged
`state.anchor` quote, statement, and scope. The question must explicitly use
that context; supplying it cannot fix a question that restricts evidence to
the quote. Only already approved provenance belongs here. A valid hash proves
integrity, not human approval; never infer approval from a supplied file.

`rubric --questions <JSON>` selects an independently accepted calibration's
sealed `{questions, digest}` object. The digest hashes canonical JSON
`{questions}` (sorted keys, compact separators, UTF-8, no ASCII escaping).
It must contain exactly the five rubric role IDs, each a choice question with
instructions and yes/no/unclear criteria. The seal proves integrity, not
calibration acceptance: retain the independently accepted tournament result
and frozen oracle before selecting its seal. Neither option edits installed
assets; absent options retain the existing request and defaults. These flags
apply only to `rubric`; `add --rubric` retains its default instrument.

Both optional inputs must be regular UTF-8 JSON files under the actual git
repository containing `--repository-root`; relative paths start at that root.
Symlink escapes, duplicate JSON keys, malformed descriptors and mismatched
digests are refused before credential lookup or transport. Dry-run and fixture
modes remain offline. Live use still requires the repository's recorded
`allow_remote_judge: true`; context does not grant consent. Oversized requests
with explicit inputs fail before sending or emitting a partial request; intent
is never truncated.

Each context/custom-rubric entry records `inputs.question_seal`,
`inputs.request_sha256` (canonical JSON of the complete request before output
redaction), and optional `inputs.intent` ID/version/content hash. Dry-run output
uses the same presentation redaction as every emitted value: credentials and
bearer-shaped text, including synthetic examples, may become `<redacted>`.
Its displayed body can therefore differ from the canonical request. Preserve
the frozen validated inputs; when output is redacted, reconstruct the body from
those inputs, the selected anchor and model, then verify its canonical hash
against the live entry before triage. Do not treat the redacted display as the
original request or disable redaction. Live output retains hashes rather than
repeating the full intent.

It first runs deterministic requirement-form linting, then asks Jev five
semantic questions about the statement: one constraint, observable outcome,
faithful to the quote (no added or dropped requirement), scoped, and
self-contained. Requirement form is deliberately not a Jev question: syntax
and required fields are deterministic properties. A prohibition or explicit
condition can be a valid positive requirement; the rubric does not require a
literal token such as “must”.

Each semantic question is called twice with byte-identical request content. A
third call is made only when the first two choices disagree; it diagnoses the
instability and never acts as a majority vote. `testable` means every question
has two agreeing `yes` answers at confidence 0.6 or higher and the deterministic
lint has no errors. Any semantic `no`, `unclear`, or sub-threshold answer is
`needs_work`; agreeing low-confidence answers are `inconclusive`; any choice
disagreement is `unstable`. `add --rubric` refuses every non-testable outcome.
Fix the statement before tuning a plan or development question.

Before changing the rubric questions, delegate an `operation: testability`
request through `templates/anchor-tuning-testability-request.md.j2`. The
independent tuner freezes a plan-independent calibration corpus and its digest
before the first Jev response, exercises multiple candidate question sets, and
returns only a candidate whose result is accepted by the testability-result
contract. The corpus must cover each of the five semantic judgments, valid and
invalid requirement forms, and statement-level deduplication. Quote text is
provenance, not deduplication input.

`calibration_corpus.schema` and `version` identify the descriptor format, not
the referenced payload format. The shipped v2 descriptor references
`testability-calibration.v2.json`, whose frozen payload uses
`plan-anchor/testability-calibration-corpus/1` and has no `version` field. Verify
its canonical JSON SHA-256 and semantic case count; do not require these format
identities to match or relabel the frozen corpus. Its legacy expected role
`atomicity` corresponds to the rubric's `single_constraint` role.

For a disputed result on an approved anchor after calibration, delegate
`operation: triage`, `gate: testability` through
`templates/anchor-tuning-review-feedback.md.j2`. Pass the actual request and raw
response, the disputed rubric role ID, the five-role sealed rubric, and the
approved locked anchor and fixtures. The evaluator may return `anchor_defect`
with a clarity recommendation or `question_defect` with authority for reopening
the affected rubric tournament. An anchor defect keeps the rubric digest
unchanged. Insufficient evidence or low confidence alone produces
`EVIDENCE.INCOMPLETE`; adjudication never waives the rubric or confidence rule.
The plan-independent calibration corpus remains frozen and separate from this
single-anchor diagnosis.

Testability triage binds the exact sealed questions to the actual request and
all five valid raw answers. `review.source_digest` hashes the canonical JSON
`canonical_request.state.anchor` object (`statement`, `quote`, `scope`), which
must match a locked authority record with nonempty approved `pass` and `fail`
fixture arrays. The authority's record digest hashes canonical JSON records;
the intent digest hashes its UTF-8 `content`. Both result digest fields retain
the actual rubric seal: triage diagnoses, and a separate tournament changes
questions. Planning/development payloads retain their existing contracts.

When the actual request includes `state.intent`, triage requires its exact
descriptor to match `authority.intent`, including ID, version, content and
digest. Legacy requests without context remain accepted; the source-anchor
digest still hashes only `state.anchor`.

## Who tunes

The planner never tunes questions. It launches the harness-specific
`anchor-tuning` subagent (Claude: Opus; Codex: Sol) for each locked anchor and
gate. The tuner receives approved intent and anchor material, not a target plan
result. Before the first Jev call it uses judgment to derive expected role
answers and verdicts for approved fixtures, calibration examples, and boundary
probes, then freezes their text, labels, rationales, and digest. It may not
relabel or discard cases after seeing Jev. The skill writes an override only
after presenting the frozen oracle and acceptance table.

## When to tune

Tune only when a fixture or expected verdict approved before tuning demonstrates one of these. A live gate failure alone is evidence to inspect, not authority to change the measuring instrument:

| Symptom | Likely cause | Tune |
| --- | --- | --- |
| `underspecified` on the same anchor across several beads, `min_confidence` under 0.7 | The generic `addressed` question cannot tell whether the directive's specific content is present | Name the content in the question |
| `not_addressed` on a bead the user says clearly does the thing | The bead uses different vocabulary than the statement ("fork the legacy repo" vs "copy the crates") | List the acceptable phrasings in the `yes` criterion |
| `satisfied` on a bead that only mentions the topic | The question accepts topic mention as compliance | Tighten the `yes` criterion to the specific starting point, method or constraint |
| `relevant: yes` on unrelated beads, or `relevant` is the limiting question across many beads | `anchor.scope` is a topic sentence Jev does not use (measured-behaviour.md) | First rewrite `scope` as bead-level signals (`update --scope`), then override `relevant` to list those signals ("the item adds or changes a CLI subcommand", "the item edits a Cargo.toml") |
| `contradicted` on a bead that merely omits the directive | The conflict question reads silence as conflict | Add "silence is not a contradiction" examples to the `no` criterion |
| `evidenced: no` on a report that cites a diff | The evidence question does not recognise the artifact type the team produces | Name the artifact types in the `yes` criterion |

Do not tune to make a bead pass. The active plan's desired verdict is not an input. Before round 1, freeze approved pass/fail fixtures plus independent probes for compliant paraphrase, silence, topical-only mention, direct conflict, irrelevance, and any boundary or exception in the anchor; the development oracle also distinguishes assertion from evidence. Tune until every role answer and derived verdict matches this unchanged oracle. If the intent and anchor do not determine a label, report `ANCHOR.AMBIGUOUS` instead of tuning.

## Where overrides live

An override has the shape of the default set and contains only the questions to replace, keyed by the same IDs (`relevant`, `addressed`, `contradicted` for plan; `relevant`, `implemented`, `violated`, `evidenced` for dev), each keeping `yes`, `no`, `unclear`. IDs and options are fixed because `decide()` depends on them (`verdict-gating.md`).

- **Per anchor, per gate, in the question bead** (preferred): the fenced
  `json` block in the plan-questions or dev-questions bead
  (`bead-layout.md`). Use the tracker selected at the orchestration boundary:
  `bd update <id> --body-file <file>`.
- **In the record**: `question_overrides.plan` / `.dev` in `anchors.json`, for a plan with no beads. The bead wins if both exist.
- **For a whole plan**: a complete alternative set passed with `--questions <file>`. Rare.

## Extra questions

Extra questions add to the set; they never replace or gate. Pass a JSONL file with `--extra-questions <file>` (repeatable): one question per line, `{"id": "...", "type": "choice", "instructions": "...", "criteria": {"yes": "...", "no": "...", "unclear": "..."}}`. Each is asked in the same request as the gate questions and its answer is recorded with the cell, so triage can read why a cell failed (for example: does the item own the requirement, or only consume it?) without a second run. `decide()` ignores them. An `--answers-file` fixture may omit them; a missing extra answer is skipped, while a missing gate answer or an unknown id is still refused.

### Default triage questions

The planning gate adds five measured triage questions by default (`owns`, `specific`, `stated`, `paraphrase`, `partial`), with routing rules in `rules.json` and the evidence in the folder's `README.md`. The installer seeds them once into `.sc/plan-anchor/triage/` in the repository; the repository owns that folder: edit, extend or delete the files, and later installs never overwrite them. `judge` reads `.sc/plan-anchor/triage/plan-questions.jsonl` when present, else the copy shipped under `assets/triage/`. Pass `--no-triage-questions` to skip them. The development gate has no default triage questions. Add a new default only after measurements across plans; keep an unmeasured question in the consuming repository's own file.

## How to write an anchor-specific question

Start from the default and make it concrete. `instructions` say what to judge and what not to judge; `criteria` describe situations, not degrees.

Default `addressed`:

> Does `item.text` explicitly carry out, preserve, or plan for the directive given in `anchor.statement`? ...

A measured example: this override for a layering anchor corrected every false pass caused by "layer" meaning a stacked-PR position (measured-behaviour.md). It names the four layers, lists the vocabulary that counts, and calls out the homonym:

```json
{
  "questions": {
    "addressed": {
      "type": "choice",
      "instructions": "The anchor requires two things of a sprint bead: (1) the bead states which architectural layer its change lands in, and (2) that layer is the lowest one able to carry the change. The architectural layers, lowest to highest, are: (a) shared types and attributes (the types crate); (b) independent analyzer tool crates; (c) the CLI crate; (d) the adoption-kit template. Decide whether `item.text` names its architectural layer in this sense. The word 'layer' in `item.text` often means something else: a stacked-PR position ('planned layer 1', 'the layer-7 PR') or a consumer test layer; those uses do not name an architectural layer. ...",
      "criteria": {
        "yes": "The item text identifies the architectural layer (a)-(d) its change lands in, by letter, by layer name, or by stating that the named crate or kit is the lowest layer able to carry the change.",
        "no": "The item text does not identify an architectural layer: it names only crates, files or paths without calling any of them the layer, or uses 'layer' only for stacked-PR positions or test layers.",
        "unclear": "The item text mentions the layered architecture but it is not possible to tell which layer it claims the change lands in."
      }
    }
  }
}
```

The same shape, tuned for ANC-012 ("The runtime crates are created by copying the legacy repo's runtime crates and deleting the legacy-specific modules; no runtime crate is written from scratch."):

```json
{
  "questions": {
    "addressed": {
      "type": "choice",
      "instructions": "Does `item.text` state that the runtime crates start as a copy of the legacy repo's runtime crates (words such as copy, fork, vendor, import, port over, start from the legacy repo) with legacy-specific modules then removed? A plan to design or write a new runtime, even one that follows the legacy design notes, is not a copy. Judge presence of the copy-then-strip plan only; do not judge conflict.",
      "criteria": {
        "yes": "The item names the legacy repo as the source the runtime crates are copied from, and mentions removing or stripping legacy-specific parts.",
        "no": "The item does not say the runtime crates come from the legacy repo, or says they are written new, or is silent about where they come from.",
        "unclear": "The item mentions the legacy repo but it cannot be determined whether the crates are copied from it or merely modelled on it."
      }
    }
  }
}
```

Rules (Jev reads literally):

- One judgment per question. Two things to check is two anchors, not a longer question.
- Point at fields with backticks: `item.text`, `anchor.statement`.
- Spell out scoping words, negations and the "not this" cases; avoid double negatives and "the former".
- Put vocabulary variants in the criteria, not the instructions.
- Keep `instructions` and `criteria` consistent.
- Never ask Jev to count, compare numbers, or look across beads.
- Never put a confidence target, role-to-verdict mapping, or instruction to
  classify another role inside a production question.

## How to test a question set

Never deploy an override unread by Jev. Test in this order; each step is one command.

1. **Inspect the request.** `judge --anchor ANC-012 --item-file <known bead> --dry-run`, then read `requests[0].questions` in the run file. The override must appear verbatim and the other questions must still be the defaults.
2. **Fixtures.** With at least one `fixtures.pass` and one `fixtures.fail`
   sentence on the anchor (`update --fixture-pass/--fixture-fail`), run
   `check-fixtures --gate plan --id ANC-012` with the override in place.
   Expected raw roles and threshold-`0.0` semantic verdicts must match the
   frozen oracle; retain confidence against the `0.8` reference as a diagnostic.
   A semantic mismatch means the question is wrong unless the user agrees the
   fixture misstates the anchor.
3. **Known beads.** Judge one bead the user says clearly satisfies the anchor and one that clearly does not. Keep both runs.
4. **A/B against the default.** Run the same two beads with the default set (`--questions assets/questions/plan.json`, or move the override aside) and `diff` the pairs. The override must fix the symptom without flipping a correct verdict.
5. **Whole-gate regression.** Re-run the planning gate for all beads and `coverage`. Compare the uncovered list before and after.

Question-tournament acceptance uses protocol
`plan-anchor/question-tournament/2.0.0`: every raw role answer must match the
frozen oracle; the existing role-to-verdict mapping evaluated at threshold
`0.0` must produce the expected semantic verdict; repeated role tuples must be
identical; and regressions must be zero. A third call may diagnose disagreement
but cannot vote an unstable candidate into acceptance. Confidence is retained
as a distribution and ranking diagnostic after semantic correctness, not as a
tournament rejection by itself. The tuner cannot add, relabel, weaken, or
remove a probe after seeing an answer.

This does not change production review. Every required production answer below
the configured gate threshold (currently `0.6` by default) remains
inconclusive/failing. The separate `0.8` confidence reference only ranks
tournament candidates. Production review must not use the
threshold-free projection, average confidence, vote across repeats, or borrow a
tournament result.

Record the protocol version, oracle and question digests, semantic score,
confidence diagnostics, rationales, acceptance table, request/response digests,
and run IDs in an append-only canonical `plan-anchor/tuning-history/2` object.
An initial non-migration event (`event_index: 1`) uses `supersedes: null` when
no prior decision exists. Every `protocol_re_evaluated` event, including the
first, and every later event retains the predecessor object; the relational
contract reports `HISTORY.PREDECESSOR` when that object is absent.
Authority record `id` and history `anchor_id` preserve nonempty evaluator IDs,
including REQ/ADR source references and ANC IDs above 999. Source-specific
record validation remains the evaluator's responsibility.
Markdown and `bd` copies store byte-identical canonical JSON and SHA-256;
paths, tracker IDs/revisions, timestamps that differ by source, credentials, and
source kind remain outside it as local bindings. A v1 result can become a v2
seal only through digest-verified offline re-evaluation that preserves the
original decision and all raw evidence.

## Running a tournament

`jev_judge.py tournament` runs and scores a tune tournament; the anchor-tuning
agent never hand-rolls one (`jev-api.md`, "Batch runner and tournament").

```
python3 scripts/jev_judge.py tournament --request tune-request.json \
  --oracle oracle.json --candidates candidates.json --out <plan>/anchor-runs/tune-ANC-012-1
```

- **Request.** The anchor-tuning tune request. The runner checks it with
  `anchor_tuning_contract.py` and recomputes the authority digests before
  anything else; `testability` is not run by the runner.
- **Oracle** (`assets/schemas/tune-oracle.schema.json`). `cases[]` of
  `{case_id, probe, item: {title, text}, expected_roles, expected_verdict,
  rationale}` plus `authority_citations`. Every expected verdict must follow
  from its expected roles at threshold `0.0`, and the planning probes
  (`compliant`, `silent`, `topical_only`, `conflicting`, `irrelevant`,
  `boundary`, `exception`, plus `fixture_pass`/`fixture_fail` when the record
  has fixtures; development adds `asserted_without_evidence` and `evidenced`) must all be
  present. The runner freezes it in `--out` as `oracle.frozen.json` and refuses
  a different oracle there, or one that differs from the last event in the
  tuning history.
- **Candidates** (`assets/schemas/tune-candidates.schema.json`).
  `candidates.<role>.<candidate id>` is one question object per wording, at
  least `candidate_minimum` per role; `bundles[]` of `{bundle_id, roles:
  {<role>: <candidate id>}}` are the role-complete sets to evaluate (default
  the cross product, at most 64). The request's `current_questions` are the
  `baseline` bundle every regression is counted against.
- **Run.** Every oracle case x bundle x `repeat_count` becomes one request line
  of the low-layer `batch` runner (at most four parallel calls, one recorded
  retry each); every result line is logged, unchanged, to `<out>/results.jsonl`.
- **Score.** Bundles rank lexicographically: semantic acceptance, exact role
  cells, exact threshold-`0.0` verdicts, stable cases, fewer regressions, then
  confidence diagnostics (cells at or above the `0.8` reference, min, p10,
  median, mean), then complexity (smaller wins) and bundle id. The selected
  bundle is the top-ranked accepted one; with none accepted, the decision is
  `unstable`, `evidence_incomplete` or `rejected`.
- **Ties.** When two or more accepted bundles tie on every key except
  complexity and bundle id, the runner picks none: the decision is `rejected`
  with reason `TOURNAMENT.TIE_NEEDS_CHOICE`, the sealed-set fields are null,
  and `evidence.tie.co_leaders` lists each co-leader with its questions,
  score, confidence diagnostics and acceptance table. The anchor-tuning agent
  chooses one and seals it with `--results <out>/results.jsonl --choose
  <bundle_id>` into a new `--out`; no call is repeated.
- **Output.** `<out>/result_vars.json` holds the `anchor-tuning-result.md.j2`
  variables, already checked by `anchor_tuning_contract.py`. The runner never
  writes a bead: reading the result, choosing and sealing stay with the
  anchor-tuning agent. `--results <out>/results.jsonl` rescores the logged
  stream without calling Jev; lines are matched by request id, so their order
  does not matter.

To judge question beads directly, without an oracle, `bead-batch --anchor
ANC-012 --gate plan --item <bead> --candidates` runs the gate question bead
and each open child candidate bead against the items and streams one result
line per (item, candidate, repeat) to stdout and to
`anchor-runs/tournament-<run>.jsonl`.

## Testing a new anchor before the first gate run

Before any gate uses a freshly locked anchor: `lint` clean, fixtures written and approved, `check-fixtures` green with the defaults. Otherwise tune first; an untested question set produces findings nobody can act on.

## When to stop

- Stop when acceptance holds. Further specificity matches only today's wording of the bead.
- If `max_rounds` (default five) do not reach acceptance, the evaluator distinguishes an ambiguous or bundled anchor from a question set that cannot express the oracle. Return the diagnosis; the planner does not keep tuning. A prohibition judged by `addressed` fails on every compliant bead, while a structural anchor is a deterministic check rather than a planning-gate question (`anchor-capture.md`).
- Never lower the production threshold to obtain a pass. Report the `leaning`
  of any cell that remains `underspecified`, fix a genuine plan defect, and use
  evidence-bound `anchor-tuning triage` only for a plausible question defect.
- If Jev's answers on the fixtures swing between runs at the same wording, the fixtures are ambiguous; rewrite them with the user.

A tuned question is a plan artifact: it lives in the question bead, versions with the work, and travels with the anchor on promotion to an ADR.
