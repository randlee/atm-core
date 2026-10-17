---
name: jev-api
version: 0.6.0
description: How jev_judge.py talks to TypeSafe's Jev - endpoint, auth, request and response shape, validation, limits, the retry and context-shortening policy, every error code and flag, model pinning, and the privacy opt-in that gates every call.
---

# Jev API as used by this skill

Jev is TypeSafe's decision model: it answers typed questions about a piece of state with probabilities and confidence, in parallel, and generates no text. `scripts/jev_judge.py` implements everything below; this document is for troubleshooting, not for hand-rolling requests.

## Privacy opt-in (read first)

A gate run sends the anchor statement and scope and the item text (a plan bead or an account of delivered work) off the machine. `rubric` also sends the verbatim quote; `dedupe --jev` sends pairs of statements and quotes. Sending a plan to TypeSafe is the user's decision, so the script fails closed:

- `TYPESAFE_API_KEY` must be set and well-formed (non-empty, ASCII, no control characters), and
- `PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1` must be set for the session (or `--allow-remote` passed).

Without both, `judge`, `check-fixtures`, `rubric` and `dedupe --jev` return `AUTH.CREDENTIALS` or `POLICY.REMOTE_JUDGE_DISABLED` and send nothing. `--dry-run` writes the exact request bodies and `totals {requests, bytes}` to the run directory. `--answers-file` runs the gating with fixture answers, offline. The key is never printed, logged, written to a run file, or included in an error. Set the opt-in per plan, not globally.

## Endpoint and auth

- `POST https://api.typesafe.ai/v1/systemone` (override with `PLAN_ANCHOR_JEV_URL` for a proxy or mock)
- `Authorization: Bearer $TYPESAFE_API_KEY`, `Content-Type: application/json`

## Request

```json
{
  "model": "jev-latest",
  "state": {
    "anchor": {"id": "ANC-012", "statement": "...", "scope": "..."},
    "item": {"id": "bd-42", "title": "...", "text": "..."}
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
| Missing opt-in or key | Checked before any request; the run aborts |

Agents and the skill never add a retry loop around the script. `attempts` is recorded on every result. Questions in one request are answered independently and in parallel: one round trip per anchor.

## Models

`jev-latest` moves with releases. Once thresholds are tuned, pin the version the response reported (`model_answered` in the run file) via `PLAN_ANCHOR_JEV_MODEL=jev-1.13.0` or `--model`.

## Error codes

| Code | Emitted by | Meaning and action |
| --- | --- | --- |
| `AUTH.CREDENTIALS` | pre-flight, 401 | Key missing, malformed, or rejected. Not retried |
| `POLICY.REMOTE_JUDGE_DISABLED` | pre-flight | Opt-in not set. Use `--dry-run` or set `PLAN_ANCHOR_ALLOW_REMOTE_JUDGE=1` |
| `JEV.RATE_LIMITED`, `JEV.OVERLOADED`, `JEV.HTTP`, `JEV.NETWORK` | transport | Transient; retried 3 times, then per-anchor `error` |
| `JEV.RESPONSE_INVALID` | validation | Malformed or oversized response; retried, then per-anchor `error` |
| `JEV.CONTEXT_EXCEEDED` | 422 token limit (development gate) | Evidence could not be shortened enough; shorten it |
| `PLAN.TOO_LARGE` | planning-gate pre-flight or 422 | The bead does not fit one request after code reduction; triage per `plan-size-and-shape.md`. Never trim or raise the cap to pass |
| `JEV.VALIDATION` | 422 other | Request rejected; usually a malformed override |
| `VALIDATION.ANCHORS`, `VALIDATION.INPUT` | record / arguments | Fix `anchors.json` (`validate`) or the flags |
| `QUESTIONS.ROLES` | question set | An override renamed a question ID; keep the default IDs |
| `ANCHOR.UNKNOWN`, `ANCHOR.DUPLICATE`, `ANCHOR.NONE`, `ANCHOR.NOT_LOCKED` | record | Wrong ID; already recorded; nothing locked; `--anchor` named a proposed or retired anchor (`--include-unlocked` is for fixtures only) |
| `ANCHOR.DUPLICATE_CANDIDATE` | `add` | Statement or quote lexically close to an existing anchor; read the match, then `--allow-similar` only for a different constraint |
| `ANCHOR.NOT_TESTABLE` | `add --rubric`, tuner | The statement failed the testability rubric or lint; rewrite it (`anchor-capture.md`) |
| `ANCHOR.LINT` | `add --strict`, `update --strict` | The statement has lint warnings and strict mode was requested |
| `FIXTURES.MISSING` | tuner | No pass or fail fixture sentence; draft them with the user (`update --fixture-pass/--fixture-fail`) |
| `INPUT.MISSING`, `INPUT.MALFORMED`, `INPUT.EMPTY` | files | A path is not a file, not JSON, or the item text is empty |
| `FIXTURE.MISSING`, `FIXTURE.NONE` | `--answers-file`, `check-fixtures` | No fixture answer for an anchor; no anchor has fixtures |
| `RUNS.NONE`, `RUNS.MISMATCH` | `coverage`, `diff` | No run files; runs for different gates or items |
| `SCRIPT.INTERNAL` | anywhere | Unexpected exception, reported as JSON with exit 2 |

Fatal codes abort with exit 2 and `success: false`. Transient codes land in `results[].error` inside a successful run that exits 1.

## Exit codes

| Code | `judge` | `check-fixtures` | `validate` | `lint` | `rubric` | `dedupe` | `shape` |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | gate passed, or dry run | every fixture landed as expected | no problems | no warnings | all testable | no likely duplicates | always |
| 1 | a failing or `error` verdict; run file written | a mismatch | problems listed | warnings | some `needs_work` | likely duplicates | never |
| 2 | fatal `JudgeError` or `SCRIPT.INTERNAL` | same | same | same | same | same | fatal only |

## Other Jev-backed subcommands

| Subcommand | Sends | Question set | Verdict |
| --- | --- | --- | --- |
| `rubric` | one anchor's statement, quote and scope | `assets/questions/testability.json` (single constraint, observable, requirement form, faithful to quote, scoped, positive form) | `testable` when every answer is yes at `--threshold` (default 0.6) and the lint has no error; else `needs_work` with the failing criteria; exit 1 on any `needs_work` |
| `dedupe --jev` | one pair of anchors (statements and quotes) per request, only for pairs the lexical pass flagged at `--threshold` (Jaccard, default 0.5) | `assets/questions/dedupe.json` (same constraint?) | pairs with `jev.choice` and confidence; likely duplicates are `yes` at the global confidence threshold, or Jaccard >= 0.8 without Jev; exit 1 when any remain. `--answers-file` implies `--jev` |
| `check-fixtures` | one fixture sentence as the item | the gate's set | pass -> `satisfied`, fail -> `not_addressed` or `contradicted` |

`lint` and `shape` are deterministic and never contact Jev. `lint` with neither `--id` nor `--all` lints every non-retired anchor.

## Judging flags

| Flag | Default | Purpose |
| --- | --- | --- |
| `--model` | `jev-latest` | Model alias or pinned version |
| `--threshold` | 0.6 | Confidence gate (`verdict-gating.md`) |
| `--questions` | `assets/questions/<gate>.json` | Alternative question-set file |
| `--overrides-file` | none | Per-anchor overrides read from the question beads |
| `--max-item-chars` | 24,000 | Development gate: truncate the evidence to this. Planning gate: refuse a larger bead with `PLAN.TOO_LARGE` |
| `--max-request-bytes` | 24,000 plan / 64,000 dev | Pre-flight cap on the serialised request (planning gate: refuse; development gate: shorten) |
| `--confirm-near-threshold` | on | Re-ask once when a required-role confidence is within 0.10 of the threshold; disagreement makes the cell `underspecified` with `unstable: true` (`--no-confirm`, `PLAN_ANCHOR_CONFIRM_NEAR=0`) |
| `--max-code-lines` | 8 | Planning gate: the shortest fenced block that may be sent as signatures plus a redaction marker, only when the bead would not otherwise fit (`plan-size-and-shape.md`) |
| `--min-item-chars` | 2,000 | Development gate: floor for context shortening |
| `--timeout` | 30 s | Per-request socket timeout |
| `--run-dir` / `--out` | `<anchors dir>/anchor-runs` | Where the run file goes |
| `--dry-run`, `--answers-file`, `--allow-remote`, `--include-unlocked` | | See above |

## Cost and latency

TypeSafe prices input tokens only (output is free) at a fraction of a cent per million tokens and quotes roughly 100 ms per request. A planning gate over 15 beads and 20 anchors is 300 requests and costs effectively nothing; re-run after every plan edit. `--dry-run` reports `totals.bytes` to size a run without a key.

## Environment variables

| Variable | Default | Purpose |
| --- | --- | --- |
| `TYPESAFE_API_KEY` | none | Required for live runs |
| `PLAN_ANCHOR_ALLOW_REMOTE_JUDGE` | unset | Must be `1` for live runs |
| `PLAN_ANCHOR_JEV_MODEL` | `jev-latest` | Model alias or pinned version |
| `PLAN_ANCHOR_JEV_URL` | TypeSafe endpoint | Proxy or mock server |
| `PLAN_ANCHOR_CONFIDENCE_THRESHOLD` | `0.6` | Gating threshold |
| `PLAN_ANCHOR_MAX_ITEM_CHARS` | `24000` | Evidence truncation (development gate) and bead size limit (planning gate) |
| `PLAN_ANCHOR_MAX_CODE_LINES` | `8` | Default for `--max-code-lines` |

Codex environment policy for `TYPESAFE_API_KEY`: `installation-and-troubleshooting.md`.
