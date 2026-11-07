---
version: 0.9.2
---

# Planning-review workflow

### C. Planning gate

Read `references/planning-gate.md` and `references/triage-and-iteration.md`.
Judge one complete sprint item at a time, as you wrote it; never concatenate the
phase index, epic, intent, or another sprint into the item. Jev's feedback is the measure of plan quality, and this loop is how the
planner raises it: run Jev, fix the plan, run again, until every item passes. A
failing verdict or a low confidence means the item is unclear, excessive or
missing something, and the planner acts on it; it is never explained away. The
usual fix is to simplify: cut work no anchor asks for, cut prose and ceremony,
and state what remains plainly. Add text only where Jev found something missing.

1. **Run.** Pass the sprint text you already have, exactly as written: the Markdown sprint file itself, or for a tracker plan `"$ANCHOR_TRACKER" show <id> --json > "$W/bead.json"` then `python3 "$S" compose-item --show-json "$W/bead.json" --for plan --out "$W/item.txt"`, or text from your own context written to a file. Any layout is accepted; the reviewer never fetches or rebuilds it. Delegate `script`, `item_id`, `item_title`, `item_file`, `anchor_tracker`, and the anchor selection to `anchor-plan-review` (see Agent Delegation). Obtain anchor storage (`bd`) from `anchor_tracker.py resolve` (SKILL.md Step 1), independently of where the plan lives; never hardcode it, and stop on a resolve error. The reviewer judges the text unchanged, resolves question overrides with the selected tracker, runs the script once, and returns unchanged verdicts plus `to_fix`. Keep reports beside the plan, on the PR, or on its tracker item when one exists.
2. **Triage** every failing verdict: re-read the sent text, then decide whether it is a plan defect (the bead), an anchor defect (statement or scope), a question defect (wording), or a Jev error. Before editing, record `to_fix` with the anchor ID, the exact weak or conflicting sentence, and one action (`cut`, `replace`, or `add`). Prefer cuts, then replacement. Adding text is allowed only for content required by a locked ANC/REQ/ADR that predates the failed run; cite that ID and its source. A reason written after the failure does not create scope. Only a plan defect is fixed in the plan. `PLAN.TOO_LARGE` is a symptom: run `shape`, remove unapproved work, cut prose and ceremony, and split only what is still too large (`references/plan-size-and-shape.md`). Never split or trim merely to pass the gate.
3. **Apply** one class of fix per iteration: edit the bead, or `update --statement/--scope` and re-test fixtures, or tune the question bead and re-run `check-fixtures`.
4. **Re-run** only the affected beads, `diff` each against its previous run, and recompute coverage in code, never by asking Jev. `diff.item_size` must be reviewed: a failing-to-passing edit that grew is not accepted unless every added passage cites a pre-existing locked ANC/REQ/ADR; otherwise cut or replace instead.

```bash
python3 "$S" coverage --runs "$P/anchor-runs" --gate plan --anchors "$A" --format md
```

5. **Stop** when every bead's latest run passes and every locked anchor is `satisfied` by at least one bead. Report any allowed growth with its pre-existing authority when planning is complete. A hard stop also ends the loop: three identical failing runs of one bead, or an anchor reworded three times. Question design: `references/plan-anchor-questions.md`. Verdict logic: `references/verdict-gating.md`.

### E. Report

Reports quote the user's own words back (`references/report-format.md`). Post the markdown from `report --run <run> --format md` as a comment on the judged bead (using the selected tracker adapter) or on the PR. Posting happens in the main session; the gate agents never write to beads.
