# Phase bj plan gate: anchor runs and tuning notes

Date 2026-10-18. Worktree `plan/phase-bj` on PR #1875 (stacked on #1874). Anchors `docs/plans/phase-bj/anchors.json`, 75 records (ANC-001..074 carried from phase bi plus ANC-051..074 and REQ-BI6-AC6; ANC-062 retired to ANC-065); `validate` clean; `lint` 27 "too long" warnings (existing convention). Jev model answered `jev-1.13.0`, plan gate, default plan questions (`relevant`, `addressed`, `contradicted`), threshold 0.6, no question overrides (no question beads exist), `--allow-remote` per Rand's session opt-in. Items were composed with `compose-item --for plan` from the rendered bead JSON in `beads/plan.jsonl` (no import; title, description and acceptance criteria are sent).

## Fixture check, ANC-051..074 with the default questions

`check-fixtures --gate plan --id ANC-0nn` for each of the 24 anchors (runs under `anchor-runs/fixtures/`). 17 pass both fixtures. 7 pass-fixtures come back `underspecified`:

| Anchor | Fixture | addressed conf | contradicted | conf | Run |
| --- | --- | --- | --- | --- | --- |
| ANC-052 | pass:1 | 0.98 | no | 0.61 | `docs/plans/phase-bj/anchor-runs/fixtures/plan-20261018T223917Z-fixture_ANC-052_pass_1.json` |
| ANC-053 | pass:1 | 0.58 | no | 0.3 | `docs/plans/phase-bj/anchor-runs/fixtures/plan-20261018T223918Z-fixture_ANC-053_pass_1.json` |
| ANC-057 | pass:1 | 0.98 | no | 0.52 | `docs/plans/phase-bj/anchor-runs/fixtures/plan-20261018T223919Z-fixture_ANC-057_pass_1.json` |
| ANC-058 | pass:1 | 0.94 | no | 0.66 | `docs/plans/phase-bj/anchor-runs/fixtures/plan-20261018T223920Z-fixture_ANC-058_pass_1.json` |
| ANC-060 | pass:1 | 0.86 | no | 0.51 | `docs/plans/phase-bj/anchor-runs/fixtures/plan-20261018T223920Z-fixture_ANC-060_pass_1.json` |
| ANC-069 | pass:1 | 0.95 | no | 0.59 | `docs/plans/phase-bj/anchor-runs/fixtures/plan-20261018T223924Z-fixture_ANC-069_pass_1.json` |
| ANC-073 | pass:1 | 0.94 | no | 0.55 | `docs/plans/phase-bj/anchor-runs/fixtures/plan-20261018T223926Z-fixture_ANC-073_pass_1.json` |

Pattern: relevance yes at 1.0, addressed yes at 0.86 to 0.98 (ANC-053 0.58), and the `contradicted` role answering no at 0.30 to 0.66 on a one-sentence positive fixture. This is the question-defect class that `question-tuning.md` says to tune (the approved fixture demonstrates it). Tuning was not run:

1. `anchor_runner_contract.py resolve --plugin-root /Users/randlee/github/atm-core --harness claude --agent anchor-tuning` returns `RUNTIME.REGISTRY: agent registry lacks the requested agent` (no `anchor-tuning` entry in `.claude/agents/registry.yaml`), so the tune request's `runtime` block cannot be resolved.
2. The agent may not set `PLAN_ANCHOR_ALLOW_REMOTE_JUDGE` or pass `--allow-remote`, and the session environment of this run does not carry the opt-in, so its `check-fixtures` calls would stop at `POLICY.REMOTE_JUDGE_DISABLED`.

The gate ran with the default questions (phase-bi precedent). On the sprint items the same role produced one non-noise failing cell, bj-1 ANC-052 (round 2: addressed yes 0.89, contradicted no 0.52); one sentence in bj-1 citing ANC-052 resolved it (round 3).

## Anchor updates (ANC-051..074 statement defects)

| Anchor | Version | Change | Reason |
| --- | --- | --- | --- |
| ANC-055 | 2 to 3 | statement: "sanity-rejected message" to "sanity-fail message" | ANC-072 (Rand's later ruling) and `docs/requirements/task-close-hook.md` name the template `sanity-fail.xml.j2`; no sanity-rejected template exists |
| ANC-056 | 2 to 3 | statement, scope, deterministic check and pass fixture: `sanity-rejected(.md.j2)` to `sanity-fail.xml.j2` | same |

Both re-checked: `check-fixtures` green with the default questions after the update. Neither cell was failing before the update (both satisfied on bj-1 in round 1); the change is the name only.

## Gate rounds (sprint items)

| Item | Round 1 | Round 2 | Round 3 | Round 4 | Latest run |
| --- | --- | --- | --- | --- | --- |
| atm-bj-1 | 15 failing (`docs/plans/phase-bj/anchor-runs/plan-20261018T224029Z-atm-bj-1.json`) | 2 | 1 (ANC-063 relevance 0.59) | pass | `docs/plans/phase-bj/anchor-runs/plan-20261018T224823Z-atm-bj-1.json` |
| atm-bj-2 | 13 failing (`docs/plans/phase-bj/anchor-runs/plan-20261018T224029Z-atm-bj-2.json`) | 1 (ANC-073 relevance 0.64, unstable) | pass | - | `docs/plans/phase-bj/anchor-runs/plan-20261018T224719Z-atm-bj-2.json` |
| atm-bj-3 | 17 failing (`docs/plans/phase-bj/anchor-runs/plan-20261018T224028Z-atm-bj-3.json`) | 2 | 2 (ANC-049, ANC-072 unstable) | pass | `docs/plans/phase-bj/anchor-runs/plan-20261018T224821Z-atm-bj-3.json` |
| atm-bj-4 | 16 failing (`docs/plans/phase-bj/anchor-runs/plan-20261018T224147Z-atm-bj-4.json`) | 4 (icon anchors) | 1 (ANC-060 unstable) | pass; rounds 5 to 7 below | `docs/plans/phase-bj/anchor-runs/plan-20261018T225203Z-atm-bj-4.json` |

Round 1 to 2 fixes, by class:

- Plan defects fixed in the plan: bj-1 ANC-054 (the dev and fix checks were "PR for branch, pushed head, stack-bead top is ancestor"; cut to the anchor's three checks on `pr_target`, the hook reads no stack bead); bj-1 ANC-057 (the round-cap criterion said `sanity_round` 4 after five FAILs; now 5, checked bead not reopened on the fifth); bj-4 ANC-017/019/020/021/024/026 (the tabs print agent names, so the bead now states the icon table `icon_for()` already holds, phase-bi precedent); bj-4 ANC-022 (an `rg -w atm-bd-console` criterion; Jev reads the `atm-bd console` subcommand as the forbidden command without it).
- Exclusions (`exclude --reason`): recorded on the anchors file, listed with reasons in the coverage output. Classes: the anchor's owner is another bj sprint (hook roles are bj-1, stack bead and pour bj-2, link/subagent/timeout/lean bj-3, runs pages bj-4); atm-bd tool code the sprint does not edit (sender ANC-042/043, logging ANC-046/047, quota ANC-049, dispatch ANC-060/061); ANC-064 on every item (atm-core hook withheld by Rand, atm-core #1873); ANC-005 on every item (lands after the v0.16.1 tag; tags v0.16.1 and v0.16.2 exist on atm-bd-orchestration); the status table and dashboard anchors on bj-4 (ANC-028/034/036/038/050, REQ-BI6-AC6: phase-bi code bj-4 does not edit).
- Applicability noise, as phase bi documented: cells leaning not_applicable with relevance 0.58 to 0.64, changing from run to run (bj-1 ANC-005, ANC-063; bj-2 ANC-073; bj-3 ANC-046, ANC-049, ANC-060, ANC-072; bj-4 ANC-060). Each was excluded with a reason that holds (the item does not touch the anchor); no wording was changed for them.

Round 4 to 6, bj-4 only: coverage showed ANC-061 (dispatch reads `bd ready` first, then `atm task list`) satisfied by no item, and the phase-bi `dispatch.py` candidate filter checks ready and no-task from one snapshot without a stated read order. Added deliverable 8 to bj-4 (the read order, no second assign, with `dispatch.py` and `test_dispatch.py` in its owned paths) and the ANC-061 criterion; round 5: ANC-061 satisfied, two new relevance-noise cells (ANC-051 0.27, ANC-052 0.28) because the text now mentions dispatch assigning; one sentence stating dispatch's half of ANC-052 and its unchanged lead identity (ANC-051) for round 6: ANC-051 and ANC-052 satisfied, four new noise cells (ANC-040 0.56, ANC-043 0.48, ANC-055 0.66 unstable, ANC-066 0.59 unstable), excluded with reasons; round 7 pass.

Item growth (`diff --prev <round 1> --next <latest>`): bj-1 +359 chars (sentences citing ANC-052, ANC-054, ANC-060, ANC-061); bj-2 and bj-3 unchanged; bj-4 grew by the icon table (ANC-017..026), the ANC-022 criterion and the ANC-061 deliverable, every added passage citing a locked anchor that predates the run.

## Root epic

`atm-phase-bj` was judged once (`docs/plans/phase-bj/anchor-runs/plan-20261018T223945Z-atm-phase-bj.json`): satisfied 10, not_applicable 29, deterministic_only 3, underspecified 22, not_addressed 3, contradicted 1 (ANC-022: Jev reads the `atm-bd console` subcommand named in the root's criteria as the forbidden `atm-bd-console` command). The epic is not a planning item (`planning-gate.md`: the phase index/epic is not sent with a sprint), so it was not iterated; the 22 underspecified cells are the summary nature of the root text against anchors whose content lives in the sprint beads.

## Coverage

Every anchor ANC-051..074 except ANC-064 is satisfied by at least one sprint item (ANC-064 excluded everywhere: the atm-core hook is bj-5, withheld by Rand). Carried phase-bi anchors that no bj sprint touches (ANC-001, 003, 004, 006, 009..014, 018, 027, 028, 030..040, 042..050, REQ-BI6-AC6) are `not_applicable`, `deterministic_only` or excluded on every item and are listed by `coverage` as uncovered: they were satisfied by the phase-bi beads and no bj bead carries them by design.
