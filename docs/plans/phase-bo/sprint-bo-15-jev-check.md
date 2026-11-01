# Sprint bo-15: `jev-check.py`, a configured check script that calls Jev, Haiku or Luna

Repo: **atm-bd**, as its own package, separate from all atm-bd code. Branch `sprint/bo-15-jev-check`. No dependencies. Owned paths:
- `packages/jev-check/`: its own `pyproject.toml`, `src/jev_check/`, `scripts/jev-check.py`, `typings/` and tests. It imports nothing from atm-bd, atm-bd-types or `atm_bd_shared`, and contains no atm-bd vocabulary.
- `.atm-bd/atm-bd-lite/jev-checks/`: lite's check TOMLs.
- `.atm-bd/atm-bd-lite/scripts/atm-bd-lite-conformance.py`: lite's caller (deliverable 7), which depends on `jev-check` via a uv path source.

## Goal

Rand, 2026-11-01:
- "a use case which would take a intro context + requirement + code + questions and return an answer"
- "inputs get extracted in toml or json likely in a config folder"
- "the calling app ... would call jev w/ info in the contract then process the results and produce a output w/ additional value"
- "we can use the same tool and call haiku or luna. additionally we should get a session-id back that could be used to spin up haiku/luna and ask additional questions"

It is repo-neutral: lite is its first caller. sc-lint-rbp and raptor are later callers and are out of scope here.

## Existing design this sprint reuses (not redesigned)

- **Jev request and transport:** `jev_client.py` in atm-bd-orchestration (`parse_request`, `evaluate`, `ChoiceAnswer`, its error ids, busy-only retry, the 96,000-byte cap). The request is `{model, state, questions}`. `questions` is a map of orthogonal choice questions, answered independently in one call. An answer is `{choice, confidence, probabilities}` and carries no rationale. jev-check copies this code into its own package; it does not redesign it or import it from orchestration.
- **Questions as config:** the plan-anchor question-set format (`assets/questions/*.json`: choice `instructions` + `criteria`), tuned and fixture-checked the same way.

## Deliverables (only what is new)

1. **Check config.** One TOML file per check, in the calling app's config folder. Lite's files are `.atm-bd/atm-bd-lite/jev-checks/task-{assign,start,close}.toml` and `stack.toml`. A file names:
   - its inputs, as sources that become the `state`;
   - its question templates, in the question-set format.

   One check is ONE Jev request. A template is repeated once per extracted subject: `implemented:<row id>` and `listed:<path:lines>`. Only the state key it points at changes. An unknown key or source kind is a load error. A request over the cap is `INPUT_TOO_LARGE`, never split.
2. **Extractors.** Each reads `git show <rev>:<path>`.
   - `file`;
   - `table-column` (row ids and text);
   - `python-branches` (`case` arms and `return Refusal(...)`, via `ast`, with lines).

   An empty extraction is a typed error.
3. **Haiku and Luna backends.** They take the same request: the Anthropic API (`claude-haiku-5-5`) and OpenAI (the fast Codex model, id from config). Each forces the reply into the same per-question `{choice, confidence}` shape.
4. **Typed result.** `JevCheckResult {check_id, rev, backend, model, session_id, answers: {question key: choice, confidence}}`. Its types belong to the jev-check package; pyright strict. The caller turns it into its own output.
5. **Sessions.** Each run stores its request and response at `~/.cache/jev-check/sessions/<id>.json` (deleted after 7 days). `ask --session <id> --backend haiku|luna "<question>"` follows up with an LLM seeded from that request and response. This is the only place a "why" comes from.
6. **Lite's caller.** It maps `JevCheckResult` to the gate verdict:
   - `implemented:<row> = no` → "implement <row>";
   - `listed:<branch> = no` → "add a row for <branch>, or remove the branch".

   This is the ANC-053/054 check, which bo-14 runs on all four checks.

## Acceptance

- [ ] Config: one load test per error (unknown key, unknown source kind, missing input).
- [ ] Extractors: one test each, against a fixture git repo at a fixed commit. `python-branches` lists exactly the arms and refusals of a fixture hook. An empty extraction gives the typed error.
- [ ] Request: the request JSON for the fixture task-close check is asserted exactly: one request, one question per row and branch. An oversized fixture gives `INPUT_TOO_LARGE`.
- [ ] Haiku and Luna backends: each is tested against a fake transport, for reply mapping and its errors. No network calls and no sleeps.
- [ ] The question templates pass their approved fixtures (one implemented row and one missing row; one listed branch and one unlisted branch) on Jev.
- [ ] Sessions: a run writes the session; `ask` against a fake LLM backend sends the stored inputs and answers and appends the answer; an expired session is refused with a typed error.
- [ ] Lite caller: a result with one `no` row and one unlisted branch maps to exactly those two gate findings and fixes.
- [ ] pyright strict passes with 0 errors on all bo-15 Python.
- [ ] One live Jev run of `task-close.toml` against the bo-12 head, with its output JSON attached to the PR. This is evidence, not a test.

## Validation
`just lint`, `just test`, `pyright` (strict).
