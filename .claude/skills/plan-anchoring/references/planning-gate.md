---
name: planning-gate
version: 0.9.2
description: Running the planning gate end to end - what to pass, what Jev sees, the command and exit codes, and how to read results for one sprint item and a whole phase.
---

# Planning gate

Judges plan text. For each complete sprint item, every locked anchor is asked:
relevant, addressed, contradicted. Run after every plan revision; it catches the
compression failure before any code exists.

## Inputs

| Input | Source | Notes |
| --- | --- | --- |
| Planning item | the sprint text the caller already has, as `item_file`: the Markdown sprint file, `"$ANCHOR_TRACKER" show <id> --json` composed with `compose-item --show-json <file> --for plan`, or text from the caller's context | One agent run per complete sprint item; any layout is accepted |
| Anchors | `$A`: `docs/plans/phase-<current>/anchors.json` | `locked` and `promoted` only; `validate` must pass and each anchor's questions must have passed tuning (`question-tuning.md`) |
| Question overrides | each anchor's plan-questions bead | Fenced JSON in the bead description; `{}` means defaults |
| Script | `scripts/jev_judge.py` next to `SKILL.md`, absolute | Passed to the agent as `script` |

## What Jev sees per request

One anchor (`id`, `statement`, `scope`; the quote is not sent), one complete
sprint item as the caller passed it, and the three questions. The phase index/epic,
intent, source paths, tracker identifiers, revisions, and other sprints are not
sent.

## Run

Delegate each sprint item to the harness-specific `anchor-plan-review`
definition. Claude Code and Codex use their Agent tool. Cursor uses the Task
tool with no model override; the registered agent pins Composer. If that
subagent is not registered, stop. The planner does not substitute for
independent question tuning. The gate agent runs once per item:

```bash
python3 "$S" judge --anchors "$A" --gate plan --all --item-id <sprint-id> --item-title "<title>" \
  --item-file "$W/item.txt" [--overrides-file "$W/overrides.json"] --run-dir "$P/anchor-runs"
```

| Exit | Meaning |
| --- | --- |
| 0 | The run completed with an answer for every anchor; read `data.summary.pass`: true when every anchor is `satisfied`, `not_applicable`, `deterministic_only` or `excluded`, false on at least one `contradicted`, `not_addressed`, `asserted_only` or `underspecified` verdict; the run file was written. A dry run (`data.dry_run`) judges nothing and is never a pass: `data.summary.pass` is false |
| 2 | The tool could not do its job: credentials, `allow_remote_judge` not `true`, malformed record or override, unknown anchor, `PLAN.TOO_LARGE` (a symptom; `plan-size-and-shape.md`), or any anchor Jev gave no answer for after retries (`JEV.*`, `FIXTURE.MISSING`); the partial run is still written and `data.run_path` names it |

Then, across all sprint items:

```bash
python3 "$S" coverage --runs "$P/anchor-runs" --gate plan --anchors "$A" --format md
```

## Reading results

| Verdict | Jev decided | What settles it | Then |
| --- | --- | --- | --- |
| `contradicted` | The item proposes what the directive rules out | Re-read the sent text (`$W/item.txt`) at the governed section | Change the approach, or if the directive has changed, `retire` and lock a new anchor |
| `not_addressed` | The item is silent on the directive | Same | Add the directive's specific content: starting point, constraint, step |
| `underspecified` | Confidence below threshold or `unclear` | The row's `leaning` and `min_confidence`: low on one item means a vague item; low on many, untuned questions or a vague anchor | Fix the item where `to_fix` points, preferring cuts; tune the questions (`question-tuning.md`) or the anchor's `scope` only when it is low on many items that plainly comply |
| `excluded` | Not asked: the anchor's `exclude` names this item | `results[].exclusion.reason` | Nothing; drop it with `exclude --remove` if the reason is wrong |
| `deterministic_only` | Structural anchor; Jev not asked | The anchor's `deterministic_check` | Run the check and record it |
| `satisfied` | Directive present | Nothing | Nothing |
| `not_applicable` | Anchor does not govern this bead | If it should have, the anchor's `scope` is too narrow | `update --scope` |

To mark an anchor not relevant to one sprint item when its scope is right, use `exclude` (`anchor-document.md`), not a scope change or a smaller `--anchor` list. Check exclusion ids against the plan with `lint --plan-items <discover output>`.
| `error` | Jev could not be asked after retries | `results[].error.code` | Fix the cause; re-run that bead |
| `PLAN.TOO_LARGE` | The sprint does not fit one request | `shape` on the sprint text; scope-anchor verdicts | Judge the cause: unapproved work, then prose and ceremony, then split (`plan-size-and-shape.md`) |

The gate passes when every sprint's latest run has `summary.pass: true` and `coverage` lists no
uncovered locked anchor. An anchor uncovered everywhere has the wrong scope, or
the phase lacks the sprint that should carry it. Triage and stop conditions:
`triage-and-iteration.md`.

When the item has a tracker identity, post its report with the selected adapter;
otherwise keep the report beside the Markdown plan or attach it to the PR.
Compare revisions with `diff --prev <older run> --next <newer run>`.
