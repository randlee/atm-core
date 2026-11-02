# jev-check

One client for Jev (TypeSafe's choice-question judge), plus Haiku and Luna as second opinions that can be
questioned afterwards. One check is one request. A request over the byte cap is refused, never split.

Spec: `docs/plans/phase-bo/design/jev-check-proposal.md`. This README is the reference for porting the
package to a Rust CLI: every behaviour below is covered by a test in `tests/` (see "Parity checklist").

Python 3.11+, no third-party dependencies. Runtime needs: network access to the vendor APIs, the keys
(see "Keys"), `git` for check configs, and macOS `security` only for the Keychain fallback.

## Concepts

- **Question**: `{"type": "choice", "instructions": str, "criteria": {criterion: description}}`, two or more
  criteria. `choice` is the only question type today.
- **Question set**: a JSON file of question *templates* (schema `plan-anchoring/questions/1`). A template is
  expanded once per subject into questions keyed `<template>:<subject id>`.
- **State**: any JSON value; the material the questions are about (source files, spec rows, ...).
- **Payload**: the bytes `{"questions": ..., "state": ...}`, identical for every backend.
- **Answer**: `{"choice": criterion, "confidence": number, "probabilities": {criterion: number}}`.
- **Backend**: `jev`, `haiku` or `luna`. Same payload, same answer shape.
- **Session**: every run is stored; a Haiku or Luna session can be asked follow-up questions ("why is
  T-CLOSE-03 yes?"). Jev gives no rationale, so this is how a Jev answer gets explained.
- **Input record**: proof that a backend saw only the payload (no extra context, no tool calls).

Haiku and Luna are not Jev clones. They give a similar perspective on the same input, which you can then
question. Compare choices; do not expect their confidence values to match Jev's.

## CLI

`scripts/jev-check.py` (runs with `uv run --script`; the package is its only dependency).

```
jev-check run <check.toml> [--rev <rev>] [--backend jev|haiku|luna]
jev-check ask --session <id> "<question>"
```

| command | does | stdout | exit |
|---|---|---|---|
| `run` | builds the request from `check.toml` at `--rev` (default `HEAD`), sends it to `--backend` (default `jev`), stores a session | the `JevCheckResult` JSON (indent 1) | 0 |
| `ask` | continues a Haiku/Luna session with one follow-up; appends it to the session | the answer text | 0 |
| any error | | stderr: `{"error": "<ERROR_ID>", "message": "..."}` | 2 |

`run` must be executed inside the git repository that `check.toml`'s paths refer to.

### JevCheckResult (stdout of `run`)

```json
{
 "check_id": "task-close", "rev": "HEAD", "backend": "haiku", "model": "claude-haiku-5-5",
 "session_id": "<uuid4>",
 "answers": {"implemented:T-1": {"choice": "yes", "confidence": 0.6, "probabilities": {"yes": 0.8, "no": 0.2}}},
 "input": {"prompt_sha256": "<hex>", "input_tokens": 14515, "prompt_tokens": 14515, "leak": 0,
           "commands": 0, "equal": true}
}
```

## Check config (`check.toml`)

One TOML file per check. Unknown top-level keys, unknown source kinds and missing/extra source keys are
`CONFIG_INVALID`.

```toml
id = "task-close"                 # required
question_set = "q.json"           # required; relative to this file
[inputs.hook]                     # one or more; the name becomes a key of state
kind = "file"
path = "hook.py"
[inputs.rows]
kind = "table-column"
path = "spec.md"
table = "Transitions"             # the markdown table under "## Transitions"
column = "id"                     # the column whose cell is the row id
[inputs.branches]
kind = "python-branches"
path = "hook.py"
[[expand]]                        # zero or more
template = "implemented"          # a template in the question set
subjects = "rows"                 # an input name; each of its entries becomes one question
```

### Extractors

Every input is read with `git show <rev>:<path>`; failure is `INPUT_MISSING`. An empty result is
`EXTRACTION_EMPTY`.

| kind | state value |
|---|---|
| `file` | the file text |
| `table-column` | `{row id: the whole markdown row line}` for the table under `## <table>` (header and separator rows skipped; rows with an empty id cell skipped) |
| `python-branches` | `{"<path>:<line>": text}` for every `case` arm (`case <pattern>`, line of the pattern) and every `return` whose value calls `Refusal(...)` (the unparsed return), sorted by line |

### Expansion

For each `[[expand]]`, every `(subject id, text)` of the named input becomes a question keyed
`<template>:<subject id>`, whose instructions are the template's with `{subject}` replaced by
`` Subject `<input>['<subject id>']`: <text> ``. Questions from all `[[expand]]` entries are merged in order.

## Question set file

```json
{"schema": "plan-anchoring/questions/1",
 "questions": {"implemented": {"type": "choice", "instructions": "{subject} implemented?",
                               "criteria": {"yes": "...", "no": "..."}}}}
```

Any other `schema`, a non-choice template or fewer than two criteria is `INPUT_INVALID`. Other top-level
keys (`gate`, `version`, `notes`) are ignored.

## Request validation (all backends)

Before anything is sent, on every backend:

- no questions, a non-`choice` question or one with fewer than two criteria: `INPUT_INVALID`;
- the Jev request body (below) longer than `max_request_bytes` (96,000): `INPUT_TOO_LARGE`. Never split.

Haiku and Luna apply the same check to the same body, so a request one backend refuses, all refuse.

## Payload bytes

```
json.dumps({"questions": questions, "state": state}, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
```

UTF-8, keys sorted at every level, no whitespace, non-ASCII kept as is. A port must produce identical bytes:
`input.prompt_sha256` is the sha256 of these bytes and is how runs are matched across backends.

## Jev backend

- `POST https://api.typesafe.ai/v1/systemone`, headers `Authorization: Bearer <TYPESAFE_API_KEY>`,
  `Content-Type: application/json`, timeout 60 s.
- Body: the payload with `"model": "jev-1.13.0"` added, same serialisation (sorted keys, so `model` comes first):
  `{"model":"jev-1.13.0","questions":{...},"state":...}`.
- Retry: only HTTP 429, 502, 529, up to `attempts` (4) sends in total with the same body; wait
  `Retry-After` seconds clamped to [0, 30] (missing: 1 s; unparsable: 30 s). Exhausted: `JEV_UNAVAILABLE`.
  Any other non-200: `JEV_UNAVAILABLE`, no retry. Connection failure: `JEV_UNAVAILABLE`.
- Reply: `{"model", "answers": {id: {choice, confidence, probabilities}}, "usage": {"input_tokens", ...}}`.
  `RESPONSE_INVALID` if `model` differs from the configured model, answer ids differ from question ids, a
  choice is not one of its question's criteria, or a number is not a number.
- Jev's confidence is always the highest probability minus the second highest (true for all 253 stored
  answers, 2026-11-01).
- Jev has no token-count endpoint, so its input record has `prompt_tokens` and `leak` null.

## Haiku and Luna backends (API-direct)

No SDK, no agent harness, no tools: one HTTPS request. The user message is the payload bytes, decoded as
UTF-8. Each run makes two requests: a token count of the exact body, then the message.

### Prompting

**Check request (`run`)**: one user turn (the payload) plus a system prompt that defines the answer and
carries the JSON Schema of Jev's answer for *these* questions.

- `answer_schema(questions)`: `{"answers": {id: <part>}}`, all objects closed (`additionalProperties:
  false`) with every property required. Each question's part is chosen by its `type` from `ANSWER_TYPES`; an
  unknown type is `INPUT_INVALID`. The `choice` part:
  `{"choice": {"type": "string", "enum": [criteria in question order]}, "confidence": <p>,
    "probabilities": {criterion: <p> for each criterion}}`, with `<p> = {"type": "number", "minimum": 0,
  "maximum": 1}`.
- Object key order in the schema is `type, additionalProperties, required, properties`; the schema is
  embedded as `json.dumps(schema, indent=2)`.
- `system_prompt(schema)` is exactly (the schema replaces `<schema>`):

```
[System Prompt / Role]
You are a strict data extraction utility. The user message is a JSON object: "state" is the material to judge and
"questions" maps each question id to its instructions and criteria. Answer every question independently.
"probabilities" gives your probability that each criterion is the correct answer; they sum to 1. "choice" is the
criterion with the highest probability. "confidence" is the margin: the highest probability minus the second highest
(probabilities 0.59 and 0.41 give confidence 0.18; 0.5 and 0.5 give 0). It is not the probability of the choice.
You must ONLY respond with a valid JSON object matching the requested schema. Do not include any
conversational filler, markdown formatting (do not use ```json), or markdown wrappers.

[Schema Definition]
Your output must perfectly match this JSON Schema:
<schema>
```

- The schema is **not** sent as the vendor's structured-output format: Anthropic rejects a per-question
  schema of ~20 questions with HTTP 400 "The compiled grammar is too large". The reply is validated instead
  (below).

**Follow-up (`ask`)**: no system prompt and no schema, so the model answers in prose. Turns: user = the
original payload, assistant = the original reply text, then each earlier follow-up as user/assistant, then
the new question.

### Wire format

| | Haiku | Luna |
|---|---|---|
| host | `api.anthropic.com` | `api.openai.com` |
| message | `POST /v1/messages` | `POST /v1/responses` |
| token count | `POST /v1/messages/count_tokens` | `POST /v1/responses/input_tokens` (unverified: no key yet) |
| headers | `x-api-key`, `anthropic-version: 2026-10-21`, `Content-Type: application/json` | `Authorization: Bearer`, `Content-Type: application/json` |
| body | `{"model", "max_tokens": 16000, "messages": [{"role", "content"}], "system"}` | `{"model", "input": [{"role", "content"}], "instructions"}` |
| count body | the message body without `max_tokens` | the message body |
| reply text | concatenated `content[]` blocks of type `text` | concatenated `text` of `output[]` items of type `message` |
| input tokens | `usage.input_tokens + cache_creation_input_tokens + cache_read_input_tokens` | `usage.input_tokens` |
| commands | `content[]` blocks whose type ends in `tool_use` | `output[]` items not of type `message` or `reasoning` |

`system`/`instructions` is present only on check requests. Non-200 from either request:
`BACKEND_UNAVAILABLE` with the status and the first 300 bytes of the body. No text in the reply:
`RESPONSE_INVALID`. Request timeout 300 s.

### Reply validation

The reply text must parse as `{"answers": {id: {"choice", "confidence", "probabilities"}}}` with exactly the
question ids, each choice one of its criteria, and a probability for exactly its criteria; otherwise
`RESPONSE_INVALID`. The model's own numbers are kept; nothing is synthesized. Extra keys inside an answer
are ignored.

## Equal input (`input`)

Recorded on every run so a comparison between backends can be shown to rest on the same input.

| field | meaning |
|---|---|
| `prompt_sha256` | sha256 of the payload bytes; same on every backend for the same request |
| `input_tokens` | tokens the vendor billed for the message request |
| `prompt_tokens` | the vendor's count of the same body (null for Jev) |
| `leak` | `input_tokens - prompt_tokens`: anything read beyond the request (null for Jev) |
| `commands` | tool/command calls in the reply (0 for Jev) |
| `equal` | `commands == 0 and abs(leak) <= leak_tolerance * prompt_tokens`; Jev: `commands == 0` |

`prompt_tokens` counts the whole body, system prompt included, so the system prompt does not show as leak.
Haiku's input is therefore about 6k tokens larger than Jev's for the same payload (e.g. 14,515 vs 8,823
without the system prompt).

## Keys

Every backend, Jev included, resolves its key the same way, in this order, and stops at the first non-empty
value (surrounding whitespace stripped):

| backend | variable = key-vault service name |
|---|---|
| jev | `TYPESAFE_API_KEY` |
| haiku | `ANTHROPIC_API_KEY` (`[haiku] key_env`) |
| luna | `OPENAI_API_KEY` (`[luna] key_env`) |

1. **Environment**: the variable above.
2. **Key vault**: the OS credential store, generic password with service = the variable name and
   account = `$USER`. On macOS this is the login Keychain, read with
   `security find-generic-password -a $USER -s <VARIABLE NAME> -w` (exit nonzero or 10 s timeout = absent).
   `ANTHROPIC_KEYCHAIN_SERVICE`, if set, replaces the service name for `ANTHROPIC_API_KEY` only.
   A Rust port should use the platform store through one interface (e.g. the `keyring` crate: macOS
   Keychain, Windows Credential Manager, Linux Secret Service) with the same service/account.
3. **Local** (Haiku/Luna): not implemented yet; the source is to be confirmed (see the open question in
   PR #2308).

None found: `KEY_MISSING`, naming the variable and both places looked, and nothing is sent. Keys are
never printed, logged, written to sessions or included in error text. Code: `client.env_or_keychain`
(lookup injectable for tests), used by `HttpsTransport` (Jev) and `llm.api_key` (Haiku/Luna).

## Sessions

Directory `[sessions] dir` (`~/.cache/jev-check/sessions`), one `<uuid4>.json` per run:

```json
{"id", "created_at": <unix seconds>, "check_id", "rev", "backend", "model",
 "request": {"state", "questions"}, "reply_text": <str | null for jev>, "answers", "input",
 "followups": [{"question", "answer"}]}
```

`followups` appears after the first `ask`. Each write deletes session files older than `keep_days` (7) by
mtime. Reading a missing session: `SESSION_NOT_FOUND`; older than `keep_days`: `SESSION_EXPIRED`; a Jev
session (no reply text): `SESSION_NOT_ASKABLE`.

## Configuration (`src/jev_check/jev.toml`, packaged)

| key | value | use |
|---|---|---|
| `model`, `host`, `path` | `jev-1.13.0`, `api.typesafe.ai`, `/v1/systemone` | Jev endpoint |
| `max_request_bytes` | 96000 | byte cap for every backend |
| `attempts` | 4 | Jev sends in total on busy replies |
| `timeout_seconds` | 60 | Jev request timeout |
| `leak_tolerance` | 0.01 | `equal` threshold |
| `[haiku]`, `[luna]` | `model`, `key_env` | LLM backends |
| `[sessions]` | `dir`, `keep_days` | session store |

## Errors

Every failure is a `JevCheckError` with one of these ids (the CLI prints it as `error`).

| id | raised when |
|---|---|
| `CONFIG_INVALID` | check.toml unreadable, unknown key or source kind, wrong source keys, bad `[[expand]]` |
| `INPUT_MISSING` | `git show <rev>:<path>` fails |
| `EXTRACTION_EMPTY` | an extractor finds nothing |
| `INPUT_INVALID` | bad question set, no questions, a non-choice question, fewer than two criteria, unknown template, a question type with no answer schema, unknown backend |
| `INPUT_TOO_LARGE` | the request body exceeds `max_request_bytes`; nothing sent |
| `KEY_MISSING` | the backend's key is in neither the environment nor the Keychain; nothing sent |
| `JEV_UNAVAILABLE` | Jev non-200, busy attempts exhausted, or connection failure |
| `BACKEND_UNAVAILABLE` | Haiku/Luna non-200 or connection failure |
| `RESPONSE_INVALID` | a reply or token count is not the expected JSON shape |
| `SESSION_NOT_FOUND`, `SESSION_EXPIRED`, `SESSION_NOT_ASKABLE` | see Sessions |

## Library API (Python)

`check(state, questions)` (Jev only) -> `JevReply(answers, input_tokens)`;
`check_llm(backend, state, questions)` -> `(answers, LlmReply(text, input_tokens, prompt_tokens, commands))`;
`run_request(check_id, rev, backend, state, questions)` -> `JevCheckResult` (any backend, stores a session;
the entry point for callers that build their own state and questions, such as the lite gate);
`run_check(cfg, rev, backend)` = `build_request` + `run_request`; `SessionStore.default().ask(sid, q)`.
Transports are injectable (`Transport.send(body)` for Jev, `Http.post(host, path, headers, body)` for LLMs),
so every test runs without network or keys.

The CLI exposes `run` (from a check.toml) and `ask`. `run_request` with a caller-built state is library-only;
a Rust port that serves the gate needs an equivalent (for example `run --state <json> --questions <json>`).

## Parity checklist (behaviour -> test)

| behaviour | test |
|---|---|
| Jev body byte-exact; one request per check | `test_client.py::test_one_check_is_one_request_byte_exact` |
| payload bytes = Jev body without `model` | `test_payload_bytes_are_the_jev_request_without_the_model` |
| over cap refused, nothing sent (Jev) | `test_oversized_input_is_refused_and_nothing_is_sent` |
| over cap / invalid question refused, nothing sent (LLM) | `test_checks.py::test_llm_input_is_refused_like_jev_and_nothing_is_sent` |
| busy-only retry, same body; attempts exhausted | `test_busy_is_retried_with_the_same_body`, `test_non_busy_error_is_not_retried`, `test_busy_attempts_exhausted` |
| answer ids must match | `test_answer_ids_must_match_questions` |
| question-set expansion and schema check | `test_question_set_expands_one_question_per_subject`, `test_question_set_rejects_other_schema` |
| check.toml errors | `test_config_load_errors` |
| extractors | `test_missing_input_at_rev`, `test_python_branches_lists_arms_and_refusals`, `test_table_column_rows`, `test_empty_extraction_is_typed` |
| request from check.toml | `test_request_is_one_question_per_row_and_branch` |
| Haiku body, system prompt, per-type schema, count request | `test_haiku_request_is_the_payload_alone` |
| Luna body and instructions | `test_luna_request_is_the_payload_alone` |
| unknown question type | `test_unknown_question_type_has_no_answer_schema` |
| reply validation | `test_llm_bad_reply_is_typed` |
| equal-input record | `test_equal_input_record` |
| keys: env, then Keychain; missing sends nothing | `test_api_key_env_first_then_keychain`, `test_jev_key_env_first_then_keychain`, `test_api_key_missing_sends_nothing` |
| session written; `ask` continues without system prompt | `test_run_writes_session_and_ask_continues_it` |
| session store from config | `test_default_session_store_follows_config` |

Run: `uv run --with pytest pytest -q tests` and `uv run --with pytest --with pyright pyright src tests scripts`.

## For the Rust CLI: requested features

Not in the Python package today; each is something the tuning work on 2026-11-01 needed and did by hand.

1. **Caller-built requests.** `run --state <file.json> --questions <file.json> --check-id <id>`: the
   equivalent of `run_request`. The lite gate builds its own state and questions; without this it must
   link the library.
2. **Dry run.** `request --backend <b> ...` prints the exact body (and, for Haiku/Luna, the count body and
   system prompt) and `prompt_sha256`, sends nothing, needs no key. Answers "what exactly did Haiku see?".
3. **Several backends in one run.** `run --backend jev,haiku,luna` sends the same payload to each and prints
   one row per question with every backend's choice and confidence, the agreement count, and each
   backend's `input` record. Agreement is reported only when every side is `equal`. A missing key on one
   LLM backend is reported per backend, not fatal. (Today: an uncommitted `tune-eq.py` script.)
4. **Disagreement cue.** In that output, mark questions where choices differ or Jev's confidence is below a
   threshold given on the command line, with the session id to `ask`. These are the questions worth asking
   Haiku about; the threshold is a display option, never a gate.
5. **Session commands.** `sessions list` (id, check, backend, rev, age) and `sessions show <id>` (request,
   answers, follow-ups). Today the JSON files are read by hand.
6. **`ask --json`.** `{"session_id", "question", "answer"}` on stdout for scripted follow-ups.
7. **Local key source** for Haiku/Luna (third after env and key vault): source to be confirmed.
8. **Luna live check.** The count endpoint `/v1/responses/input_tokens` is unverified; confirm it with a
   real key before relying on Luna's `equal`.

## For the Rust CLI: areas to tighten

Gaps in the Python code a port should close rather than copy.

1. **Canonical bytes.** The payload is sorted-key compact JSON with non-ASCII kept. Use an ordered map
   (`BTreeMap`/`serde_json` without `preserve_order`) for the payload, but keep question insertion order for
   the answer schema's `enum` and `probabilities` (Python dicts preserve it). Float formatting differs
   between Python `repr` and Rust; state from check.toml is strings only, but caller-built state with
   floats will hash differently. Add a cross-implementation fixture: one request, its exact bytes and
   sha256, checked by both.
2. **python-branches without Python.** The extractor's text is `ast.unparse` output, which a Rust parser
   (tree-sitter-python, ruff's parser) will not reproduce byte for byte, so `prompt_sha256` would differ
   from the Python version. Either define the text as the original source slice of the arm/return (and
   change the Python version to match), or accept that hashes differ across implementations.
3. **Jev probabilities.** `parse_answers` accepts any probability keys. Require exactly the question's
   criteria, as the LLM path does.
4. **Truncated or refused LLM replies.** `stop_reason` (Anthropic) / `status` (OpenAI) is not read; a reply
   cut at `max_tokens` surfaces as "not JSON". Check it and raise `RESPONSE_INVALID` naming the stop reason.
5. **No busy retry for Haiku/Luna.** Jev retries 429/502/529 with `Retry-After`; the LLM backends fail at
   once. Share one retry policy (Anthropic also returns 529 when overloaded).
6. **Session ids.** `ask --session <id>` joins the id into a path unchecked. Accept only a UUID.
   Session files hold the full state (source code); create them `0600`.
7. **Session expiry.** Pruning uses file mtime, reading uses `created_at`. Use `created_at` for both.
8. **Repo root.** `git show <rev>:<path>` paths are repo-relative but the command runs in the current
   directory. Resolve paths against `git rev-parse --show-toplevel`, or require running there and say so
   in the error.
9. **Markdown tables.** `table-column` splits on every `|`, so an escaped `\|` in a cell breaks the row.
   Split on unescaped pipes.
10. **Error ids as an enum.** Every `JevCheckError` code above becomes one enum variant with its fix text,
    so CLI errors stay typed (`{"error", "message"}` on stderr, exit 2) and nothing new appears ad hoc.
11. **Reply validation leniency.** Extra keys inside an answer are ignored and probabilities are not checked
    to sum to 1 or to match the stated confidence. Keep accepting them, but record a `consistent` flag per
    answer so a malformed but parseable reply is visible.
12. **Pinned versions in one place.** `anthropic-version` and `max_tokens` are constants in code, the
    models are in `jev.toml`. Put all vendor pins in the config file.
