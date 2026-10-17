---
name: planning-gate
version: 0.6.1
description: Running the planning gate end to end - what to pass, what Jev sees, the command and exit codes, and how to read results for one sprint item and a whole phase.
---

# Planning gate

Judges plan text. For each complete sprint item, every locked anchor is asked:
relevant, addressed, contradicted. Run after every plan revision; it catches the
compression failure before any code exists.

## Inputs

| Input | Source | Notes |
| --- | --- | --- |
| Planning items | `plan_source.py discover-markdown <phase file/directory>` or `discover-tracker --tracker bd\|br --epic <id>` | One agent run per complete sprint document, in descriptor order |
| Anchors | `doc/plan/phase-current/anchors.json` | `locked` and `promoted` only; `validate` must pass and each anchor's questions must have passed tuning (`question-tuning.md`) |
| Question overrides | each anchor's plan-questions bead | Fenced JSON in the bead description; `{}` means defaults |
| Script | `scripts/jev_judge.py` next to `SKILL.md`, absolute | Passed to the agent as `script` |

## What Jev sees per request

One anchor (`id`, `statement`, `scope`; the quote is not sent), one complete
normalized sprint document, and the three questions. The phase index/epic,
intent, source paths, tracker identifiers, revisions, and other sprints are not
sent. For a tracker source, the planning item is only the copied issue
description; title, acceptance, design, and notes remain workflow metadata. See
`plan-source-contract.md`.

## Run

First discover the source and, when Markdown has an epic copy, verify parity:

```bash
P="$(dirname "$S")/plan_source.py"
python3 "$P" discover-markdown docs/plans/phase-a/
python3 "$P" discover-tracker --tracker br --epic pa-sza
python3 "$P" copy docs/plans/phase-a/ --tracker br --epic pa-sza
```

Delegate each ordered descriptor to the harness-specific `anchor-plan-review`
definition. Claude Code and Codex use their Agent tool. Cursor uses the Task
tool with no model override; the registered agent pins Composer. If that
subagent is not registered, stop. The planner does not substitute for
independent question tuning. The gate agent runs once per item:

```bash
python3 "$S" judge --anchors "$A" --gate plan --all --item-id <sprint-id> --item-title "<title>" \
  --item-file "$W/item.txt" [--overrides-file "$W/overrides.json"] --run-dir doc/plan/phase-current/anchor-runs
```

| Exit | Meaning |
| --- | --- |
| 0 | Every anchor `satisfied`, `not_applicable`, `deterministic_only` or `excluded`, or a dry run |
| 1 | At least one `contradicted`, `not_addressed`, `underspecified` or `error` verdict; the run file was written |
| 2 | Fatal before judging: credentials, opt-in, malformed record or override, unknown anchor, or `PLAN.TOO_LARGE` (a symptom; `plan-size-and-shape.md`) |

Then, across all sprint items:

```bash
python3 "$S" coverage --runs doc/plan/phase-current/anchor-runs --gate plan --format md
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
| `PLAN.TOO_LARGE` | The sprint does not fit one request | `plan_source.py shape <sprint path>`; scope-anchor verdicts | Judge the cause: unapproved work, then prose and ceremony, then split (`plan-size-and-shape.md`) |

The gate passes when every sprint's latest run exits 0 and `coverage` lists no
uncovered locked anchor. An anchor uncovered everywhere has the wrong scope, or
the phase lacks the sprint that should carry it. Triage and stop conditions:
`triage-and-iteration.md`.

When the item has a tracker identity, post its report with the selected adapter;
otherwise keep the report beside the Markdown plan or attach it to the PR.
Compare revisions with `diff --prev <older run> --next <newer run>`.
