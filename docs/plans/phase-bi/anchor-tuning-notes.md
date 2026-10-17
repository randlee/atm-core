# Phase bi anchor tuning notes

Working notes for a post-mortem once the phase bi anchors are tuned. Facts only: what was tried, what happened, what is still open. Newest entries at the bottom. Run files are in `anchor-runs/`; scratch inputs and answers of the haiku experiment are in the session scratchpad (`haiku/`), not committed.

## Setup

- 24 locked anchors (ANC-001 to ANC-029 minus retired), 6 beads judged: the root `atm-phase-bi` and sprints `atm-bi-1` to `atm-bi-5`. Plan gate, default plan questions (`relevant`, `addressed`, `contradicted`), threshold 0.6, no overrides, Jev model `jev-1.13.0`.
- Gate rule that matters (verdict-gating.md): relevance `no` at confidence >= 0.6 is `not_applicable` (passes). Below 0.6 the cell is `underspecified` and the run fails.

## 2026-10-17 round 1 (after plan review PASS round 3)

| Bead | Failing cells | Shape of the failures |
| --- | --- | --- |
| atm-bi-1 | 1 (ANC-004) | relevance yes at 0.33, addressed no at 0.94 |
| atm-bi-2 | 5 (ANC-004, 005, 011, 012, 022) | relevance no at 0.30 to 0.45 for four; ANC-022 conflict 0.45 |
| atm-bi-3 | 1 (ANC-005) | relevance yes at 0.52 |
| atm-bi-4 | 13 | 11 low-confidence relevance (leaning no), ANC-022 conflict 0.46 |
| atm-bi-5 | 2 (ANC-005, 022) | relevance 0.5, conflict 0.46 to 0.52 |
| root | 13 | ANC-024..026 confident not_addressed; ANC-010/011/017..023 low compliance; ANC-027..029 low relevance |

### Tooling problems

- First three dispatches failed at preflight (`VALIDATION.PATH`): the agent preflight accepts a script and an anchors file only under one repository root. The anchors live in the plan worktree, so the script path must be the worktree's own copy of `jev_judge.py` (byte-identical to the main checkout's). Cost: one wasted round of three agents. Fix: dispatch with the worktree script and the worktree as git root.
- The gate agents return abbreviated JSON (dropped `answers` for passing cells, estimated `duration_ms`). The run files hold the full answers; read them there.

## Haiku cross-check

Question being tested: do other models, given the exact Jev request, answer like Jev? If not, the difference is a separate investigation (Rand's framing).

Method:
1. For every failing cell, `jev_judge.py judge --dry-run` wrote the exact request (`state` with anchor and bead text, and the three questions). Item lengths matched the run files for all 35 cells.
2. One haiku agent per cell, one request file each, told to read only that file and not the repository. Answers: choice and probability per question.
3. Mismatched cells were then resumed with a follow-up: why that answer (quote the deciding words), and critical feedback on the bead for that anchor.

What did not work: the first attempt gave one haiku agent all cells of a bead. Rand's correction: one prompt per agent, otherwise the answers are contaminated by the other cells and plan text. Those answers were discarded and the agents stopped. Bead bi-1 and bi-3 had one cell each so their first-run answers were kept.

Result (35 cells, 105 answers):

| Question | Choice differs from Jev | Notes |
| --- | --- | --- |
| relevant | 15 | all 15 are Jev `no` (0.30 to 0.56) against haiku `yes` (0.50 to 0.80) |
| addressed | 2 | root ANC-017 (Jev yes 0.06, haiku no) and ANC-023 (Jev no 0.55, haiku yes 0.60) |
| contradicted | 2 | haiku `unclear` where Jev says `no` at 0.56 and 0.62 |

- Confidence on `contradicted`: haiku is more confident than Jev on several cells (Jev 0.45 to 0.59, haiku 0.80 to 0.92), which is why the ANC-022 conflict cells look noisy only on the Jev side.
- Haiku's `relevant: yes` means the bead depends on the anchor's subject (for example "after bi-3 and bi-5, so the icons ship"). Jev reads "different part of the system" as `no`. Under haiku's reading the cells would fail as `not_addressed` instead of `underspecified`, so the run still fails.
- Haiku's critique and Jev's `to_fix` name the same gaps for every cell: the bead states a dependency or ordering but not what the earlier sprint delivers, so neither judge can tell whether the anchor is carried.

Open question, not investigated: why Jev's relevance answers differ from haiku's on ordering-only beads (Jev-side behaviour, not a plan defect).

## Interventions and outcomes

| Date | Change | Reason | Outcome |
| --- | --- | --- | --- |
| 2026-10-17 | bi-4: new `## Prerequisites` section listing what bi-1, bi-2, bi-3 and bi-5 deliver, each with its anchor ids | Jev and haiku cannot judge ANC-003, 011..014, 017..029 without being told what bi-4 depends on (Rand: provide the missing information; easiest as prerequisites) | Failing cells 13 -> 16. Relevance flipped to yes (0.85 to 0.98) for the icon and skill anchors, so the information landed. The failures moved to `addressed`: Jev now asks whether bi-4 checks the item in the published build or only ships it. Six cells that were `not_applicable` or low-relevance are now `underspecified` (ANC-012, 018, 021, 028, 029 and others). 5 satisfied (ANC-004, 010, 014, 017, 023). ANC-022 conflict 0.55 (was 0.46) |
| 2026-10-17 | root sprint table, bi-5 row: "agent icon immediately to the left of every agent name the console prints, plus QA and GitHub icons" | root row said only "agent, QA and GitHub icons", losing ANC-023 placement (haiku follow-up on root ANC-023) | Root failing cells 13 -> 9. ANC-023, 022, 005, 003, 004, 012, 013, 014 satisfied; ANC-018, 027, 028, 029 now `not_applicable`. Still failing: ANC-025 (not_addressed 0.66), ANC-010 (0.53), ANC-011 (0.43), ANC-017, 019, 020, 021, 024, 026. Jev `to_fix` for all nine is the same: the root bead gives no icon table (codepoints, colours, the contains rule, the fallback) or names where it lives |

### Finding from the re-run

The Prerequisites section answered the relevance question and exposed the real decision: for each anchor bi-5 delivers, does bi-4 check it in the published build, or only ship it? Jev's `to_fix` offers both ("add a criterion ... or say plainly that bi-4 asserts none of it"). Rand's rule "the icons are delivered and tested in bi-5; bi-4 only orders after it" is the second answer; it is not yet stated in the bead in those words. Not applied; awaiting Rand.

## Rules followed while tuning

- No question or anchor wording was changed to make a cell pass; changes are to the bead text only, each citing a locked anchor that predates the run.
- Applicability noise (leaning `no`, relevance < 0.6) was not edited unless the bead was missing information the anchor needs.
- A rule change so that an unsure relevance `no` counts as `not_applicable` would be a change to the shared plan-anchoring skill; not made here.

## To fill in at the post-mortem

- Final per-bead verdicts after the prerequisites change.
- Whether adding a Prerequisites section is enough for ordering-only beads, or whether the questions need a ruling.
- ANC-022: whether the rg criterion in bi-4 and bi-5 needs the wording Jev and haiku asked for (pattern, scope, exclusions).
