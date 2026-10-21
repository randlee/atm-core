# Sprint bl-2 — jev_client: request array in, typed JSONL out, busy-only retry

closure_type `contract`; target_boundary `BOUNDARY-JevResult` (the JSON that
`jev_client.py` prints, and the `JevErrorKind` enum behind its `error.kind`).
Branch `sprint/bl-2-jev-client`, PR → `integrate/phase-bl`. Wave 1; no dependency. Priority P1 (Jev errors or misuse).
`external_ref`: `gh-365,gh-497,gh-379,gh-164`.
recommended_agent `arch-ctm`; recommended_model `opus` (bl-5 consumes the
enum and the result shape).

Code under `plugins/atm-bd-orchestration/skills/atm-bd-orchestration/scripts/`
unless a path says otherwise. Line numbers are at v0.18.4 (86853d5).

## Requirement source (verbatim)

The two rulings below are dev-sanity requirements of atm-bd-orchestration
(`docs/requirements/dev-sanity.md`, PR in flight), not phase-bl anchors. This
sprint implements them; cite their requirement numbers here once that PR
lands.

Rand, 2026-10-21, on #365 (this replaces the review's "prompt rule" remedy;
no prompt rule is wanted):

> "on #365, prompt rule should not be at play here. the jev call should be returned. I do NOT want an agent 'intrepeting' what the jev python call returns. the return must be json and included in the final result/data written to sanity log. the elapsed time for the jev call is calculated in jev-py, jev-py includes retry for known API 'busy' error (it does not stupidly retry on other errors like out-of-tokens, context-too-large). jev should return an enum with all known errors which is exhaustively checked in match. jev should return # of retries in the case of api-busy. jev icon/error in tui dashboard should show 'do-not-enter' icon if jev did not return an evaluation so it is not confused w/ jev returning negative response (from a properly functioning model call)"

Rand, 2026-10-21, on #497 (this replaces every earlier #497 remedy, including
"pack parts by file" and "split inconclusive"):

> "then it is a poor design and will be removed. context packing LLM must split and send array of requests to jev."

Rand, 2026-10-21, on the child boundary: sc-sanity-jev returns the client
JSONL unchanged. It does not select the LLM reply; D5 selection belongs to
the mechanical merge in bl-5.

Consequences used below: all Python splitting is deleted; the context-packing
LLM (the dev-sanity side that holds the full context) writes one or more Jev
requests per deliverable; `jev_client.py` sends each request as given and
returns each Jev JSON unchanged under the #365 contract; a request over the
input budget is a named enum error, never cut or halved. No Python packer and
no file-list hybrid.

"Split inconclusive" is left out. It can only be computed by reading the
`choice` of every reply, which is interpreting verdicts, which the #365 ruling
forbids. It is not planned; it would need a new ruling.

The do-not-enter icon is bl-7's (console). Recording the request array and
the verbatim result in the sanity log, and the dev-sanity flow that writes the
array, are bl-5's. This sprint owns the client and the Jev child prompt.

## Dev-sanity requirements and relations

Requirements 51–56 govern this sprint ahead of the historic issue prose:
Jev receives the LLM-created request array unchanged, never splits it; the
launcher emits one JSONL response per request; only known busy failures retry;
all failure variants are stable closed tags; and downstream consumers preserve
those responses without interpreting a verdict. This doc uses those rules as
the authoritative contract.

**Relations.** `parallel_safe`: bl-1, bl-3, bl-4 (disjoint paths and no
consumed artifact). `must_follow`: none. bl-5 is a consumer and must follow
this sprint's named JSONL contract, not its files.

## Scope boundary

In: the client's input (a request array), its printed JSONL, the error enum
and its exhaustive matches, the retry policy, `retries`, `elapsed_s`, the
budget refusal, deletion of the splitting code, `sc-sanity-jev.md`, the null
PR body (#379, see deliverable 6), and `sanity-split --external-evidence`
(#164). Out: `sanity-merge`, `sanity-run-history`, `dev-sanity.md`,
templates, the console. No new plugin file, so `manifest.yaml` is untouched:
the enum lives in `jev_client.py`.

## Owned paths

- `scripts/jev_client.py`
- `scripts/wrap_jev_receipt.py`
- `scripts/sanity-split`
- `plugins/atm-bd-orchestration/agents/sc-sanity-jev.md`
- `scripts/tests/test_jev_client.py`, `scripts/tests/test_wrap_jev_receipt.py`,
  `scripts/tests/test_sanity_split.py`, `scripts/tests/test_agent_prompts.py`

bl-5 also edits `test_agent_prompts.py`; bl-5 stacks on this sprint.

## Public contract

The client is a launcher seam: it accepts the LLM-authored request array and
writes **JSONL**, exactly one JSON object per input request in input order.
Each line is self-contained and stable enough for a future Rust launcher to
mirror with an exhaustive enum match. There is no top-level envelope, no
Python exception name, and no display text that a consumer could interpret as
behaviour.

```python
class JevErrorKind(enum.Enum):
    BUSY_EXHAUSTED = "busy_exhausted"
    OUT_OF_TOKENS = "out_of_tokens"
    CONTEXT_TOO_LARGE = "context_too_large"
    TRANSPORT = "transport"
    HTTP_REJECTED = "http_rejected"
    RESPONSE_INVALID = "response_invalid"
    KEY_MISSING = "key_missing"
    KEY_INVALID = "key_invalid"
    INPUT_INVALID = "input_invalid"

# Every match on the enum is exhaustive and ends with assert_never(kind).
def error_fields(kind: JevErrorKind, *, status: int | None = None,
                 size: int | None = None, budget: int | None = None) -> dict[str, object]: ...
```

Command: `jev_client.py --requests <file>`, where the file is a JSON array of
Jev requests (each the shape `validate_request` accepts today, `:121-140`),
written by the LLM. `--startup` and `--announce` stay. `--assignment` and
`--request` are removed.

For every array element, stdout has one line with this tagged shape (newline
terminated; no surrounding array):

```json
{"index": 0, "request_sha256": "...", "success": true,
 "data": {"answers": {}}, "receipt": {"...": "..."}, "error": null,
 "retries": 0, "elapsed_s": 0.18}
{"index": 1, "request_sha256": "...", "success": false,
 "data": null, "receipt": null,
 "error": {"kind": "context_too_large", "size": 131072, "budget": 96000},
 "retries": 0, "elapsed_s": 0.0}
```

- `index`, `request_sha256`, `success`, `data`, `receipt`, `error`,
  `retries`, and `elapsed_s` are present on every line. `data` is Jev's JSON
  unchanged. The client never reads `choice`.
- `error.kind` is exactly one `JevErrorKind` stable string. Remaining fields
  are machine data for that kind (for example `status`, `size`, `budget`),
  never Python exception classes or message text that directs control flow.
- Each request has its own busy-only retry budget. `retries` counts only busy
  retries and `elapsed_s` is measured inside the client around that request.
- A request over the budget is not sent and gets its ordered
  `context_too_large` line; the other requests are still sent. A malformed
  array is refused before execution as `input_invalid` and exits 2.
- Exit status is 0 iff every request line is successful; otherwise 2. A
  consumer reads the ordered lines, never reconstructs a synthetic envelope.

## Authoritative deliverables

1. **Enum and exhaustive match (#365).** Add `JevErrorKind`. Every `JevError`
   carries a `kind`. `error_code` and `suggested_action` are each one `match`
   ending in `case _: assert_never(kind)`. The `FIXES` prefix table
   (`:48-69`) is replaced by the match.
2. **Busy-only retry and `retries` (#365).** `evaluate()` (`:155-196`)
   retries only when the status is in `BUSY_STATUSES`, honouring
   `Retry-After` through the injected `sleep`. A connection or timeout error
   raises `TRANSPORT` at once (today it is retried, `:172-175`). Any other
   non-200 raises `HTTP_REJECTED`; 413 raises `CONTEXT_TOO_LARGE`. The busy
   budget exhausted raises `BUSY_EXHAUSTED`. `OUT_OF_TOKENS` maps TypeSafe's
   quota signal, which the dev confirms from TypeSafe's API documentation or
   a recorded reply and cites in the PR; until confirmed, no status maps to it
   and the member exists for the match.
3. **Delete Python splitting (#497).** Remove `split_request`,
   `diff_pieces`, the `chunks` helper, `evaluate_parts` and its 413 halving
   (`:394-430`), `AttemptBudget` sharing between parts, `parts()`, the
   `assignment_requests` request builder and its pack loop (`:470-528`), and
   the `--assignment` / `--request` modes. Delete their tests.
4. **Request array in, JSONL results out (#497, #365).** Add `--requests <file>`:
   validate each element with `validate_request`; a non-array or invalid
   element fails the whole run as `INPUT_INVALID` naming the index. Refuse an
   element over `MAX_REQUEST_BYTES` as `CONTEXT_TOO_LARGE` without sending
   it. Send the rest in order, each with its own busy budget, and print exactly its one JSONL result line above. Receipts are computed per request as today (`receipt`,
   `:230-240`).
5. **No interpretation text (#365).** Remove "Selection takes the LLM reply
   for JEV slots until a Jev child succeeds." (`:629`). Rewrite
   `sc-sanity-jev.md` so the child receives the request array file the LLM
   wrote, runs `jev_client.py --requests <file>`, and returns the client's
   stdout unchanged through `wrap_jev_receipt.py`. Delete from it the
   per-part rule (`:63-67`, "When every part answers the same, that answer
   sets the findings...") and "selects the LLM reply for that deliverable"
   (`:125-127`). The child never reads `choice`. `wrap_jev_receipt.py` copies the ordered client JSONL result lines verbatim (today it copies `data.receipt` or `data.parts`).
6. **Null PR body (#379).** Deliverable 3 deletes the client's request
   builder, which is where `external_evidence()` (`:447-468`) runs. If the
   dev finds `external_evidence()` still called after deliverable 3 (by
   `sanity-split` or another script), map a `null` `body` from a successful
   `gh api` call to `""` there. If no caller is left, delete it with the
   builder; #379 then closes with criterion 2 below.
7. **`sanity-split --external-evidence` (#164).** Add a repeatable
   `--external-evidence <json-or-@file>` to the parser
   (`sanity-split:393-399`), validate each entry with the `EVIDENCE_REFS`
   shapes (`jev_client.py:432-444`, kept as data), and append it to the
   `external` list built at `sanity-split:423`, so the entries reach the
   context-packing LLM in its assignment.

## Authoritative acceptance criteria (per issue, deterministic)

gh-365
- [ ] `test_every_error_kind_has_stable_tag_and_fields`: iterates `JevErrorKind`; every emitted error has its enum value in `error.kind` and only kind-specific machine fields; a member without a `case` hits `assert_never` and fails.
- [ ] `test_busy_status_retried_and_counted`: fake connection returns 529, 529, 200; that entry is `success`, `retries == 2`; the injected `sleep` saw two calls.
- [ ] `test_non_busy_failures_not_retried`: 413, 401 and a raised `OSError` each make exactly one request, with kinds `context_too_large`, `http_rejected`, `transport` and `retries == 0`.
- [ ] `test_elapsed_measured_in_client`: injected clock gives `elapsed_s == 4.0` on a `busy_exhausted` entry, which has `retries == MAX_JEV_ATTEMPTS - 1`.
- [ ] `test_no_llm_selection_text` (in `test_agent_prompts.py`): neither `jev_client.py` nor `sc-sanity-jev.md` contains "selects the LLM reply" or "Selection takes the LLM reply".

gh-497
- [ ] `test_no_python_splitting`: `jev_client` has no attribute `split_request`, `diff_pieces`, `evaluate_parts`, `assignment_requests`; `--assignment` exits 2 as an unknown option.
- [ ] `test_requests_sent_as_given`: three requests produce three fake HTTP bodies byte-equal to `json.dumps` of each input element, in order, and exactly three JSONL result lines.
- [ ] `test_over_budget_request_refused_not_cut`: an element of `MAX_REQUEST_BYTES + 1` bytes is never sent, its entry is `context_too_large` with `size` and `budget`, and the other elements are sent.
- [ ] `test_jsonl_results_carry_jev_json_verbatim`: parsing each emitted JSONL line yields a `data` value equal to that request's fake reply object; no output line is an aggregate envelope.

gh-379
- [ ] Either `test_pr_body_null_is_empty_text` (a fake `gh api` printing `{"body": null}` with exit 0 gives evidence text `""`), when `external_evidence()` survives; or `test_external_evidence_removed` (no module in `scripts/` defines or calls `external_evidence`), when it is deleted.

gh-164
- [ ] `test_external_evidence_flag_appends`: two `--external-evidence` values (inline JSON and `@file`) appear, in order, after the bead-notes and PR-body entries in every rendered assignment.
- [ ] `test_external_evidence_flag_rejects_bad_shape`: `{"kind":"pr_body","repo":"o/r"}` exits 1 naming the entry.

## Authoritative validation

```bash
cd plugins/atm-bd-orchestration && python3 tests/gen_manifest.py --check && python3 -m pytest -q -rs
```

No test makes a network call or sleeps: the HTTP connection, `gh`, the clock
and `sleep` are injected fakes.

## Unverified

- TypeSafe's out-of-tokens signal and whether 502 is "busy" on that API.
  429/502/529 is today's set (`:181`).
- The exact input budget Jev enforces; `MAX_REQUEST_BYTES = 96000` (`:25`) is
  kept as the client's refusal threshold.

## Non-closure

This sprint does not close #365 or #497 alone: the LLM side that writes the
array and the sanity-log record are bl-5; the icon is bl-7.
