---
name: triage-and-iteration
version: 0.6.0
description: The loop at both gates - run, triage every failing verdict into plan defect, anchor defect, question defect or Jev error, apply the fix in the right place, re-run only what changed, diff, and stop conditions.
---

# Triage and iteration

A gate run is the first step of a loop: run, triage each failing verdict, fix where the defect is, re-run what changed, diff, repeat until the gate passes or a stop condition fires. The planning agent drives it at the planning gate; the quality manager at the development gate.

```
run -> report -> for each failing verdict: triage -> fix in the right place -> re-run affected beads -> diff -> coverage
         ^                                                                                                |
         +------------------------------------------------------------------------------------------------+
```

## 1. Run

Planning gate: one `anchor-plan-review` per bead (`planning-gate.md`). Development gate: one `anchor-sprint-review` per sprint (`development-gate.md`). Then `coverage` across the run directory. Post each report on its bead.

## 2. Triage every failing verdict

For each `contradicted`, `not_addressed`, `asserted_only`, `underspecified` or `error` row, re-read the sent text (`$W/item.txt` for a bead, the account for a sprint) where the anchor governs. Then decide which of these is wrong; only one is "the plan".

| Diagnosis | How you know | Fix where | Command |
| --- | --- | --- | --- |
| **Plan defect** | The bead really is silent about, or conflicts with, the statement | The bead | Cut conflicting or unapproved text first, replace unclear text second, and add only content required by a pre-existing locked ANC/REQ/ADR |
| **Anchor defect: statement** | Vague, bundled, or words no bead would use; `lint` warns; the same anchor fails on most beads | The anchor | `update --statement`, `lint`, re-test fixtures (`question-tuning.md`) |
| **Anchor defect: scope** | A frozen, user-approved expected verdict says `not_applicable` should apply, or `relevant: yes` appears on an approved unrelated example | The anchor, after user approval | `update --scope`; incremented anchor version invalidates prior runs for that anchor |
| **Question defect** | Frozen, user-approved fixtures or known examples land wrong; a live gate failure alone is insufficient | The question bead | Override the question (`question-tuning.md`), `check-fixtures`, re-run against the frozen set |
| **Oversized bead** (`PLAN.TOO_LARGE`) | The bead does not fit one Jev request after code reduction | The plan, in this order: unapproved work, then prose and ceremony, then split | `shape`, scope-anchor verdicts, the reviewing-ceremony skill when installed; see `plan-size-and-shape.md`. Never split to pass |
| **Jev error** | `verdict: error`; `results[].error.code` | Environment | `JEV.CONTEXT_EXCEEDED`: shorten the evidence; transport codes: re-run later (the script already retried three times); `AUTH`/`POLICY`: fix credentials or opt-in |

Rules of triage:

- Do not reword a bead until the statement and the question are ruled out; fixing the plan to satisfy a bad question entrenches it.
- Do not reword the statement to match a bead. Only the user changes what it requires; rewording for clarity (`update --statement`) keeps the quote.
- Low confidence is a plan-quality signal: the bead is unclear, excessive or missing something. Fix the bead; do not explain the score away.
- Before editing a bead, write one `to_fix` record: `{anchor_id, sentence, action: cut|replace|add, replacement, authority}`. `authority` is required for `add` and names a locked ANC/REQ/ADR that predates the failed run.
- Cuts are the default. Replace only when cutting would remove required meaning. Do not add explanation merely to lift confidence.
- Work the change cannot succeed without stays only when pre-existing authority requires it; list the cell, added line, and that authority in the planning-complete summary.
- If Jev looks wrong and no defect applies, say so in the posted report and ask the user. Do not iterate until Jev agrees.
- If two anchors fail together on every bead, run `dedupe` first.

## 3. Apply

One class of change per iteration, so the diff is attributable:

- Plan edits: change the bead text, then re-judge that bead.
- Anchor edits: change the statement or scope, `validate`, `render`, re-run `check-fixtures`, then re-judge every bead that was not `not_applicable` for that anchor.
- Question edits: the independent Opus/Sol tuner returns an oracle-backed override; the planner never edits it. Record the oracle digest, run `check-fixtures`, then re-judge affected beads as a regression check rather than a tuning target.

## 4. Re-run and diff

Re-run only the affected beads; the run directory keeps every run. Then:

```bash
python3 "$S" diff --prev anchor-runs/plan-<before>-<bead>.json --next anchor-runs/plan-<after>-<bead>.json
python3 "$S" coverage --runs anchor-runs --gate plan --format md
```

`diff` reports changed verdicts, `pass_to_fail`, `fail_to_pass`, and `item_size` (before, after, delta, grew); with `--expected` it reports corrections and regressions against known-correct verdicts. A failing-to-passing edit with `item_size.grew: true` is not accepted unless its `to_fix` record traces every added passage to a locked ANC/REQ/ADR that predates the failed run. A self-declared "required work" reason is not authority. A change on a bead and anchor you did not touch is noise before it is a regression (measured-behaviour.md). The script re-asks near-threshold cells once; a cell that still differs on unchanged input is `unstable`, not regressed.

## 5. Stop conditions

- **Pass.** Every bead's latest run exits 0 and `coverage` lists no uncovered locked anchor (planning gate); the sprint's latest run exits 0 and every deterministic check is recorded (development gate).
- **Hard stop: three identical failures.** Three consecutive runs of one bead with the same failing set. Stop editing; post the report and ask the user.
- **Hard stop: anchor churn.** A statement updated three times. Retire the anchor and lock a new one with the user.

## Recording the iteration

Each run file is the record. Post the final passing run's report on the bead or the PR, referencing the iterations' run IDs and each `to_fix` action. The final summary lists every allowed growth exception with its pre-existing authority; no exception first declared during remediation is accepted. If tuning was needed, identify the frozen fixture or expected verdict that proved the defect, what was tuned, and why.
