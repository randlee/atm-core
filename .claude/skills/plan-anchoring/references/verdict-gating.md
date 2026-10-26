---
name: verdict-gating
version: 0.6.1
description: How jev_judge.py turns independent Jev answers into one verdict per anchor and item, the role contract, thresholds, severity order, pass criteria and exit codes, coverage, run diffs, and offline fixtures.
---

# Verdict gating

Jev cannot gate one question on another inside a request, and it cannot count. Both happen in `scripts/jev_judge.py`, deterministically.

## The role contract

A question set maps roles to question IDs:

```json
"roles": {"relevance": "relevant", "compliance": "addressed", "conflict": "contradicted"}
```

`relevance`, `compliance`, and `conflict` are required; `evidence` is optional, used by the development gate. Overrides change wording only; never rename question IDs or options. The script re-checks roles after merging every override and refuses a set whose roles do not resolve (`QUESTIONS.ROLES`) before any request leaves the machine.

## Anchor kinds

The gating depends on the anchor's `kind` (`anchor-capture.md`):

| Kind | Roles used | Why |
| --- | --- | --- |
| `directive` | relevance, compliance, conflict (+ evidence at the dev gate) | The table below |
| `prohibition` | relevance, conflict (+ evidence at the dev gate) | Silence is compliance; the generic `addressed` question says no on compliant beads (measured-behaviour.md) |
| `structural` | none at the planning gate; the anchor's `deterministic_check` | Verdict `deterministic_only`, non-failing; Jev cannot see code blocks as compliance (measured-behaviour.md) |

An anchor whose `exclude` list names the item (`--item-id`, exact match) is not asked at either gate: verdict `excluded`, non-failing, with `exclusion.reason` (`anchor-document.md`).

## From answers to a verdict

Each `choice` answer gives a `choice` and a `confidence` in [0, 1]. The threshold `t` defaults to 0.6 (`--threshold`, or `PLAN_ANCHOR_CONFIDENCE_THRESHOLD`). Evaluated in order; the first match wins:

| # | Condition | Verdict |
| --- | --- | --- |
| 1 | relevance is `no` with confidence >= t | `not_applicable` |
| 2 | conflict is `yes` with confidence >= t | `contradicted` |
| 3 | relevance or compliance is `unclear`, or either confidence < t | `underspecified` |
| 4 | compliance is `no` | `not_addressed` |
| 5 | compliance is `yes`, conflict is `no` with confidence >= t, and an evidence role exists and evidence is `no` with confidence >= t | `asserted_only` |
| 6 | compliance is `yes` and conflict is `no` with confidence >= t | `satisfied` |
| 7 | anything else, including a conflict answer that is `unclear` or below t | `underspecified` |

Contradiction outranks underspecification: a confident conflict is actionable even when relevance was shaky. Lower `t` to 0.5 only after every anchor's questions have passed tuning (`question-tuning.md`, measured-behaviour.md). A `noul` answer maps to yes/no at 0.5 with confidence `abs(p - 0.5) * 2`. Responses that fail validation never reach this table; they are retried, then recorded as `verdict: error`.

`min_confidence` on each result is the lowest confidence across the required roles; recurring `underspecified` means the questions need tuning. `leaning` is the verdict the choices alone would give at threshold 0, so an undecided cell still says which way it points. Cells with a required-role confidence within 0.10 of the threshold are re-asked once (`--confirm-near-threshold`, on by default); if the two verdicts disagree the cell is `underspecified` with `unstable: true` (noise: measured-behaviour.md).

## Severity order

`contradicted` > `not_addressed` > `asserted_only` > `underspecified` > `satisfied` > `not_applicable` > `deterministic_only` > `excluded`. `error` sorts first in reports and ranks below every verdict in diffs, so losing a verdict is a regression. Results are sorted by severity then anchor ID.

## Pass criteria and exit codes

- A **run** (one item) passes when no result is `contradicted`, `not_addressed`, `asserted_only`, `underspecified`, or `error`; `excluded` passes. `judge` exits 0 on pass or dry run, 1 on a failing run (the run file is still written), 2 on a fatal error before judging.
- The **planning gate** passes when every planning bead's latest run passes and `coverage` lists no uncovered locked anchor.
- The **development gate** passes when the sprint's latest run passes and each returned deterministic check has been run and recorded.

## Coverage, in code

```bash
python3 "$S" coverage --runs doc/plan/phase-current/anchor-runs --gate plan --format md
```

Takes the latest non-dry run per (gate, item), builds the anchor x item matrix, and lists anchors with no `satisfied` cell. Without `--gate`, columns are labelled `gate:item` so plan and dev verdicts are not merged. "Is ANC-012 satisfied anywhere?" is answered here, never by Jev. An anchor `not_applicable` to every bead is reported as uncovered on purpose: its scope is wrong or the plan lacks the bead that should carry it. Excluded cells show as `excluded` and are listed with their reasons (`data.excluded`); they never count as covering.

## Diffing runs

```bash
python3 "$S" diff --prev anchor-runs/plan-<older>-bd-42.json --next anchor-runs/plan-<newer>-bd-42.json
```

Lists changed, added, and removed verdicts for the same gate and item (`RUNS.MISMATCH` otherwise), with `pass_to_fail` and `fail_to_pass` lists (`regressions` is `pass_to_fail`), and pass before and after. Movement is not correctness: on a measured tuning round every `satisfied` to `not_addressed` flip was a correction (measured-behaviour.md). `--expected <json {anchor_id: verdict}>` reports corrections and regressions relative to what is known to be right. Run it after every plan edit. Two runs of one item in the same second get distinct file names (`-2`, `-3`).

## Fixtures and offline runs

Two different things are called fixtures:

- **Fixture sentences** on an anchor (`fixtures.pass`, `fixtures.fail`, set with `update --fixture-pass/--fixture-fail`): what a bead would say if it satisfied or violated the anchor. `check-fixtures --gate plan --all` sends each sentence to Jev as the item (same key and opt-in as `judge`) and reports whether it landed: pass -> `satisfied`, fail -> `not_addressed` or `contradicted`. Exit 1 on any mismatch. Use it after changing question wording.
- **Fixture answers** (`--answers-file`): Jev-shaped answers keyed by anchor ID (or `*`), injected instead of calling Jev. They exercise the gating table, report format, coverage and diffs offline; `assets/examples/answers.fixture.json` is one, and the maintainer-only `<repository root>/tests/sc-plan-anchor/test_jev_judge.py` uses them. For `check-fixtures`, keys are `<anchor>:pass:<n>` / `<anchor>:fail:<n>`.

`--dry-run` writes the exact requests, plus `totals {requests, bytes}`, to the run directory and no verdicts. Dry runs are never counted by `coverage`.
