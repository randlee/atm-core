---
name: planning-gate
version: 0.4.0
description: Running the planning gate end to end - what to pass, what Jev sees, the command and exit codes, and how to read the results for one bead and for the whole plan.
---

# Planning gate

Judges plan text. For each planning bead, every locked anchor is asked: relevant, addressed, contradicted. Run after every plan revision; cost is negligible and it catches the compression failure before any code exists.

## Inputs

| Input | Source | Notes |
| --- | --- | --- |
| Planning beads | open children of the phase epic: `bd children <epic> --json` | One agent run per bead; set aside sanity or QA beads |
| Anchors | `doc/plan/phase-current/anchors.json` | `locked` and `promoted` only; `validate` must pass and each anchor's questions must have passed tuning (`question-tuning.md`) |
| Question overrides | each anchor's plan-questions bead | Fenced JSON in the bead description; `{}` means defaults |
| Script | `scripts/jev_judge.py` next to `SKILL.md`, absolute | Passed to the agent as `script` |

## What Jev sees per request

One anchor (`id`, `statement`, `scope`; the quote is not sent), one bead's text composed as `# Title`, `## Description`, `## Acceptance criteria` (`compose-item --for plan`), and the three questions. Design and notes are not sent (measured-behaviour.md). The bead text is what `bd show --json` holds, never an agent's summary.

## Run

Delegate per bead using the harness-specific `anchor-plan-review` definition (Claude Code and Codex). Cursor may follow these steps inline, but cannot substitute the planner for independent question tuning. The gate agent runs once:

```bash
python3 "$S" judge --anchors "$A" --gate plan --all --item-id <bead> --item-title "<title>" \
  --item-file "$W/item.txt" [--overrides-file "$W/overrides.json"] --run-dir doc/plan/phase-current/anchor-runs
```

| Exit | Meaning |
| --- | --- |
| 0 | Every anchor `satisfied`, `not_applicable` or `deterministic_only`, or a dry run |
| 1 | At least one `contradicted`, `not_addressed`, `underspecified` or `error` verdict; the run file was written |
| 2 | Fatal before judging: credentials, opt-in, malformed record or override, unknown anchor, or `PLAN.TOO_LARGE` (a symptom; `plan-size-and-shape.md`) |

Then, across all beads:

```bash
python3 "$S" coverage --runs doc/plan/phase-current/anchor-runs --gate plan --format md
```

## Reading results

| Verdict | Jev decided | What settles it | Then |
| --- | --- | --- | --- |
| `contradicted` | The bead proposes what the directive rules out | Re-read the sent text (`$W/item.txt`) at the governed section | Change the approach, or if the directive has changed, `retire` and lock a new anchor |
| `not_addressed` | The bead is silent on the directive | Same | Add the directive's specific content: starting point, constraint, step |
| `underspecified` | Confidence below threshold or `unclear` | The row's `leaning` and `min_confidence`: low on one bead means a vague bead; low on many, untuned questions or a vague anchor | Fix the bead where `to_fix` points, preferring cuts; tune the questions (`question-tuning.md`) or the anchor's `scope` only when it is low on many beads that plainly comply |
| `deterministic_only` | Structural anchor; Jev not asked | The anchor's `deterministic_check` | Run the check and record it |
| `satisfied` | Directive present | Nothing | Nothing |
| `not_applicable` | Anchor does not govern this bead | If it should have, the anchor's `scope` is too narrow | `update --scope` |
| `error` | Jev could not be asked after retries | `results[].error.code` | Fix the cause; re-run that bead |
| `PLAN.TOO_LARGE` | The bead does not fit one request after code reduction | `shape` for the bead; scope-anchor verdicts | Judge the cause: unapproved work, then prose and ceremony, then split (`plan-size-and-shape.md`) |

The gate passes when every bead's latest run exits 0 and `coverage` lists no uncovered locked anchor. An anchor uncovered everywhere has the wrong scope, or the plan lacks the bead that should carry it. Triage and stop conditions: `triage-and-iteration.md`.

Post each bead's report on the bead: `python3 "$S" report --run <run> --format md > "$W/report.md" && bd comment <bead> --file "$W/report.md"`. Compare revisions with `diff --prev <older run> --next <newer run>`.
