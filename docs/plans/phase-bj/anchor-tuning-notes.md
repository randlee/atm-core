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
- Exclusions (`exclude --reason`): recorded on the anchors file, listed with reasons in the coverage output. Classes: the anchor's owner is another bj sprint (hook roles are bj-1, stack bead and pour bj-2, link/subagent/timeout/lean bj-3, runs pages bj-4); atm-bd tool code the sprint does not edit (sender ANC-042/043, logging ANC-046/047, quota ANC-049, dispatch ANC-060/061); ANC-005 on every item (lands after the v0.16.1 tag; tags v0.16.1 and v0.16.2 exist on atm-bd-orchestration); the status table and dashboard anchors on bj-4 (ANC-028/034/036/038/050, REQ-BI6-AC6: phase-bi code bj-4 does not edit).
- Applicability noise, as phase bi documented: cells leaning not_applicable with relevance 0.58 to 0.64, changing from run to run (bj-1 ANC-005, ANC-063; bj-2 ANC-073; bj-3 ANC-046, ANC-049, ANC-060, ANC-072; bj-4 ANC-060). Each was excluded with a reason that holds (the item does not touch the anchor); no wording was changed for them.

Round 4 to 6, bj-4 only: coverage showed ANC-061 (dispatch reads `bd ready` first, then `atm task list`) satisfied by no item, and the phase-bi `dispatch.py` candidate filter checks ready and no-task from one snapshot without a stated read order. Added deliverable 8 to bj-4 (the read order, no second assign, with `dispatch.py` and `test_dispatch.py` in its owned paths) and the ANC-061 criterion; round 5: ANC-061 satisfied, two new relevance-noise cells (ANC-051 0.27, ANC-052 0.28) because the text now mentions dispatch assigning; one sentence stating dispatch's half of ANC-052 and its unchanged lead identity (ANC-051) for round 6: ANC-051 and ANC-052 satisfied, four new noise cells (ANC-040 0.56, ANC-043 0.48, ANC-055 0.66 unstable, ANC-066 0.59 unstable), excluded with reasons; round 7 pass.

Item growth (`diff --prev <round 1> --next <latest>`): bj-1 +359 chars (sentences citing ANC-052, ANC-054, ANC-060, ANC-061); bj-2 and bj-3 unchanged; bj-4 grew by the icon table (ANC-017..026), the ANC-022 criterion and the ANC-061 deliverable, every added passage citing a locked anchor that predates the run.

## Root epic

`atm-phase-bj` was judged once (`docs/plans/phase-bj/anchor-runs/plan-20261018T223945Z-atm-phase-bj.json`): satisfied 10, not_applicable 29, deterministic_only 3, underspecified 22, not_addressed 3, contradicted 1 (ANC-022: Jev reads the `atm-bd console` subcommand named in the root's criteria as the forbidden `atm-bd-console` command). The epic is not a planning item (`planning-gate.md`: the phase index/epic is not sent with a sprint), so it was not iterated; the 22 underspecified cells are the summary nature of the root text against anchors whose content lives in the sprint beads.

## Coverage

Every locked anchor ANC-051..074 is satisfied by at least one sprint item (ANC-064 retired to phase bk). Carried phase-bi anchors that no bj sprint touches (ANC-001, 003, 004, 006, 009..014, 018, 027, 028, 030..040, 042..050, REQ-BI6-AC6) are `not_applicable`, `deterministic_only` or excluded on every item and are listed by `coverage` as uncovered: they were satisfied by the phase-bi beads and no bj bead carries them by design.

## Question tuning (plan-anchoring step B2), 2026-10-18, fenix

Run after the first round, with the plan-anchoring agent registry fix of PR #1876 (worktree `fix/anchor-agent-registry`, head e7a8ec2a08afccb120ff193bb5199696b081eb44) as `--plugin-root`; every anchor and question write was made in this worktree, by the main session, never by the tuner. `--allow-remote` per Rand's session opt-in, Jev answered `jev-1.13.0` throughout.

Testability rubric: no rubric seal record exists in this repository; the rubric shipped with the skill (v2) was used unchanged, as phase bi did. Recorded here instead of running `testability`, which would have produced a seal for a rubric nobody changed.

### Tuned anchors

One `anchor-tuning` (opus) run per anchor from the tune-request template, INTENT and the ANC packet, never a desired verdict. Each result was checked with `validate-fence` (one fence) and `anchor_tuning_contract.py result` (valid), attested, and the selected set written as `question_overrides.plan` on the anchor record, version unchanged. Evidence per anchor under `docs/plans/phase-bj/anchor-runs/tuning/<ANC>/` (`result.md` byte copy, `attestation.json`, `tuning-history.json` canonical JSON whose sha256 equals the result's `tuning_history_after_sha256`).

| Anchor | Selected bundle | Sealed digest | Tournament score (frozen oracle, two repeats) | History sha256 |
| --- | --- | --- | --- | --- |
| ANC-052 | `planning.relevant-c0.addressed-c2.contradicted-c4` | `fd2fb8d6bbd1f6c1` | 78/78 cells, 28/28 verdicts, 14/14 stable, min confidence 0.34 | `6f11094aa1c66d89` |
| ANC-053 | `B7` | `3d1becff13384172` | 72/72 cells, 24/24 verdicts, 12/12 stable, min confidence 0.33 | `141f22086599d7a6` |
| ANC-057 | `B4:relevant=R3,addressed=A4,contradicted=C4` | `91c20aff90415d56` | 78/78 cells, 26/26 verdicts, 13/13 stable, min confidence 0.55 | `6d277e0ea204af00` |
| ANC-058 | `CE` | `e667bf6dafb8cab7` | 54/54 cells, 18/18 verdicts, 9/9 stable, min confidence 0.83 | `de6d542bbe6fb3a4` |
| ANC-060 | `K1` | `6badd41125d89853` | 33/33 cells, 11/11 verdicts, 11/11 stable, min confidence 0.46 | `4b5373031f27c4e2` |
| ANC-069 | `B-r1a1c1` | `33764582ae268e29` | 80/80 cells, 28/28 verdicts, 14/14 stable, min confidence 0.53 | `8a968b5dc4e165fc` |
| ANC-073 | `B-r1a2c1` | `7aee727422a5cfa6` | 98/98 cells, 36/36 verdicts, 18/18 stable, min confidence 0.58 | `8e98f5b72dc6da9e` |

The other 15 locked anchors of ANC-051..074 (051, 054, 055, 056, 059, 061, 063, 065, 066, 067, 068, 070, 071, 072, 074) keep the default plan questions: their fixtures were green on the default set in the first round. ANC-062 and ANC-064 are retired.

Authority recommendations returned by the tuners (recorded, not applied; the statements were not changed):
- ANC-052 (clarity, applied: None): ambiguity: "The statement requires assignment 'by the dev's close hook ... and by atm-bd ...' but does not say whether a single plan item must carry both paths. A hook-only or atm-bd-only item is not determined: the sealed questions judge such items not_addressed (hook-only is unstable between not_addressed and underspecified), while the default questions judged them satisfied." Proposed text: "When a dev or fix bead closes, the group's sanity bead must be assigned to the dev-sanity member with the sanity template and vars without any lead action: by the dev's close hook in the same shell command as bd close, and by atm-bd when bd ready lists the sanity bead with no ATM task. A plan item carries this directive only when it plans both paths." Why meaning unchanged: "None"
- ANC-073 (clarify, applied: False): ambiguity: "The statement does not say that dev-sanity's reply stops rendering the runs table, though the deterministic check and the approved fail fixture require it; an item carrying only that reply change has no statement text to be judged against (diagnostic diag-reply-only)." Proposed text: "Append to the statement: "; dev-sanity's reply carries only the verdict and no longer renders the runs table."" Why meaning unchanged: "The same requirement is already locked in ANC-073 v2's deterministic_check and fail fixture; the edit only moves it into the judged statement."

### Fixture results with the sealed sets (production, threshold 0.6, confirmation re-judge on)

| Anchor | pass fixture | fail fixture | Run files |
| --- | --- | --- | --- |
| ANC-052 | satisfied 0.97 | contradicted 1.0 | `fixtures/plan-20261018T232017Z-fixture_ANC-052_{pass,fail}_1.json` |
| ANC-053 | underspecified (contradicted no 0.28; repeat 0.30), leaning satisfied | contradicted 0.95; repeat 0.92 | `fixtures/plan-20261018T231821Z-…_ANC-053_…`, `fixtures/plan-20261018T232113Z-…_ANC-053_…` |
| ANC-057 | underspecified (addressed yes 0.46; repeat 0.45), leaning satisfied | contradicted 1.0 | `fixtures/plan-20261018T231904Z-…_ANC-057_…`, `fixtures/plan-20261018T232113Z-…_ANC-057_…` |
| ANC-058 | satisfied (addressed 0.97, contradicted 0.97) | contradicted 0.92 | `fixtures/plan-20261018T232522Z-fixture_ANC-058_{pass,fail}_1.json` |
| ANC-060 | satisfied 0.77 | contradicted 1.0 | `fixtures/plan-20261018T232141Z-fixture_ANC-060_{pass,fail}_1.json` |
| ANC-069 | satisfied (addressed 0.95, contradicted 0.85) | contradicted 1.0 | `fixtures/plan-20261018T232522Z-fixture_ANC-069_{pass,fail}_1.json` |
| ANC-073 | satisfied (addressed 0.98, contradicted 0.83) | contradicted 0.94 | `fixtures/plan-20261018T232850Z-fixture_ANC-073_{pass,fail}_1.json` |

ANC-053 and ANC-057: the one-sentence approved pass fixtures stay underspecified at the 0.6 production threshold on both the default and the sealed sets, stable across two runs and leaning satisfied; the sealed sets are kept (semantically exact on the frozen oracles, fail fixtures contradicted at 0.92 to 1.0). The fixtures are authority and were not rewritten; the threshold was not lowered. Rand item: reword the two pass fixtures or accept them as leaning-satisfied.

### Gate re-runs

Runs were made with `jev_judge.py judge --all` on the item texts composed by `compose-item --for plan` from `beads/plan.jsonl` (no `anchor-plan-review` descriptor, no `plan_source`), the same settings as round 1. Items whose text changed in the cleanup below (bj-1, bj-3, bj-4; bj-2 for a complete latest run) were re-run in full.

| Item | Previous run | New run | `diff` |
| --- | --- | --- | --- |
| atm-bj-1 | `plan-20261018T224823Z-atm-bj-1.json` | `plan-20261018T233126Z-atm-bj-1.json` (pass) | ANC-028 not_applicable to excluded; ANC-064 removed (retired); no regressions; -82 chars |
| atm-bj-2 | `plan-20261018T224719Z-atm-bj-2.json` | `plan-20261018T233234Z-atm-bj-2.json` (pass) | ANC-056 and ANC-060 satisfied to excluded; ANC-064 removed; no regressions; text unchanged |
| atm-bj-3 | `plan-20261018T224821Z-atm-bj-3.json` | `plan-20261018T233020Z-atm-bj-3.json` (pass) | no verdict change; ANC-064 removed; +55 chars (ANC-069 lean sentence) |
| atm-bj-4 | `plan-20261018T225203Z-atm-bj-4.json` | `plan-20261018T233126Z-atm-bj-4.json` (pass) | ANC-052 satisfied to excluded; ANC-064 removed; no regressions; -159 chars |

Intermediate runs and their triage (every new failure was near-threshold, leaning the passing verdict):

- bj-2 ANC-060 (`plan-20261018T232158Z-atm-bj-2.json`, sealed set): relevant no 0.27, leaning not_applicable, was satisfied 0.70 on the default set. Excluded with reason (bj-2 touches no dispatch or assignment, the ground of its ANC-061 exclusion); `plan-20261018T232228Z-atm-bj-2.json` passes.
- bj-3 ANC-069 (`plan-20261018T232545Z-atm-bj-3.json`, sealed set): addressed yes 0.59, re-judge 0.69, leaning satisfied. Plan defect, `replace`: the lean clause of deliverable 1 did not say the ancestry check belongs to the lean path; it now reads "`lean`: the ancestor check above is the only divergence check; verify the PR base ref equals the top branch from one filtered REST pull list ...". Growth +55 chars, content required by locked ANC-069 v2 (predates the run). `plan-20261018T233020Z-atm-bj-3.json`: ANC-069 satisfied, addressed 1.0.
- bj-1 ANC-028 (`plan-20261018T233022Z-atm-bj-1.json`): relevance no 0.56, re-judge 0.79, leaning not_applicable; was not_applicable at 0.62. Applicability noise on a phase-bi icon anchor; excluded with reason.
- bj-4 ANC-052 (`plan-20261018T233022Z-atm-bj-4.json`, sealed set): addressed yes 0.62, re-judge 0.43, leaning satisfied; was satisfied 0.85 on the default set. The sealed `addressed` question requires both assignment paths (close hook and atm-bd) in one item; bj-4 plans only the atm-bd read order (ANC-061). Excluded with reason; ANC-052 is satisfied by bj-1 (0.86). This is the ANC-052 two-path reading the tuner flagged (above), a Rand ruling, not a plan change.
- bj-2 ANC-056 (`plan-20261018T233151Z-atm-bj-2.json`, text unchanged): addressed yes 0.70, re-judge 0.55, leaning satisfied; was satisfied 0.73. The sanity FAIL hook is bj-1; excluded with reason.

No item was edited to pass a verdict; no bead failed three times.

### Coverage (sprint items, latest run each)

`coverage --runs <the four latest sprint runs> --gate plan`: 4 items, 67 anchors, 0 contradicted, 34 excluded cells. Every locked anchor ANC-051..074 is satisfied by at least one sprint item; ANC-062 and ANC-064 (retired) are absent from the table. The 35 uncovered anchors are the carried phase-bi anchors no bj sprint touches (not_applicable, deterministic_only or excluded on every item), as in round 1. `coverage` over the whole `anchor-runs` directory also picks up the one exploratory root-epic run (`plan-20261018T223945Z-atm-phase-bj.json`), which still carries an ANC-064 cell; the root epic is not gated.

### Cleanup requested by team-lead (Rand: bj = atm-bd-orchestration only, bk = atm-core only, "make sure bj and bk anchors are not mixed up in bj")

- ANC-064 retired (`retire --reason "atm-core scope; carried into phase bk anchors (docs/plans/phase-bk)"`, no `--replaced-by`: the bk anchor id does not exist yet), its five item exclusions removed, `anchors.md` re-rendered.
- Bead vars: bj-1 "Does Not Close" bullet on the atm-core hook deleted; bj-4 deliverable 7 no longer says the quality-mgr prompt is a separate atm-core PR (bj-4 renders `metadata.reviewers` when present and `-` when absent, so it consumes no atm-core artefact) and its "Does Not Close" bullet on that prompt deleted; root epic description sentence on the atm-core hook deleted and its design bullet shortened to "The hook is a script the agent runs as the last step of its template." `plan.jsonl` re-rendered strictly from the vars (`sc_render.py` with the atm-beads templates; the render reproduced the previous file byte for byte before the edits); `validate-plan --file ... --phase bj --index docs/plans/phase-bj.jsonl` exit 0. bj-2 is untouched: its mentions of atm-core are the consumer repo of the skill install (`validate-plan --ci` after install), not atm-core work.
- Left in place, for team-lead: `intent.md` cites atm-core #1873 comment 1 as the provenance of Rand's rulings (line 3) and names #1873 as the long-term home of the hook (line 23); the `context` of ANC-051..063 cites the same provenance. `intent.md` is the tuning authority (`intent_sha256` in every sealed history), so an edit needs an intent version bump and would invalidate the seven seals; the anchor contexts are locked provenance. Neither is a bj scope statement.

### Tuner harness observations (for the skill, not fixed here)

- The ANC-057 tuner put the expected class in oracle item titles ("... (approved pass fixture)"), visible to Jev; ANC-053's tuner used neutral titles. The ANC-073 brief added "do not put the expected class in any item title or id". A title rule belongs in the tune-request template.
- `jev_judge.py` run ids are second-resolution and item-keyed, so a tuner's concurrent repeats of one case share a run id and the second repeat's run file overwrites the first (ANC-053); the tuners kept their own per-repeat files. A sequence or nonce in the run id would remove the collision.
