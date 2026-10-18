# Phase bi anchor tuning notes

Working notes for a post-mortem once the phase bi anchors are tuned. Facts only: what was tried, what happened, what is still open. Newest entries at the bottom. Run files are in `anchor-runs/`; scratch inputs and answers of the haiku experiment are in the session scratchpad (`haiku/`), not committed.

## Setup

- 24 locked anchors at that run (ANC-001 to ANC-029 minus retired; 25 after ANC-030 and ANC-031 replaced ANC-029 on 2026-10-18), 6 beads judged: the root `atm-phase-bi` and sprints `atm-bi-1` to `atm-bi-5`. Plan gate, default plan questions (`relevant`, `addressed`, `contradicted`), threshold 0.6, no overrides, Jev model `jev-1.13.0`.
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

## The tuning loop (Rand 2026-10-17; to be added to the anchor-tuning process)

1. Run the gate on the failing beads; read each failing cell's `to_fix` and, where the cause is unclear, the one-cell haiku cross-check.
2. Where the bead is missing information the anchor needs, add it to the bead as context or a `## Prerequisites` section. Rand: adding it is allowed, and it must also serve the dev agent who executes the sprint, so the text is the instruction the dev needs and not wording aimed at the judge.
3. Never reword the anchor or its questions to pass; applicability noise is left alone.
4. Re-render, import, validate, and re-run immediately; compare per-cell verdicts to the previous run and record the outcome here.
5. Repeat until the beads pass. Three identical failing runs of one bead escalate to Rand.

## Interventions and outcomes

| Date | Change | Reason | Outcome |
| --- | --- | --- | --- |
| 2026-10-17 | bi-4: new `## Prerequisites` section listing what bi-1, bi-2, bi-3 and bi-5 deliver, each with its anchor ids | Jev and haiku cannot judge ANC-003, 011..014, 017..029 without being told what bi-4 depends on (Rand: provide the missing information; easiest as prerequisites) | Failing cells 13 -> 16. Relevance flipped to yes (0.85 to 0.98) for the icon and skill anchors, so the information landed. The failures moved to `addressed`: Jev now asks whether bi-4 checks the item in the published build or only ships it. Six cells that were `not_applicable` or low-relevance are now `underspecified` (ANC-012, 018, 021, 028, 029 and others). 5 satisfied (ANC-004, 010, 014, 017, 023). ANC-022 conflict 0.55 (was 0.46) |
| 2026-10-17 | root sprint table, bi-5 row: "agent icon immediately to the left of every agent name the console prints, plus QA and GitHub icons" | root row said only "agent, QA and GitHub icons", losing ANC-023 placement (haiku follow-up on root ANC-023) | Root failing cells 13 -> 9. ANC-023, 022, 005, 003, 004, 012, 013, 014 satisfied; ANC-018, 027, 028, 029 now `not_applicable`. Still failing: ANC-025 (not_addressed 0.66), ANC-010 (0.53), ANC-011 (0.43), ANC-017, 019, 020, 021, 024, 026. Jev `to_fix` for all nine is the same: the root bead gives no icon table (codepoints, colours, the contains rule, the fallback) or names where it lives |

### Finding from the re-run

The Prerequisites section answered the relevance question and exposed the real decision: for each anchor bi-5 delivers, does bi-4 check it in the published build, or only ship it? Jev's `to_fix` offers both ("add a criterion ... or say plainly that bi-4 asserts none of it"). Rand's rule "the icons are delivered and tested in bi-5; bi-4 only orders after it" is the second answer; it is not yet stated in the bead in those words. Not applied; awaiting Rand.

### Iterations 2 to 4 (2026-10-17, same day)

| Bead | Iter 1 (round 1) | Iter 2 | Iter 3 | Iter 4 | What changed |
| --- | --- | --- | --- | --- | --- |
| root | 13 | 9 | 0 (pass) | not re-run | icon table, offline/`vX.Y.Z` wording, then the status/state/PR icon lists in the root text |
| bi-5 | 2 | not run | 1 (ANC-005) | 0 (pass) | ANC-022 rg widened to every owned file; "changes only apps/atm-bd-console, after the v0.16.1 tag" |
| bi-4 | 13 | 16 | 15 | 16 | ownership statements; exact ANC-022 rg (passes from iter 3); "Not bi-4's work" list; ref/sha/version criterion |

What worked: stating the missing content in the bead where the bead owns it (root, bi-5) turned every cell satisfied at 0.74 to 1.0. Exact commands (ANC-022 rg) worked.

What did not work (bi-4): mentioning another sprint's topic in any form (ids only, ownership statements, an out-of-scope list) makes Jev answer `relevant: yes` at 0.34 to 0.67 and `addressed: no`; saying nothing leaves `relevant: no` at 0.3 to 0.56. Both fail because relevance below 0.6 is `underspecified`. Jev's `to_fix` offers only "state the content here" or "drop the anchor from this bead", and the first would duplicate bi-1/2/3/5 content in a publish-only sprint. Stopped at iteration 4 for Rand's ruling; no question or anchor wording was changed.

### Iterations 5 and 6 (2026-10-17): copy the owning content, flag drift

Rand's ruling: where bi-4 only orders after another sprint, copy that sprint's content into bi-4 (a "Not bi-4's work" list), and quality-mgr flags any drift between the copy and the owning bead. The bead states that bi-4 neither changes nor re-proves the items and returns any defect to the owning sprint.

| Iteration | bi-4 text | Jev result |
| --- | --- | --- |
| 5 | ids and words, no codepoints | 11 cells satisfied (skill-side anchors >= 0.8); 10 icon cells (ANC-018..021, 024..029) failing, all `addressed` low confidence; every `to_fix` says to state the codepoints, colours and rule |
| 6 | the same list plus every codepoint, colour and the contains/fallback rules, and the quality-mgr drift sentence | PASS: 21 satisfied, 3 deterministic-only; lowest confidence ANC-028 0.74, ANC-018 0.76, ANC-020 0.77 |

Haiku, run in parallel on the same request for ten cells (ANC-018..021, 024..029): relevant yes 0.80 to 0.95, addressed yes 0.65 to 0.90, contradicted no 0.85 to 0.95 on all ten. Jev and haiku now agree on every cell checked.

Haiku's suggestions, all optional: add an acceptance criterion in bi-4 that re-checks each copied icon on the installed console (ANC-019..021, 025..028); name where `GH_THEME` lives (ANC-018); state which name wins when a model string contains two listed names (ANC-024, the anchor gives no order). The first conflicts with the ruling that bi-5 owns and tests the icons and bi-4 neither changes nor re-proves them, so it is not applied. The other two are open for Rand.

### Modified tuning algorithm (Rand 2026-10-17)

Each iteration: run Jev and the one-cell haiku agents in parallel on the same bead text, compare per cell, discuss the differences with the haiku agents, then change the bead. When the gap is "this sprint depends on content another sprint owns", the fix is to copy that content into the dependent bead and leave drift checking to quality-mgr.

### Iterations 7 to 10: bi-1, bi-2, bi-3 (2026-10-17)

| Bead | Round 1 | Iter 7 | Iter 8 | Iter 9 | Iter 10 |
| --- | --- | --- | --- | --- | --- |
| bi-1 | 1 (ANC-004) | pass (ANC-004 satisfied at 0.62, near threshold) | - | - | - |
| bi-3 | 1 (ANC-005) | pass (lowest 0.74) | - | - | - |
| bi-2 | 5 (004, 005, 011, 012, 022) | 5 (003, 004, 011, 012, 014) | 6 (003, 004, 011, 012, 013, 014) | 4 (004, 011, 012, 014) | 4 (003, 011, 013, 014) |

What fixed bi-1 and bi-3: one sentence each saying what the sprint owns and what it leaves alone (ANC-004 publish is bi-4's and `atm-bd` is the name bi-4 publishes; the sprint changes nothing shipped in v0.16.1).

What did not fix bi-2: it builds the data the skill subcommand reads, so the skill anchors (ANC-003, 011..014) sit next to it. Each edit moved the failure instead of removing it. A long "Does Not Close" bullet naming the options made relevance flip to yes (0.27 to 0.59) with `addressed` no. One generic line ("all its options are bi-3's") returned relevance `no` at 0.45 to 0.59, which is below the 0.6 line and so `underspecified`. Jev's `to_fix` changes between runs: iteration 8 and 9 said add the ownership line, iteration 9 and 10 said to drop it or name each option. ANC-014 and ANC-011 were unstable on identical requests. Haiku on iteration 9 gave the same split (ANC-011 relevant yes 0.55 / addressed no 0.60; ANC-012 relevant no 0.60; ANC-013 relevant no 0.55; ANC-014 relevant yes 0.80 / addressed no 0.60). The same bead text gives answers on both sides of the 0.6 relevance line for both judges, so no wording of an ownership note settles it.

Open for Rand: bi-2 fails on relevance noise for the skill anchors. Options are (a) accept those cells as applicability noise for a non-owning bead, (b) scope the skill anchors (ANC-011..014) to the bead that owns them, which is an anchor change, or (c) keep editing bi-2. Not applied.

### bi-2 iterations 11 and 12, with the haiku follow-up turns

| Iteration | bi-2 text change | Jev failing | Haiku turn 1 (relevant / addressed) |
| --- | --- | --- | --- |
| 11 | per-option bullets deleted; one fence line "adds no subcommand and no option; builds the data `atm-bd skill` reads (bi-3)" | ANC-011 (relevant yes 0.54), ANC-013 (no 0.55 / 0.64, unstable) | 003 yes 0.93 / unclear 0.50; 011 yes 0.65 / no 0.85; 012 no 0.45; 013 no 0.60; 014 yes 0.65 / no 0.85 |
| 12 | fence line names the mode: "builds the bundled data that bare `atm-bd skill` installs from, offline (bi-3, ANC-010)" | ANC-011 (no 0.59), ANC-012 (no 0.71) | 011 yes 0.70 / no 0.90; 013 no 0.55 / no 0.92 |

Findings from the follow-up turns (iteration 9 and 11 agents, one cell each):
- All five agents said the anchors' scope ("adds or changes the --list option", the version argument, `--latest`, `--orchestration") is CLI behaviour that belongs to bi-3; they sit on bi-2 only because the bead mentions them.
- Cutting a per-option bullet moved `relevant` toward no (about 0.65 to 0.80 predicted). What remains is the bead naming `atm-bd skill` at all, which the ANC-003 line requires.
- Haiku's own `REF` suggestion for ANC-003 was its invention: nothing in the bead or the anchor asks for it. Not applied.
- Haiku's predictions from a counterfactual ("with the bullet deleted, no 0.65") did not match its later first-turn answer on the real edited text (yes 0.65). Treat counterfactuals as weak evidence; the first turn on the real text is the measurement.
- Process error recorded: I twice asked a fresh agent to reconcile numbers given by a different agent. A follow-up goes only to the agent that gave the answer, about its own answer.

Jev result by run, bi-2: 5, 5, 6, 4, 4, 2, 2 failing cells. The failing set changes every run and `to_fix` flips between "add" and "drop", so these cells sit on the 0.6 relevance line and wording moves them either side. Awaiting Rand: accept as applicability noise, scope ANC-011..014 to bi-3 (anchor change), or continue.

## plan-anchor 0.6.0 re-run and cross-check (2026-10-17)

Skill: develop's plan-anchor 0.6.0 install (verbatim copy used for the run, not committed to this branch). Gate steps run by hand (`compose-item`, `judge --all`) because the 0.6.0 gate agents refuse to set the remote-judge opt-in (`POLICY.REMOTE_JUDGE_DISABLED`); the opt-in was set per command.

| Bead | Result | Weakest cell |
| --- | --- | --- |
| atm-phase-bi | pass | ANC-020 0.80 |
| atm-bi-1 | pass (one gate agent run: ANC-004 underspecified 0.53, unstable; own run 0.69) | ANC-004 |
| atm-bi-2 | fail | ANC-011 underspecified (relevant no 0.37), ANC-012 underspecified (relevant no 0.59) |
| atm-bi-3 | pass | ANC-022 0.82 |
| atm-bi-4 | pass | ANC-020 0.64 |
| atm-bi-5 | pass | ANC-005, ANC-019 0.85 |

Cross-check on bi-2 (turn 1 = the exact Jev request only; follow-ups to the same agent):

| Agent | Cell | relevant (turn 1) | relevant (after follow-up) |
| --- | --- | --- | --- |
| haiku | ANC-011 | yes 0.6 | no 0.7 |
| haiku | ANC-012 | no 0.7 | no 0.7 |
| luna (lumen) | ANC-011 | yes 0.94 | no 0.98 |
| luna (cipher) | ANC-012 | no 0.7 | no (suggests an explicit "does not touch --latest" sentence) |

Findings: addressed is no and contradicted is no for every agent, so the only open question is applicability. Turn-1 answers split across the 0.6 line on identical text; all four agree after follow-up that bi-2 does not add or change the version argument or `--latest`. The shared `relevant` criterion counts "a clear prerequisite" as yes (assets/questions/plan.json), which is what pulled lumen's first answer. Not applied: the agents' suggested sentences (restating bi-3 options in bi-2 is the pattern that flipped cells before).

Rand's ruling (21:07Z, via omega-prime): the defect is that there is no formal way to signal "not relevant"; fix the channel, not the anchor. Filed as randlee/plan-anchor#35 (lead-declared not_applicable per anchor and bead, with reason and provenance in the run file). No anchor scope change; bi-2 is carried behind that issue.

## Mistakes made while tuning (for the post-mortem)

- Iterations 6 and 9: I put "why" and "best sentence to add" into the first haiku prompt and did not run follow-up turns. Turn 1 is then not Jev's exact request, and the discussion with haiku, which is where the tuning comes from, did not happen. Corrected by Rand; follow-up turns ran from iteration 9 on. Rule: turn 1 is the exact request only; every question goes in a later turn to the same agent.
- A follow-up goes only to the agent that gave the answer. I twice asked a fresh agent to reconcile numbers from a different agent; both pointed out it had no such record.
- Iteration 8 (bi-2): listing every skill option in one long "Does Not Close" bullet made things worse (5 to 6 failing cells). Mentioning an anchor's subject raises relevance without raising `addressed`.
- Early dispatches mixed script and anchors roots (preflight refusal); multi-cell haiku agents contaminated each other; `bd import` needs compact JSONL (`jq -c`), not the pretty render.
- Haiku's counterfactual predictions did not match its later first-turn answers on the real edited text. Edit and measure; do not predict.

## Rules followed while tuning

- No question or anchor wording was changed to make a cell pass; changes are to the bead text only, each citing a locked anchor that predates the run.
- Applicability noise (leaning `no`, relevance < 0.6) was not edited unless the bead was missing information the anchor needs.
- A rule change so that an unsure relevance `no` counts as `not_applicable` would be a change to the shared plan-anchoring skill; not made here.

## To fill in at the post-mortem

- Final per-bead verdicts after the prerequisites change.
- Whether adding a Prerequisites section is enough for ordering-only beads, or whether the questions need a ruling.
- ANC-022: whether the rg criterion in bi-4 and bi-5 needs the wording Jev and haiku asked for (pattern, scope, exclusions).

## atm-bi-6 (2026-10-18, after plan review round 2)

The layout C table sprint moved from atm-phase-bi.2 to atm-bi-6 and lost its restated glyph table (plan review round 2). Seven cells then failed or flipped between runs (ANC-004, 005, 020, 021, 024, 027, 028). anchor-tuning adjudicated: six were plan-text gaps, fixed with one clause naming what bi-6 carries unchanged from bi-5 (ANC-020/027/028), one sentence on the gavel colours (ANC-021), the contains-match rule spelled out so `agent_icon_of` is not read as `AgentIcon(raw)` (ANC-024), and the bi-4 ordering sentence deleted (ANC-004 is bi-4's). Run plan-20261018T033358Z-atm-bi-6: 14 satisfied, 7 not applicable, 3 deterministic-only, 1 underspecified.

The remaining cell is ANC-005 ("no sprint changes what ships in v0.16.1"): addressed yes 1.00, contradicted no 0.92, relevance yes 0.46 to 0.61 over seven runs of three wordings (the same sentence scored 0.76 on atm-phase-bi.2 and 0.85 on bi-5). The anchor applies to every sprint, but the plan relevance question asks whether the work touches the anchor's subject, so for a sprint that never touches the release the answer is noise around the threshold. Not excluded (the anchor does apply). Recommendation for Rand: a relevance override on ANC-005, "Is this item a sprint dispatched in this phase?", then version the anchor and re-run check-fixtures. Same gap as randlee/plan-anchor#35.

## atm-bi-6 deliverable 11 (2026-10-18, sanity finding atm-bi-6.group-dev.1)

Six anchors added from the spec's own words (develop 914bda28ec5c1515a9b909052c7afab0b14ab1cf): ANC-035 state precedence (agent wins over sprint state), ANC-036 one icon per active agent, ANC-037 ci cell values, ANC-038 pr cell form, ANC-039 `atm-bd cron --md-table`, ANC-040 spec table form and byte determinism. Ruling A's anchor is not added here: its fix (atm-6jf) moved to bi-7 on Rand's "do not block publishing" ruling, so it is written with bi-7's anchors.

Fixture check at the plan gate: ANC-036, 037, 038 pass both fixtures. ANC-035 and ANC-039 fail-fixtures and both ANC-040 fixtures come back underspecified (fixture sentence too thin for the relevance question), not wrong-way. Run plan-20261018T053418Z-atm-bi-6 against the approved bi-6 text: 19 satisfied, 7 not applicable, 3 deterministic-only, 5 underspecified (ANC-035 addressed no 0.51; ANC-037/038/039/040 addressed yes 0.82 to 0.89, contradicted no 0.56 to 0.73, ANC-038 unstable). bi-6's plan review passed before these anchors existed; the five cells are adjudicated by anchor-tuning (no anchor or question wording changed toward a pass) and the result is recorded in a follow-up, not held against the dev-complete sprint.
