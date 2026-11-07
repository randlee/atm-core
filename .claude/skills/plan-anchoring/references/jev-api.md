---
name: jev-api
version: 0.9.2
description: How jev_judge.py talks to TypeSafe's Jev - endpoint, auth, request and response shape, validation, limits, the retry and context-shortening policy, every error code and flag, model pinning, and the committed allow_remote_judge setting that gates every call.
---

# Jev API as used by this skill

Jev is TypeSafe's decision model: it answers typed questions about a piece of state with probabilities and confidence, in parallel, and generates no text. `scripts/jev_judge.py` implements everything below; this document is for troubleshooting, not for hand-rolling requests.

## Remote judging consent (read first)

A gate run sends the anchor statement and scope and the item text (a plan bead or an account of delivered work) off the machine. `rubric` also sends the verbatim quote; `dedupe --jev` sends pairs of statements and quotes. Sending a plan to TypeSafe is the user's decision, so the script fails closed:

- `TYPESAFE_API_KEY`, when present, must be well-formed (non-empty, ASCII, no control characters); when it is absent the script tries the documented platform credential store, and
- the repository's committed `.sc/plan-anchor/settings.yaml` must set `allow_remote_judge: true`. `false` is the user's no; an absent resolved credential means the live call is refused. Both are refused.

Without both, `judge`, `check-fixtures`, `rubric`, `add --rubric` and `dedupe --jev` return `AUTH.CREDENTIALS` or `POLICY.REMOTE_JUDGE_DISABLED` and send nothing. The answer is recorded once per repository, never per session: ask the user, then `python3 scripts/plan_anchor_settings.py set allow_remote_judge true|false` (or the installer's `--allow-remote-judge` / `--no-remote-judge`), and commit the file. Only the user decides it: agents never write it and never infer it from existing run files or past runs. The setting is found from `--repository-root` (default `.`) by the same lookup as `anchor_tracker.py`; reading a present settings file needs PyYAML (`DEPENDENCY.PYYAML`, exit 2, otherwise), and nothing else in `jev_judge.py` does. `--dry-run` writes the exact request bodies and `totals {requests, bytes}` to the run directory. `--answers-file` runs the gating with fixture answers, offline. The key is never printed, logged, written to a run file, or included in an error.

## Endpoint and auth

- `POST https://api.typesafe.ai/v1/systemone`
- `Authorization: Bearer $TYPESAFE_API_KEY`, `Content-Type: application/json`

## Request

```json
{
  "model": "jev-latest",
  "state": {
    "anchor": {"id": "ANC-012", "statement": "...", "scope": "..."},
    "item": {"id": "ex-42", "title": "...", "text": "..."}
  },
  "questions": {
    "relevant":     {"type": "choice", "instructions": "...", "criteria": {"yes": "...", "no": "...", "unclear": "..."}},
    "addressed":    {"type": "choice", "instructions": "...", "criteria": {"yes": "...", "no": "...", "unclear": "..."}},
    "contradicted": {"type": "choice", "instructions": "...", "criteria": {"yes": "...", "no": "...", "unclear": "..."}}
  }
}
```

- `state` may be a string, object, or array; questions reference fields with backticked paths such as `item.text`.
- `questions` is keyed by your own IDs; the `instructions` are sent, the ID is not.
- Types: `choice` (criteria map option to description; up to 255 options), `score` (2 to 10 ordered level descriptions), `noul` (probability of yes, no confidence). This skill uses `choice` so every answer carries a confidence.

## Response

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "relevant":     {"type": "choice", "choice": "yes", "probabilities": {"yes": 0.96, "no": 0.03, "unclear": 0.01}, "confidence": 0.94},
    "addressed":    {"type": "choice", "choice": "no",  "probabilities": {"yes": 0.08, "no": 0.88, "unclear": 0.04}, "confidence": 0.85},
    "contradicted": {"type": "choice", "choice": "no",  "probabilities": {"yes": 0.30, "no": 0.62, "unclear": 0.08}, "confidence": 0.55}
  },
  "usage": {"input_tokens": 410, "output_tokens": 9}
}
```

`probabilities` says which option; `confidence` says whether to trust the call. Near-threshold cells are re-asked because of run-to-run noise (measured-behaviour.md). The script validates every response before gating: answer IDs equal the question IDs, each `choice` is a criteria option, `confidence` and every probability lie in [0, 1], and probabilities sum to 1 within rounding. Anything else is `JEV.RESPONSE_INVALID`, retried as transient. The response read is bounded at 1 MiB. The run file stores the full answers so thresholds can be re-tuned without re-judging. An `underspecified` result also lists `low_questions`: each gating answer that was `unclear` or under the threshold, as `{ask, role, question_id, choice, confidence}`; the report prints one `Low:` line per entry.

## Size, context window, and shortening

- Jev allows about 32k tokens for state plus the longest question and about 64k for state plus all questions.
- **Planning gate: a plan bead is never shortened.** A bead that fits `--max-item-chars` and, for every anchor, `--max-request-bytes` is sent whole; otherwise long code is reduced to signatures only as far as needed to fit (the rule is in `plan-size-and-shape.md`, Long code). Prose is never cut. A bead that does not fit with every long block reduced stops the run before sending with `PLAN.TOO_LARGE` (exit 2). A token-limit 422 at this gate is recorded per anchor as `PLAN.TOO_LARGE`, never halved (`plan-size-and-shape.md`). The run records `item.chars_before_reduction` and `item.code_reduced`.
- **Development gate:** before the first call the script shortens the evidence account (keeping the head, appending a truncation marker) until the serialised request is under `--max-request-bytes` (default 64,000 bytes at this gate; measured-behaviour.md). If Jev still answers a 4xx reporting the token limit, the script halves the account and retries, down to `--min-item-chars` (default 2,000). Below that the anchor is `verdict: error` with `JEV.CONTEXT_EXCEEDED`.
- Each result records `item_chars_sent`, `item_shortened`, `request_bytes`; the run's `summary.shortened` lists affected anchors.

## Attempt policy

Per anchor, per request:

| Situation | Behaviour |
| --- | --- |
| 429, 529, any 5xx, timeout, network error, invalid response | Up to 3 attempts. Delay before the second: `Retry-After` if present (capped at 30 s), else 1 s; before the third: 2 s. Then `verdict: error` with the last code; other anchors continue |
| 422 token limit | Shorten and retry as above (counts toward the 3 attempts) |
| 422 other | Not retried; `JEV.VALIDATION` with up to 300 characters of the body, which names the bad field |
| 401 | Not retried; the whole run aborts with `AUTH.CREDENTIALS` (body withheld) |
| Missing key, or `allow_remote_judge` not `true` | Checked before any request; the run aborts |

Agents and the skill never add a retry loop around the script. `attempts` is recorded on every result. Questions in one request are answered independently and in parallel: one round trip per anchor.

## Models

`jev-latest` moves with releases. Once thresholds are tuned, pin the version the response reported (`model_answered` in the run file) with `--model jev-1.13.0`.

## Error codes

| Code | Emitted by | Meaning and action |
| --- | --- | --- |
| `AUTH.CREDENTIALS` | pre-flight, 401 | Key missing, malformed, or rejected. Not retried |
| `POLICY.REMOTE_JUDGE_DISABLED` | pre-flight | `allow_remote_judge` is `false` or absent. Absent: ask the user once, record it with `plan_anchor_settings.py set`, commit. `false`: only the user changes it. `--dry-run` works either way |
| `DEPENDENCY.PYYAML`, `SETTINGS.*`, `REPO.NOT_FOUND` | pre-flight | The settings file could not be read; fix what the error names. Never a silent default |
| `JEV.RATE_LIMITED`, `JEV.OVERLOADED`, `JEV.HTTP`, `JEV.NETWORK` | transport | Transient; retried 3 times, then the command exits 2 (the cell is `error` in the partial run) |
| `JEV.RESPONSE_INVALID` | validation | Malformed or oversized response; retried, then the command exits 2 (the cell is `error` in the partial run) |
| `JEV.CONTEXT_EXCEEDED` | 422 token limit (development gate) | Evidence could not be shortened enough; shorten it |
| `PLAN.TOO_LARGE` | planning-gate pre-flight or 422 | The bead does not fit one request after code reduction; triage per `plan-size-and-shape.md`. Never trim or raise the cap to pass |
| `JEV.VALIDATION` | 422 other | Request rejected; usually a malformed override |
| `VALIDATION.ANCHORS`, `VALIDATION.INPUT` | record / arguments | Fix `anchors.json` (`validate`) or the flags |
| `QUESTIONS.ROLES` | question set | An override renamed a question ID; keep the default IDs |
| `ANCHOR.UNKNOWN`, `ANCHOR.NONE`, `ANCHOR.NOT_LOCKED` | record | Wrong ID; nothing locked; `--anchor` named a proposed or retired anchor (`--include-unlocked` is for fixtures only) |
| `ANCHOR.DUPLICATE` | `add` (exit 0, `data.refused.code`) | The ID is already recorded; `update` it instead |
| `ANCHOR.DUPLICATE_CANDIDATE` | `add` (exit 0, `data.refused.code`) | Statement or quote lexically close to an existing anchor; read the match, then `--allow-similar` only for a different constraint |
| `ANCHOR.NOT_TESTABLE` | `add --rubric` (exit 0, `data.refused.code`), tuner | The statement failed the testability rubric or lint; rewrite it (`anchor-capture.md`) |
| `ANCHOR.LINT` | `add --strict`, `update --strict` (exit 0, `data.refused.code`) | The statement has lint warnings and strict mode was requested |
| `FIXTURES.MISSING` | tuner | No pass or fail fixture sentence; draft them with the user (`update --fixture-pass/--fixture-fail`) |
| `INPUT.MISSING`, `INPUT.MALFORMED`, `INPUT.EMPTY` | files | A path is not a file, not JSON, or the item text is empty |
| `FIXTURE.MISSING`, `FIXTURE.NONE` | `--answers-file`, `check-fixtures` | No fixture answer for an anchor; no anchor has fixtures |
| `RUNS.NONE`, `RUNS.MISMATCH` | `coverage`, `diff` | No run files; runs for different gates or items |
| `SCRIPT.INTERNAL` | anywhere | Unexpected exception, reported as JSON with exit 2 |

Fatal codes abort with exit 2 and `success: false`. A Jev failure is never a negative result: when any cell, pair or candidate still has no answer after retries, `judge`, `check-fixtures`, `dedupe --jev`, `rubric` and `add --rubric` exit 2 with the first failure's code. The partial run is still written: `data.run_path` names it (`judge`, `check-fixtures`) and `data.failures` lists `{id, code, message}` for each item without an answer. `add --rubric` records nothing, with or without `--allow-untestable`.

## Exit codes

A tool failure is the only nonzero exit. Exit 0 means the command ran (`success: true`); read the outcome from `data`, never from the exit code. Exit 2 means it could not do its job (`success: false` with a typed `error`). The only other exit code is 130, an interrupt.

| Subcommand | Outcome field (exit 0) |
| --- | --- |
| `judge` | `data.summary.pass` (false on a failing verdict or a dry run; the run file is written), `data.dry_run` |
| `check-fixtures` | `data.pass`, `data.mismatches`, `data.judged` (`pass` is false when nothing was judged, e.g. every check is `deterministic_only`; `data.note` says so) |
| `validate` | `data.valid`, `data.problems`, `data.warnings` |
| `lint` | `data.clean`, `data.warning_count` |
| `rubric` | `data.pass` (false when any anchor is `needs_work`) |
| `dedupe` | `data.likely_duplicates` (empty when none) |
| `add` | `data.added`; a refusal is `added: false` with `data.refused` `{code, message, recoverable, suggested_action}` (`ANCHOR.DUPLICATE`, `ANCHOR.LINT`, `ANCHOR.DUPLICATE_CANDIDATE`, `ANCHOR.NOT_TESTABLE`, or `VALIDATION.INPUT` when the statement repeats the quote) and nothing written |
| `update` | `data.updated`; a `--strict` refusal is `updated: false` with `data.refused.code: ANCHOR.LINT`; nothing to change is `updated: false, changed: false` |
| `retire`, `exclude --remove` | `data.changed` (false when the anchor is already retired, or there was no exclusion to remove) |
| `exclude --from` | `data.applied` (one entry per line), `data.count`, `data.changed` (lines that changed the record) |
| `report`, `coverage`, `diff` | `data.pass`; `data.uncovered_anchors`; `data.pass_after`, `data.regressions` |

Exit 2 covers bad arguments, missing or unreadable files, invalid settings, `POLICY.REMOTE_JUDGE_DISABLED`, credential failure, any Jev call left without an answer, a missing dependency, and `SCRIPT.INTERNAL`. Exit 130 (`CANCELED`, `recoverable: true`) is an interrupt.

## Other Jev-backed subcommands

| Subcommand | Sends | Question set | Verdict |
| --- | --- | --- | --- |
| `rubric` | one anchor's statement, quote and scope | `assets/questions/testability.json` (single constraint, observable, requirement form, faithful to quote, scoped, positive form) | `testable` when every answer is yes at `--threshold` (default 0.6) and the lint has no error; else `needs_work` with the failing criteria; `data.pass` false on any `needs_work` |
| `dedupe --jev` | one pair of anchors (statements and quotes) per request, only for pairs the lexical pass flagged at `--threshold` (Jaccard, default 0.5) | `assets/questions/dedupe.json` (same constraint?) | pairs with `jev.choice` and confidence; likely duplicates are `yes` at the global confidence threshold, or Jaccard >= 0.8 without Jev; `data.likely_duplicates` lists any that remain. `--answers-file` implies `--jev` |
| `check-fixtures` | one fixture sentence as the item | the gate's set | pass -> `satisfied`, fail -> `not_addressed` or `contradicted` |

`lint` and `shape` are deterministic and never contact Jev. `lint` with neither `--id` nor `--all` lints every non-retired anchor.

## Judging flags

| Flag | Default | Purpose |
| --- | --- | --- |
| `--model` | `jev-latest` | Model alias or pinned version |
| `--threshold` | 0.6 | Confidence gate (`verdict-gating.md`) |
| `--questions` | `assets/questions/<gate>.json` | Alternative question-set file |
| `--overrides-file` | none | Per-anchor overrides read from the question beads |
| `--extra-questions` | none | Repeatable JSONL file of consumer questions, one `{id, type, instructions, criteria}` per line. Added to the set for every cell, answered and recorded in `results[].answers`, listed with path, sha256 and ids in the run's `extra_questions`; never gating. An id already in the set, or a role mapped to an extra question, is refused before any call |
| `--no-triage-questions` | off | Skip the default triage questions. Without it, `judge --gate plan` adds `.sc/plan-anchor/triage/plan-questions.jsonl` next to the nearest `settings.yaml` (from `--repository-root`), else the shipped `assets/triage/plan-questions.jsonl`. Extra answers are optional: an answer missing from the response or fixture is skipped, and when the body exceeds `--max-request-bytes` the extras are dropped (`extra_questions_dropped`) rather than the item refused. `questions_sha256` covers the gate questions only; `extra_questions_sha256` covers the extras |
| `--max-item-chars` | 24,000 | Development gate: truncate the evidence to this. Planning gate: refuse a larger bead with `PLAN.TOO_LARGE` |
| `--max-request-bytes` | 24,000 plan / 64,000 dev | Pre-flight cap on the serialised request (planning gate: refuse; development gate: shorten) |
| `--confirm-near-threshold` | on | Re-ask once when a required-role confidence is within 0.10 of the threshold; disagreement makes the cell `underspecified` with `unstable: true` (`--no-confirm` turns it off) |
| `--max-code-lines` | 8 | Planning gate: the shortest fenced block that may be sent as signatures plus a redaction marker, only when the bead would not otherwise fit (`plan-size-and-shape.md`) |
| `--min-item-chars` | 2,000 | Development gate: floor for context shortening |
| `--timeout` | 30 s | Per-request socket timeout |
| `--run-dir` / `--out` | `<anchors dir>/anchor-runs` | Where the run file goes |
| `--repository-root` | `.` | Any path inside the repository whose settings file holds `allow_remote_judge` (live calls only) |
| `--dry-run`, `--answers-file`, `--include-unlocked` | | See above |

## Cost and latency

TypeSafe prices input tokens only (output is free) at a fraction of a cent per million tokens and quotes roughly 100 ms per request. A planning gate over 15 beads and 20 anchors is 300 requests and costs effectively nothing; re-run after every plan edit. `--dry-run` reports `totals.bytes` to size a run without a key.

## Environment variables

| Variable | Default | Purpose |
| --- | --- | --- |
| `TYPESAFE_API_KEY` | none | Preferred TypeSafe credential for live runs; when absent, use the documented platform credential store |

It is the only environment variable the plugin reads. Model, threshold, size limits and near-threshold confirmation are the flags above, with checked-in defaults. The installed troubleshooting reference specifies the Keychain, Credential Manager, and Secret Service fallback entries.

Codex environment policy for `TYPESAFE_API_KEY`: `installation-and-troubleshooting.md`.
