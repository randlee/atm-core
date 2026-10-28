# Current close-hook transitions, mapped against the docs/diagrams spec

Read-only extraction at atm-bd-orchestration `origin/main`. It describes what the code does today. The transitions spec is the set of Mermaid diagrams in `docs/diagrams/`: close-hooks.md is the primary one, with lifecycle.md, sprint.md, correlation.md and bd-cache.md alongside. Every edge below is mapped to the code that implements it; then come the diagram-only and code-only decisions.

## Source abbreviations

Code (under `plugins/atm-bd-orchestration/skills/atm-bd-orchestration/scripts/` unless noted):

| abbr | file | lines |
|---|---|---|
| ATC | atm-task-close.py | 963 |
| STH | sanity_task_close_hook.py (daemon `task.close` hook) | 471 |
| AG | assignment-gates.py | 267 |
| ST | stack_top.py | 29 |
| TR | transitions.py | 25 |
| CV | close-vars.py | 205 |
| SCV | sanity-close-vars | 101 |
| BF | close_fields_backfill.py | 158 |
| CF | `skills/atm-beads/scripts/close_fields.py` | 124 |

Diagrams: `CH:n` = docs/diagrams/close-hooks.md line n; `LC:n` = lifecycle.md; `SP:n` = sprint.md; `CO:n` = correlation.md; `BC:n` = bd-cache.md.

Governing: `bj:ANC-nnn` = atm-core `develop:docs/plans/phase-bj/anchors.json`; `bk:ANC-nnn`, `REQ-BK-*` = atm-core `develop:docs/plans/phase-bk/anchors.json`. The REQ-BK ids are defined only there; no separate requirements file was found. `TCH.n` = atm-bd `docs/requirements/task-close-hook.md`, numbered item n. `SCH:n` = `scripts/SANITY-CLOSE-HOOK.md` line n.

---

## 1. Diagram nodes and edges mapped to code

### 1a. Dev or fix close (CH:17-46; also LC:24-27, SP:13, CO:44)

| node/edge | guard | outcome | error | implementing code | governing |
|---|---|---|---|---|---|
| VARS (CH:26) | — | lead's close vars read | VARS.INVALID if unreadable / not JSON / not object | ATC:920, 122-136 | bj:ANC-058, TCH.2 |
| RES (CH:27) | dev/fix only | result read | ARGS.INVALID if `--result` absent | ATC:921-925 | TCH.2 |
| MERGE yes → BADV (CH:41) | result field not in `result_variables_by_role`; or a field in both files | refused | VARS.UNKNOWN / VARS.INVALID | ATC:139-149 | TCH.2 |
| MERGE no → DERIVE (CH:42) | merge clean | derive `commit`, `generated_at` (git HEAD); `rebased_onto` = stack top head; `pr_number`, `pr_url`, `pr_base` from the single open PR at HEAD; PR and stack checks | see section 3 (no refusal edge is drawn) | ATC:742-748, 591-662, 576-589, ST:9-29 | bj:ANC-054, bj:ANC-059, TCH.7 |
| DERIVE → WRITE (CH:42) | checks pass | journal `update --set-metadata {commit, branch, pr_number, pr_url, pr_base?}` (only changed keys), then `close` checked bead "dev complete at <sha>" / "fixed at <sha>"; each drained at once (≤5 busy waits) | — | ATC:826-829, 541-546, 502-511, 732-740 | TCH.8, BC:23 |
| APPLIED pending → PEND (CH:43) | journal entry not applied after drain | refused, rerun applies once | BEADS.WRITE_PENDING | ATC:521-524 | BC:67-74 (UNGOVERNED by ANC/REQ) |
| APPLIED escalated → ESCW (CH:44) | entry escalated | refused | BEADS.WRITE_ESCALATED | ATC:517-520 | BC:74-75 (UNGOVERNED by ANC/REQ) |
| APPLIED yes → REPORT (CH:45) | **only if** `assign and not open_assignment`, and lead ≠ caller | `atm send <lead> --template dev-complete.md.j2 / fix-complete.md.j2` | — | ATC:830-831, 677-684 | TCH.8 |
| REPORT → ASSIGN (CH:45) | **only if** `assign` (ATC:821) | `atm task assign <dev_sanity_member> --task-id <sanity> --template dev-sanity-template.xml.j2` as lead, with a temp dispatch file (close_context, beads_dir, sanity_close_mode) | — | ATC:832, 706-719, 686-704 | bj:ANC-051, bj:ANC-052, TCH.8 |
| ASSIGN → DONE (CH:45) | dev task not terminal | `atm task close <dev task> completed <reason>` as caller | — | ATC:835, 722-730 | TCH.21. **Conflicts with bj:ANC-055** (dev task stays open until sanity PASS) |

### 1b. Sanity close hook (CH:51-97; LC:28-51; SP:14, 130-159; CO:42, 48)

| node/edge | guard | outcome | error | implementing code | governing |
|---|---|---|---|---|---|
| CLOSE → ROUTE (CH:74) | ATM calls the hook with the envelope | — | — | STH:395, 459-462 | bk:ANC-001, bk:ANC-003 |
| ROUTE no → DEFAULT (CH:75) | prefix mismatch | **not-applicable** (not pass-through) | — | STH:400-402 | bk:ANC-001 |
| ROUTE no → DEFAULT | caller not in `sanity_agents` | pass-through | — | STH:418-420 | bk:ANC-005, bk:ANC-022, SCH:33-35 |
| ROUTE no → DEFAULT | bead lacks `stage:dev-sanity` | pass-through | — | STH:421-424 | SCH:33-35 |
| ROUTE yes → READ (CH:76) | — | cache read of sanity bead (pre-lock), then batched `bd list --id` refresh of sanity + dev; on failure use cached rows, log `read_fallback` | — | STH:422, 283, 157-168 | bk:ANC-010, SCH:46-52 |
| VALID: dev_bead and blocks edge (CH:64) | `metadata.dev_bead` missing; no `blocks` dependency on dev | refused | SANITY_CLOSE.INVALID (phase-lead) | STH:279-280, 284-285 | SCH:54-57 |
| VALID: verdict PASS or FAIL | CANNOT_RUN / other | refused | SANITY_CLOSE.LEGACY_VERDICT / .INVALID | STH:296-302 | SCH:74-80 |
| VALID: findings shape | not `{deliverable:int≥0, issue:str}` | refused | SANITY_CLOSE.INVALID | STH:305-310 | SCH:59-61 |
| VALID: caller is sanity assignee | last sanity assignment's assignee ≠ caller | refused | SANITY_CLOSE.INVALID | STH:316-317 | SCH:54-57 |
| VALID: dev assignee == dev task assignee | mismatch / missing | refused | SANITY_CLOSE.INVALID (phase-lead) | STH:318-323 | SCH:54-57 |
| VALID no → ESC (CH:77) | any refusal, or hook exception | `{"result":"refused"}`; journal entry per error_id; `atm task assign <lead> --task-id <task>.close-error-<id> --template hook-close-escalation.xml.j2` once per cause | codes in section 3 | STH:210-258, 430-432, 463-470 | bk:ANC-004, REQ-BK-ERRORS, SCH:85-100 |
| VERDICT PASS → PW (CH:79) | PASS and sanity bead not already closed | journal `close <sanity> "PASS at <commit>"` | — | STH:332-334, 365-374 | bj:ANC-053 (partly), SCH:63-64 |
| VERDICT FAIL → FW (CH:80) | FAIL | journal dev → open (precondition assignee == dev_agent) if not open; sanity → open, assignee "" (precondition assignee unchanged) if not already | — | STH:335-339 | bj:ANC-056, bj:ANC-072, SP:150-153 |
| PW/FW → DRAIN (CH:79-80) | **only if** writes non-empty | spawn detached `bd_cache.py drain`; on OSError log and rely on cron | — | STH:376-377, 382-392 | BC:23, SCH:68-70 |
| DRAIN PASS → PA (CH:81) | dev task's last event not `complete` | action `task.close <dev>` as dev_agent with sanity-dev-close.md.j2; then original sanity close | — | STH:349-351, 355-357 | bk:ANC-006/007/009, bj:ANC-055 |
| PA → STACK (CH:81) | "fixed PASS" | **not in the hook.** dev-sanity writes stack input only when PASS, not not_reproducible, and stack_bead and pr_number are set | — | SCV:65-78 (sanity-stack itself not read) | bj:ANC-065, bj:ANC-066 |
| DRAIN FAIL → FA (CH:82) | FAIL | action `send` sanity-failed-template.md.j2 to `<dev_agent>@<team>` as caller; original sanity close | — | STH:352-357 | bj:ANC-056, bj:ANC-072 |
| CH:93-97 "hook neither dispatches QA" | — | no QA assign in STH | — | STH:346-358 (absence) | Conflicts with **bj:ANC-053** |
| LC:49-50 invalid result → escalation | as ESC | as ESC | — | as ESC | — |

### 1c. Close vars (CH:109-146)

| node/edge | guard | outcome | error | implementing code | governing |
|---|---|---|---|---|---|
| POUR → BEAD (CH:132) | pour | stamp `close_fields.POURED` | — | CF:40-48 (callers in formulas not read) | bj:ANC-063, ANC-078..081 |
| OLD → BF → BEAD (CH:133) | bead lacks fields | fill only missing/empty; report differing; `--apply` journals one update per bead | — | BF:59-117, 143-152 | UNGOVERNED |
| BEAD → CV (CH:134) | — | read one bead from bd-cache | — | CV:112-130, 45-70 | UNGOVERNED |
| CV → NP (CH:135) | poured field missing after one fresh re-read | refused | CLOSE_VARS.NOT_POURED | CF:64-68, CV:147-156 | UNGOVERNED |
| CV → VJ → NEXT (CH:136) | ok | write `--out`; print next commands | — | CV:197-200, 172-177 | bj:ANC-089 |
| CH:143 "sanity close adds round from started events and reviewers_round1; QA adds round" | — | as stated | CLOSE_VARS.NOT_STARTED if 0 started | CV:121-129, 91-109 | UNGOVERNED |
| BEAD → QV (CH:137) | `--qa-assignment` | QA vars from the QA bead + the checked bead's recorded commit/branch/pr_number/pr_base | — | CV:133-144, CF:90-124 | bj:ANC-053 (manual substitute) |
| QV refused → QR (CH:138) | recorded field missing / QA ≠ checked value / layer, round, reviewers_round1 missing | refused | ASSIGN_VARS.NOT_RECORDED / .CONFLICT / .NOT_PLANNED | CF:102-115 | UNGOVERNED |
| QV → QA (CH:139) | ok | **prints** `atm task assign <qa_member> ... --template <templates.qa>`; does not run it | CLOSE_VARS.CONFIG if template/member missing | CV:159-169, 190-200 | bj:ANC-051 |

### 1d. QA close (no flowchart; LC:59-61, CH:10, SP:15)

| node/edge | guard | outcome | error | implementing code | governing |
|---|---|---|---|---|---|
| LC:59 Q→C `--role qa --vars` | — | shared prelude validation | see App. A prelude | ATC:904-930 | TCH.18 |
| LC:60 journal close qa "verdict at sha" | QA bead not closed | journal close | — | ATC:896, 732-740 | bj:ANC-070, TCH.18 |
| LC:61 QA verdict message, close qa task | QA task not terminal (message); lead ≠ caller | `atm send <lead> "QA <verdict> <qa> commit=<sha> round=<n>"`; `atm task close <qa> completed --template qa-complete.md.j2` | — | ATC:897-901 | bj:ANC-070 |

### 1e. Legacy sanity close (prose only: CH:99-107, LC:75-79, SP:161-164; no flowchart)

| statement | implementing code | match |
|---|---|---|
| mode chosen by the hook registration (`sanity_close_mode`) | ATC:68-80, 693-694 | yes |
| PASS closes the sanity bead and task and assigns QA itself | ATC:851-862 (QA assign only when the QA task has no events, or is terminal with an open bead: ATC:709) | yes, with a guard |
| FAIL increments `metadata.sanity_round` | ATC:866-871: increments only when `sanity_round_key` differs | differs on replay |
| ≤ max: reopen checked, reset sanity open/unassigned, send sanity-fail | ATC:881-889 | yes |
| > max: SANITY.ROUND_CAP to the lead, reopen nothing ("once", SP:164) | ATC:874-880. It goes to `team_lead_member` from the repo config, not the phase `lead`; it is sent only while the sanity task is not terminal | recipient differs from the "lead" wording |

### 1f. Roles and paths the code handles with no diagram

- Fix `not_reproducible` (ATC:791-800, 828, 750-765)
- `--skip-checks` (ATC:803-805), intentionally undocumented per bj:ANC-059
- Offline GitHub fallback (ATC:621-627, 640-642, 661-662)
- Legacy sanity CANNOT_RUN (ATC:843-849)
- STH routing of `.close-error-` tasks (STH:403-404) and `.bd-repair-` tasks (STH:405-416)
- STH dry_run (STH:427-428)
- STH doctor (STH:435-455)
- Assignment pre-claim gates (AG:137-205). These are not close paths.
- TR `next_transition` (TR:7-25). No production caller; a QA round cap at 2 that appears in no diagram.

---

## 2. In diagram, not in code, or implemented differently (12 rows)

| # | diagram | code | difference |
|---|---|---|---|
| 2.1 | CH:45 REPORT always follows a successful write | ATC:830, 682 | Sent only when `assign and not open_assignment` and lead ≠ caller. A sha-changed re-close (open sanity assignment) re-assigns with no report. |
| 2.2 | CH:45 ASSIGN always | ATC:821, 709-710 | Skipped when the sanity assignment is open at the same sha, or sanity is terminal and closed. |
| 2.3 | CH:42 order DERIVE → WRITE → REPORT → ASSIGN | ATC:825 | The sanity dispatch is built before WRITE: `atm hook list` as lead, strict render, size check. A failure refuses before any write. |
| 2.4 | CH:30 DERIVE always runs "PR and stack checks" | ATC:791-805 | Skipped for not_reproducible (branch/commit = stack top) and `--skip-checks` (rebased_onto = `origin/<pr_target>`, ignoring the ledger). |
| 2.5 | CH:75 ROUTE no → "pass-through" | STH:396-402 | Wrong hook name or prefix returns `not-applicable`. Effectively the same under bk:ANC-001, which says default logic runs when every hook replies not-applicable. |
| 2.6 | CH:65 ESC "no bead write" | STH:361-374, 216-218 | A failure inside `execute()` after some writes were journaled also goes to ESC, with `progress.journaled`. Those writes stay and drain. |
| 2.7 | CH:79-80 every verdict path → DRAIN | STH:376 | Drain is spawned only when the plan has writes; a replayed close with nothing to write spawns none. |
| 2.8 | CH:81 PA → STACK as a hook transition | SCV:65-78 | Not in STH; dev-sanity does it after the close. |
| 2.9 | CH:139 QV → QA "atm task assign" | CV:159-169 | close-vars only prints the command; the lead runs it. |
| 2.10 | CH:104 / SP:162 "FAIL increments sanity_round" | ATC:869 | No increment when the `[task, assignment_id, verdict, commit]` key repeats (replay). |
| 2.11 | CH:106 / SP:164 ROUND_CAP "to the lead" | ATC:878 | Goes to config `team_lead_member`, not the phase-toml `lead` that receives everything else. |
| 2.12 | LC:33, CH:70 PA closes the dev task "if still open" | ATC:835 | The dev hook already closed it at dev complete, so in practice PA is a no-op unless the dev task was re-opened. Two writers for one transition. |

Not in the code read here (out of scope, not counted): LC:34-43 sanity-stack outcomes (stacked, rebase_required, lock_timeout, stack_failed), and the BC:41-58 drain internals, which live in bd_cache.py and are only called from here.

---

## 3. In code, not in diagram: decisions that change an outcome (main finding, 74 rows)

### Dev/fix close (ATC): 31

| # | file:line | decision |
|---|---|---|
| 3.1 | ATC:916-919 | bad `ATM_BD_GATE_TIMEOUT_S` → CONFIG.INVALID |
| 3.2 | ATC:152-161 | outcome must be fixed (dev) / fixed or not_reproducible (fix); `completed` aliased → VARS.INVALID |
| 3.3 | ATC:299-300 | dev + not_reproducible → VARS.INVALID |
| 3.4 | ATC:305-314 | required vars missing (role exemptions for not_reproducible) → VARS.MISSING |
| 3.5 | ATC:312, 315-316 | undeclared dispatch var → VARS.UNKNOWN (BADV covers only merge) |
| 3.6 | ATC:317-329 | named fields empty → VARS.EMPTY |
| 3.7 | ATC:325-326, 750-765 | not_reproducible evidence must be a real file:line in the worktree → VARS.INVALID / EVIDENCE.INVALID |
| 3.8 | ATC:330-333, 340-347, 350-351 | type checks; undone shape; task id ≠ bead id → VARS.INVALID |
| 3.9 | ATC:164-182 | strict header-only render → VARS.INVALID (bj:ANC-058 governs; no node) |
| 3.10 | ATC:360-366, 203-224 | ATM_IDENTITY unset; repo/phase/team config invalid → VARS.INVALID / CONFIG.INVALID |
| 3.11 | ATC:252-259, 483 | dev task has no assignment event → REPLAY.CONFLICT |
| 3.12 | ATC:442-446 | bead lookup not unique → BEAD.INVALID |
| 3.13 | ATC:747, 767-773 | derived commit empty → VARS.EMPTY |
| 3.14 | ATC:781-788 | group sanity bead missing or ≠ vars → BEAD.NO_SANITY |
| 3.15 | ATC:594-596 | dirty worktree → HEAD.DIRTY (beyond bj:ANC-054's "only") |
| 3.16 | ATC:607-609 | origin head ≠ HEAD → HEAD.NOT_PUSHED |
| 3.17 | ATC:600-627 | ls-remote/fetch fail 3x → offline mode (github=unavailable); tracking ref ≠ HEAD → HEAD.NOT_PUSHED |
| 3.18 | ATC:576-589, ST:10-28 | stack-top selection (ledger last layer, else pr_target, else base) and its refusals → VARS.EMPTY / STACK.TOP / COMMAND.INVALID |
| 3.19 | ATC:630-634 | top not ancestor → BASE.NOT_ANCESTOR; other git failure → COMMAND.FAILED |
| 3.20 | ATC:635-638 | origin URL unparsable → COMMAND.INVALID |
| 3.21 | ATC:643-662 | gh api fails 3x → offline, PR identity from bead metadata |
| 3.22 | ATC:651-653 | no open PR at HEAD → PR.NOT_FOUND |
| 3.23 | ATC:654-656 | >1 PR at HEAD → PR.AMBIGUOUS |
| 3.24 | ATC:806, 814 | pr_number/pr_url empty after checks/offline/skip → VARS.EMPTY |
| 3.25 | ATC:796, 807, 671-675 | completion template strict render fails → VARS.INVALID |
| 3.26 | ATC:821-823 | sanity closed at a different sha → REPLAY.CONFLICT |
| 3.27 | ATC:68-80, 693-694 | `sanity_close_mode` daemon/legacy chosen from `atm hook list`; a read failure → COMMAND.FAILED (SCH:39-42 prose only) |
| 3.28 | ATC:696-703 | sanity dispatch > 1,114,112 bytes → ASSIGNMENT.TOO_LARGE |
| 3.29 | ATC:454-462 | BEADS_DIR unresolvable → BEADS_DIR.INVALID |
| 3.30 | ATC:803-805 | `--skip-checks` path |
| 3.31 | ATC:791-800, 828 | fix not_reproducible: sanity assigned at the stack top, reason "not reproducible: <file:line>" |

### Sanity close hook (STH): 21

| # | file:line | decision |
|---|---|---|
| 3.32 | STH:396-397 | hook ≠ task.close → not-applicable |
| 3.33 | STH:403-404 | `.close-error-` escalation task → pass-through |
| 3.34 | STH:405-416 | `.bd-repair-` task: pass-through only if exactly one repaired record, else refused + escalation (removed by PR #607, see section 4) |
| 3.35 | STH:84-86 | per-task flock held → SANITY_CLOSE.BUSY |
| 3.36 | STH:262-267 | message_source not a Template / bad sections → INVALID |
| 3.37 | STH:277 | vars task_id ≠ closing task → INVALID |
| 3.38 | STH:281 | vars checked_bead ≠ dev_bead → INVALID |
| 3.39 | STH:286-289 | dev bead must be stage:dev/fix, or a stage:finding whose sanity_bead is this task → INVALID |
| 3.40 | STH:293 | reviewer ≠ sanity-selected → INVALID |
| 3.41 | STH:294 | run_id missing → INVALID |
| 3.42 | STH:303-304 | PASS while the dev bead is not closed → INVALID (no ATC equivalent) |
| 3.43 | STH:311-312 | PASS with findings / FAIL without findings → INVALID |
| 3.44 | STH:314-315 | sanity task has no assignment → INVALID |
| 3.45 | STH:181-188, 324 | phase lead unreadable → SANITY_CLOSE.CONFIG |
| 3.46 | STH:325-328 | journal has a prepared plan with a different (run_id, verdict, findings) signature → INVALID |
| 3.47 | STH:101, 112-131 | 1.8 s work budget exhausted / command fails → SANITY_CLOSE.COMMAND |
| 3.48 | STH:133-137 | invalid JSON → SANITY_CLOSE.DATA; STH:139-155, 370-371 bd-cache errors → BD_CACHE.* |
| 3.49 | STH:427-428, 228-231 | dry_run: actions returned, no writes, no escalation |
| 3.50 | STH:236-240 | journal unreadable → fallback escalation journal |
| 3.51 | STH:243-256 | escalation assign fails → retained `notified:false`, surfaced by doctor |
| 3.52 | STH:468-470 | escalation itself impossible → exit 1 (ATM_HOOK_EXIT per REQ-BK-ERRORS) |

### Legacy sanity close (ATC --role sanity): 14

| # | file:line | decision |
|---|---|---|
| 3.53 | ATC:334-337 | verdict ∈ PASS/FAIL/CANNOT_RUN; round must be a positive int → VARS.INVALID |
| 3.54 | ATC:348-349 | FAIL with empty undone → VARS.INVALID |
| 3.55 | ATC:843-849 | CANNOT_RUN → sanity bead open/unassigned, task closed `refused` (TCH.17; STH rejects it) |
| 3.56 | ATC:553-555 | last event refused + CANNOT_RUN → no-op replay |
| 3.57 | ATC:556 | last event refused + PASS/FAIL → REPLAY.CONFLICT |
| 3.58 | ATC:558-563 | older-assignment vars: same key → no-op; else REPLAY.CONFLICT |
| 3.59 | ATC:564-565 | same assignment, different verdict/sha → REPLAY.CONFLICT |
| 3.60 | ATC:566-569 | sanity bead closed with a different reason → REPLAY.CONFLICT |
| 3.61 | ATC:229-231, 244-248 | malformed sanity_round / round key → BEAD.INVALID |
| 3.62 | ATC:853-858 | fix PASS requires carry_forward == checked; a set carry_forward must equal it → VARS.INVALID |
| 3.63 | ATC:854, 863, 844 | strict renders of qa-template / sanity-fail / dev-sanity-complete → VARS.INVALID |
| 3.64 | ATC:872-873 | FAIL after increment, assignment no longer current → returns ok with no close and no send |
| 3.65 | ATC:876, 887 | ROUND_CAP notice and sanity-fail send skipped when the sanity task is already terminal |
| 3.66 | ATC:709 | QA assign skipped unless the QA task has no events, or is terminal with an open bead |

### QA close (ATC --role qa): 3

| # | file:line | decision |
|---|---|---|
| 3.67 | ATC:338-339 | QA CANNOT_RUN → VARS.INVALID |
| 3.68 | ATC:566-569 | QA bead closed with a different reason → REPLAY.CONFLICT |
| 3.69 | ATC:897-900 | lead status message skipped when the QA task is already terminal |

### Close vars (CV/CF): 5

| # | file:line | decision |
|---|---|---|
| 3.70 | CF:51-57 | labels name no single stage → CLOSE_VARS.NOT_A_TASK_BEAD |
| 3.71 | CF:69-71 | metadata.role ≠ stage → CLOSE_VARS.ROLE_MISMATCH |
| 3.72 | CV:45-88 | cache unopenable → `bd show` fallback; read failures → CLOSE_VARS.READ_FAILED |
| 3.73 | CV:91-109 | events unreadable / no started event → CLOSE_VARS.EVENTS_FAILED / NOT_STARTED |
| 3.74 | CF:97-99, CV:139-142 | not a QA bead → ASSIGN_VARS.NOT_QA; QA bead lacks checked_bead → CLOSE_VARS.NOT_POURED |

Cross-cutting decisions that also change outcomes but appear in no diagram, and are not counted above: Dolt-drop retry ×3 with 10 s sleep (ATC:381-407; AG:38-50); exception → error code mapping in `main` (KeyError/ValueError/TypeError → COMMAND.INVALID, OSError → COMMAND.FAILED; ATC:940-954); BEADS.WRITE_* also reachable from the sanity and QA roles of ATC, though the diagram draws them only for dev/fix.

---

## 4. Stale diagram content

- **CH:99-107 "Legacy sanity close"** describes `atm-task-close.py --role sanity` with `sanity-close-vars`. That is live code (ATC:837-889, chosen by ATC:68-80) and PR #607 does not touch it: #607's file list has no atm-task-close.py and no docs/diagrams change. The section does **not** describe `sanity_close_legacy.py`. PR #607 (OPEN, "fix: remove the legacy sanity close hook and bd-repair runner") deletes something different that shares the word "legacy": `sanity_close_legacy.py` (the frozen pre-bd-cache hook), `sanity-close-repair.py`, `sanity-bead-repair.xml.j2`, their tests, and the `.bd-repair-` routing in STH (STH:405-416, row 3.34). No diagram depicts those, so #607 makes no diagram stale. Rows 3.34, the doctor's `SANITY_CLOSE.REPAIR_PENDING` (STH:448-450) and the SCH:116-122 text become dead once it merges. Not treated as live spec here. If the coordinator's premise came from another document, please name it; nothing in docs/diagrams mentions sanity_close_legacy.py.
- **CH:65 / LC:50 "no bead write"** on ESC is inaccurate for execute-time failures (row 2.6).
- **CH:70 / LC:33** PA dev-task close: written as if the hook is the dev task's closer, but CH:37 / CH:127 / SP:13 say the dev close already closes it (row 2.12). The diagrams agree with TCH.21; **bj:ANC-055 disagrees with both**.
- **Anchor vs diagram conflicts.** Here the diagrams match the code but contradict the anchors. bj:ANC-053: the hook should assign QA; CH:93-96 says it does not. bj:ANC-057: count rounds and cap them; SP:14 says no round is counted in daemon mode. bj:ANC-056: send on "the dev's still-open task"; SP:156 says the dev task was closed. Which side is stale is a ruling, not an extraction result.

---

## 5. Duplicated decisions

| # | decision | copies (file:line) | note |
|---|---|---|---|
| 1 | **sanity round** | ATC:227-232 + 866-871 (FAIL counter in metadata.sanity_round, keyed); ATC:257 (assignment round = 1 + completed events); CV:91-109, 124 (round = count of `started` events); SCV:44 (`close_context.round`, default 1) | Four different definitions. ATC:560 compares vars round (from CV/SCV) to the assignment round (ATC:257). |
| 2 | **assignment event / current assignee** | ATC:36, 252-259, 572-574; STH:41, 314-317, 319-321 | Same event set; ATC also counts rounds, STH also checks the assignee. |
| 3 | **sanity verdict set** | ATC:334 (PASS/FAIL/CANNOT_RUN); SCV:17 (same); STH:296-302 (PASS/FAIL, CANNOT_RUN rejected) | Contradictory across paths. |
| 4 | **findings → undone** | SCV:26-31 (convert, with fallback "all deliverables" on lint FAIL); ATC:340-349 (validate undone, FAIL needs undone); STH:305-312 (validate findings, PASS⇔empty) | |
| 5 | **FAIL reopen writes** | ATC:881-885; STH:335-339 | Same bead effects; STH adds preconditions; ATC adds the round cap. |
| 6 | **PASS close reason "PASS at <sha>"** | written ATC:859, STH:334; parsed AG:79-89 (`has_prior_pass`), AG:179-187 (`sanity_pass_commit`), ATC:567 (replay compare), TR:12, 18 (`startswith("PASS")`) | String protocol parsed in 4 places. |
| 7 | **stack top** | ST:9-29 (shared); ATC:576-584 (own no-stack_bead fallback via pr_target/base); ATC:805 (skip path: origin/pr_target directly, ignores ledger); AG:126-135 (stack-top CLI); SCV:67 (stack input needs stack_bead) | Three top computations that can disagree under `--skip-checks`. |
| 8 | **is-fix** | ATC:853 (metadata.role==fix or label stage:fix); CF:116 (same); SCV:48 (label stage:fix only) | |
| 9 | **not_reproducible field exemptions** | ATC:169-172, 308-311, 318-326; SCV:53-61, 67 | Same rule expressed 5 times. |
| 10 | **required-vars contract check** | ATC:164-182 (strict render); ATC:298-316 (set difference); SCV:50-56; CF:77-80 | Two checks of one contract inside ATC alone. |
| 11 | **Dolt-drop retry** | ATC:47-49, 381-407; AG:33-35, 38-50 (self-described "local mirror") | |
| 12 | **phase lead read** | ATC:185-200, 365; STH:181-188 | |
| 13 | **sanity bead ↔ dev bead link** | ATC:781-786 (checked.metadata.sanity_bead, else list by dev_bead); STH:279-289 (sanity.metadata.dev_bead + blocks edge); AG:181-183 (list by dev_bead) | |
| 14 | **dev task close** | ATC:835 (dev hook closes own task); STH:349-351 (sanity PASS closes dev task if not complete) | Normally redundant. bj:ANC-055 and TCH.21 disagree on which one is correct. |
| 15 | **sanity-fail message** | ATC:888 (`sanity-fail.xml.j2`); STH:353 (`sanity-failed-template.md.j2`) | Two templates for one event. |
| 16 | **terminal-task test** | ATC:83-84 (used at 709, 724, 817, 876, 887, 897); STH:329 | |

---

## 6. Non-transition code (approximate line counts)

ATC (963 lines):

| block | lines | range |
|---|---|---|
| header, imports, constants, small helpers (load_sample, bd_value, CloseError, Parser, header) | ~100 | 1-66, 87-121 |
| input validation (read/merge/outcome/strict render/validate) | ~115 | 122-182, 298-351 |
| config reads | ~40 | 185-224 |
| command JSON shape validation | ~34 | 262-295 |
| Hook init | ~26 | 354-379 |
| subprocess run, Dolt retry, attempt log, json | ~60 | 381-440 |
| bead read / cache / journal write / drain / update / metadata diff | ~105 | 442-546 |
| render / send / notify / dispatch build and size check | ~34 | 671-704 |
| derive_local / evidence / identity checks | ~32 | 742-773 |
| checks(): git/gh calls, two 3x retry loops, offline fallback (≈15 lines are guards D-R3..D-R13) | ~79 | 591-669 |
| main(): argparse, exception→error mapping, double print | ~60 | 904-963 |
| **transition-bearing functions** (role flows 775-901, check_replay 548-574, assign/close due checks 706-740, stack_top 576-589, round/assignment helpers 227-259, mode/terminal 68-84) | ~250 | — |

Of the ~250 transition-bearing lines, about 70 are guard predicates that choose an outcome (the `if`/`assign =`/`due =` lines and the replay comparisons). The rest are the effect calls. Roughly **70-250 of 963 lines (7-26%) are transition logic**; ~700 are I/O, validation, retries and error mapping.

Defensive branches for states that likely cannot occur: P9 (ATC:302-304), P14 (330-333), D-R8 STACK.TOP (581-583, preceded by a raising `run`), the `assert`s (399, 403), ATC:657 `or ""` on a required PR number, SL4 silent return (872-873, reachable only on a race with re-assignment), the stack-bead read on sanity PASS that is never used (ATC:477-478).

STH (471 lines):

| block | lines | range |
|---|---|---|
| header, imports, Failure, require, digest | ~57 | 1-57 |
| Journal (SQLite) + task_lock | ~32 | 59-90 |
| Runtime: run/budget/read_json/cache/bead/refresh/events/state/lead | ~96 | 93-188 |
| action() request builder | ~19 | 190-208 |
| validation_error escalation | ~49 | 210-258 |
| selected_values | ~10 | 261-270 |
| execute + drain_now | ~32 | 361-392 |
| doctor | ~21 | 435-455 |
| __main__ | ~14 | 458-471 |
| **transition-bearing** (routing 395-432, prepare 273-343, close_actions 346-358) | ~122 | — |

Of those ~122 lines, ~35 are `require` validations, ~15 choose writes/actions (STH:331-339, 349-357) and ~20 are routing guards (STH:396-424). That is **≈35 decision lines vs ≈430 other**.

Across both close paths, about **105 of ~1430 lines (~7%)** are decisions that choose an outcome. Counting validation guards as decisions brings it to about 25%.

## 7. Unclear / not verified

- Where rebase-required (bj:ANC-065, ANC-071) is emitted: not in ATC or STH.
- Whether ATM treats `atm task assign` of an existing id as an in-place update (D3 depends on it): TCH.19 says it does, but this was not verified in atm-core code.
- STH's escalation assign runs as the caller identity (STH:105, 251), not as the lead. Whether ATM accepts that for a lead-owned task was not checked.

---

## Appendix A. Code-derived transition tables (per role, as first extracted)

## Common prelude (ATC, all roles)

Input facts read before any role logic:

| fact | subsystem | where |
|---|---|---|
| `--role`, `--vars`, `--result`, `--skip-checks` | argv | ATC:905-914 |
| `ATM_BD_GATE_TIMEOUT_S` | env / repo config | ATC:916-919 |
| dispatch vars (JSON object) | vars file | ATC:920, 122-136 |
| result vars (dev/fix only, required) | result file | ATC:921-926, 139-149 |
| contract `required_variables[_by_role]`, `result_variables_by_role`, `render_variables_by_role` | `templates/task-close-vars.j2` header | ATC:141, 167-168, 301-306 |
| `ATM_IDENTITY` | env | ATC:360-362 |
| repo config (`dev_sanity_member`, `qa_member`, `team_lead_member`, `max_sanity_rounds`) | `.claude/project/atm-bd-orchestration.yaml` via repo_config (default max_sanity_rounds 5, `atm-beads/scripts/repo_config.py:81`) | ATC:203-224, 363 |
| phase `lead` | `.atm-bd/phase-<x>.toml` | ATC:365 |
| team | `.atm.toml` `atm.default_team` | ATC:366 |

Prelude refusals. None writes anything:

| row | guard | error code | governing | source |
|---|---|---|---|---|
| P1 | bad argv / role | ARGS.INVALID | TCH.1 | ATC:113-115 |
| P2 | dev/fix without `--result` | ARGS.INVALID | TCH.2 | ATC:922-924 |
| P3 | bad timeout env | CONFIG.INVALID | TCH (timeouts §) | ATC:918-919 |
| P4 | vars/result unreadable, not JSON, not object | VARS.INVALID | bj:ANC-058, TCH.2 | ATC:122-136 |
| P5 | result has non-result fields | VARS.UNKNOWN | TCH.2 | ATC:142-144 |
| P6 | result overlaps dispatch | VARS.INVALID | TCH.2 | ATC:145-148 |
| P7 | dev outcome not `fixed`, fix outcome not in {fixed, not_reproducible} (`completed` is aliased to `fixed`) | VARS.INVALID | TCH.2 | ATC:152-161, 41-43 |
| P8 | dev with `not_reproducible` | VARS.INVALID | TCH.10 | ATC:299-300 |
| P9 | contract lacks by-role map | VARS.INVALID | UNGOVERNED (defensive) | ATC:302-304 |
| P10 | required field missing (exemptions for not_reproducible: fix drops branch/commit/pr_number/stack_bead; sanity drops stack_bead) | VARS.MISSING | bj:ANC-058, bj:ANC-089, TCH.2 | ATC:305-314 |
| P11 | undeclared field | VARS.UNKNOWN | TCH.2 | ATC:312, 315-316 |
| P12 | not_reproducible evidence not `\S+:\d+` | VARS.INVALID | TCH.10 | ATC:325-326 |
| P13 | named field empty (phase, sprint, paths, ids; stack_bead unless not_repro; pr_target when fix not_repro without stack_bead; branch for dev/fix; branch/commit/pr_number for sanity/qa) | VARS.EMPTY | TCH.2, TCH.13 | ATC:317-329 |
| P14 | path/phase/stack_bead not a string | VARS.INVALID | UNGOVERNED (defensive) | ATC:330-333 |
| P15 | sanity/qa verdict not in PASS/FAIL/CANNOT_RUN | VARS.INVALID | TCH.13, TCH.18 | ATC:334-335 |
| P16 | sanity round not a positive int | VARS.INVALID | UNGOVERNED | ATC:336-337 |
| P17 | QA verdict CANNOT_RUN | VARS.INVALID | TCH.18 (implicit) | ATC:338-339 |
| P18 | `undone` malformed | VARS.INVALID | bj:ANC-072 | ATC:340-347 |
| P19 | sanity FAIL with empty undone | VARS.INVALID | bj:ANC-072, TCH.15 | ATC:348-349 |
| P20 | task id differs from bead id (dev/sanity/qa) | VARS.INVALID | UNGOVERNED (CF:74-75 assumes it) | ATC:350-351 |
| P21 | strict render of header-only contract fails | VARS.INVALID | bj:ANC-058 | ATC:164-182 |
| P22 | `ATM_IDENTITY` unset | VARS.INVALID | TCH.4 | ATC:361-362 |
| P23 | repo config missing/invalid, or role's member field empty | CONFIG.INVALID | TCH.16 (max rounds), UNGOVERNED otherwise | ATC:203-224 |
| P24 | phase toml lead / atm.toml team missing | CONFIG.INVALID | TCH.4, bj:ANC-051 | ATC:185-200, 365-366 |
| P25 | fix not_reproducible evidence file:line not a real line inside the worktree | EVIDENCE.INVALID | TCH.10 | ATC:929-930, 750-765 |

Shared snapshot and replay step, used by every role (ATC:464-484, 548-570):

| fact | subsystem | where |
|---|---|---|
| own task bead, plus the role's related beads | bd `bd show --json` | ATC:442-446, 480 |
| task events for own task (and sanity/QA task) | atm `atm task events --all --json` | ATC:451-452, 481 |
| last assignment event id and round = 1 + completed events before it | atm | ATC:252-258, 483 |

| row | guard | outcome | error | governing | source |
|---|---|---|---|---|---|
| S1 | own task has no assigned/reassigned/reopened event | refused | REPLAY.CONFLICT | UNGOVERNED | ATC:259 |
| S2 | bd show returns ≠1 row | refused | BEAD.INVALID | UNGOVERNED (defensive) | ATC:444-445 |

---

### Role dev / fix (ATC `Hook.dev`, `finish_dev`)

Input facts specific to dev/fix:

| fact | subsystem | where |
|---|---|---|
| HEAD sha → `commit`, `generated_at` | git `rev-parse HEAD` (worktree) | ATC:744-746 |
| checked bead, sanity bead, stack bead (when included) | bd show | ATC:468-472, 778 |
| sanity task events | atm | ATC:472, 816 |
| `metadata.sanity_bead` of checked bead, or open `stage:dev-sanity` beads keyed by `metadata.dev_bead` | bd show / bd list | ATC:781-786 |
| worktree dirty | git status --porcelain | ATC:594 |
| origin branch head | git ls-remote (3 attempts), git fetch | ATC:598-616 |
| tracking ref (offline) | git rev-parse refs/remotes/origin/<branch> | ATC:621-627 |
| stack top (last ledger layer, else pr_target head) | bd stack bead + git rev-parse | ATC:576-589, ST:9-29 |
| ancestry of top in HEAD | git merge-base --is-ancestor | ATC:630-634 |
| owner/repo | git remote get-url | ATC:635-639 |
| open PRs for branch | gh api pulls (3 attempts) | ATC:643-660 |
| recorded pr_number/pr_url/pr_base (offline/skip) | bd checked bead metadata | ATC:664-669 |
| hook registrations | atm `atm hook list --phase --json` as lead | ATC:693 |
| beads dir | env BEADS_DIR or `bd where --json` | ATC:454-462 |

Pre-write refusals (dev/fix):

| row | guard | outcome | error | governing | source |
|---|---|---|---|---|---|
| D-R1 | derived commit empty (not for not_repro) | refused | VARS.EMPTY | TCH.5, TCH.7 | ATC:747, 767-773 |
| D-R2 | no group sanity bead, or it ≠ vars `sanity_bead` | refused | BEAD.NO_SANITY | TCH.9 | ATC:781-788 |
| D-R3 | worktree dirty | refused | HEAD.DIRTY | TCH.7. Not in bj:ANC-054, which says "only" PR, head equality and descent | ATC:594-596 |
| D-R4 | ls-remote head ≠ HEAD | refused | HEAD.NOT_PUSHED | bj:ANC-054, TCH.7 | ATC:607-609 |
| D-R5 | remote unreachable (3x) and cached tracking ref ≠ HEAD | refused, github=unavailable | HEAD.NOT_PUSHED | bj:ANC-059, TCH.6 | ATC:621-627 |
| D-R6 | stack-top ledger malformed / pr_target empty / no head | refused (ValueError mapped in main) | COMMAND.INVALID | TCH.7 | ST:10-28, ATC:951-954 |
| D-R7 | no stack_bead and no pr_target/base | refused | VARS.EMPTY | UNGOVERNED | ATC:577-580 |
| D-R8 | no stack_bead, origin/<branch> empty | refused | STACK.TOP | UNGOVERNED. Run raises COMMAND.FAILED first on a git failure, so this is nearly unreachable | ATC:581-583 |
| D-R9 | top not ancestor of HEAD | refused | BASE.NOT_ANCESTOR | bj:ANC-054, TCH.7 | ATC:630-632 |
| D-R10 | merge-base other failure | refused | COMMAND.FAILED | UNGOVERNED | ATC:633-634 |
| D-R11 | origin URL lacks owner/repo | refused | COMMAND.INVALID | UNGOVERNED | ATC:636-638 |
| D-R12 | GitHub reachable, no open PR at HEAD | refused | PR.NOT_FOUND | bj:ANC-054, TCH.7 | ATC:651-653 |
| D-R13 | GitHub reachable, >1 open PR at HEAD | refused | PR.AMBIGUOUS | TCH.7 | ATC:654-656 |
| D-R14 | after checks/skip/offline: commit, pr_number or pr_url empty | refused | VARS.EMPTY | bj:ANC-059, TCH.5, TCH.6 | ATC:806, 814, 767-773 |
| D-R15 | strict render of dev-complete/fix-complete fails | refused | VARS.INVALID | bj:ANC-058 | ATC:796, 807, 671-675 |
| D-R16 | sanity task terminal **and** sanity bead closed **and** checked metadata.commit ≠ assignment commit | refused | REPLAY.CONFLICT | UNGOVERNED | ATC:821-823 |
| D-R17 | hook list read fails | refused | COMMAND.FAILED | SCH:39-42 | ATC:693, 404-406 |
| D-R18 | sanity dispatch fails strict render | refused | VARS.INVALID | bj:ANC-052 | ATC:695 |
| D-R19 | sanity dispatch > 1,114,112 bytes | refused | ASSIGNMENT.TOO_LARGE | UNGOVERNED | ATC:696-703 |
| D-R20 | BEADS_DIR unset and `bd where` not absolute | refused | BEADS_DIR.INVALID | UNGOVERNED | ATC:460-461 |
| D-R21 | bead write journaled but not applied | refused (rerun applies once) | BEADS.WRITE_PENDING | UNGOVERNED (BD-CACHE.md) | ATC:521-524 |
| D-R22 | bead write escalated by bd-cache | refused | BEADS.WRITE_ESCALATED | UNGOVERNED (BD-CACHE.md) | ATC:517-520 |

Derived selector values (ATC:816-821):
`terminal` = the last sanity event has to_state complete. `open_assignment` = sanity events exist and the task is not terminal. `sha_changed` = checked `metadata.commit` ≠ assignment commit. `assign` = (not terminal **or** sanity bead open) **and** (sha_changed **or** not open_assignment).

Outcomes (dev/fix), all ok:

| row | guard (facts) | outcome | effects (in order) | error | governing | source |
|---|---|---|---|---|---|---|
| D1 | normal outcome, no sanity events (first close) | closed, sanity assigned | write checked metadata {commit, branch, pr_number, pr_url, pr_base?}; `bd close` checked "dev complete at <sha>" / "fixed at <sha>"; `atm send <lead> --template dev-complete/fix-complete` (skipped if lead == caller); `atm task assign <dev_sanity_member> --task-id <sanity> --template dev-sanity-template.xml.j2` as lead with dispatch incl. close_context, beads_dir, sanity_close_mode; `atm task close <dev task> completed <reason>` as caller | — | bj:ANC-052, bj:ANC-051, TCH.8, TCH.21. **Conflicts with bj:ANC-055** (dev task must stay open until sanity PASS) | ATC:825-835, 677-684, 706-719, 722-730 |
| D2 | sanity task terminal and sanity bead open (re-close after a FAIL) | same as D1 | same as D1, including the report send | — | bj:ANC-052, TCH.12, TCH.15 | ATC:817-832 |
| D3 | sanity assignment open (not terminal) and sha changed | closed, sanity re-assigned in place | metadata, bead close (no-op if closed), **no** report send, re-assign sanity with new dispatch, close dev task | — | TCH.19 (in-place update) | ATC:821, 830-832 |
| D4 | sanity assignment open, same sha (replay) | closed, no assign | metadata no-op; bead close no-op; no send; no `atm task assign` (but `assigned` lists sanity, ATC:720); close dev task if not terminal | — | UNGOVERNED (idempotent replay) | ATC:821, 709-720 |
| D5 | sanity terminal and bead closed (PASS already), same sha | closed, no assign | as D4 | — | UNGOVERNED | ATC:821 |
| D6 | `--skip-checks` | checks() skipped | PR identity from checked bead metadata; `rebased_onto` = `git rev-parse origin/<pr_target>`, **ignoring the stack ledger**; then D1-D5 | — | bj:ANC-059, TCH.5 | ATC:803-805 |
| D7 | GitHub unreachable (ls-remote/fetch 3x fail, or gh api 3x fail) but tracking ref == HEAD | proceeds offline | github=unavailable + github_error in result; PR identity from bead metadata; then D1-D5 | — | bj:ANC-059, TCH.6 | ATC:621-627, 640-642, 661-662, 935-936 |
| D8 | fix, outcome not_reproducible | closed, sanity assigned at stack top | checks skipped; branch/commit for the sanity assignment = stack top; close reason "not reproducible: <file:line>"; then D1-D5 selection, with sha_changed compared to the stack-top head | — | TCH.10 | ATC:791-800, 828 |

Not in this path: refused/cancelled dev closes (TCH.11).

---

### Role sanity: legacy script (ATC `Hook.sanity`)

Input facts: vars (`verdict`, `round`, `undone`, `commit`, `carry_forward`, `qa_bead`) from the vars file, built by SCV (SCV:12-62). bd: sanity bead, checked bead, plus `qa_bead` and stack bead on PASS (stack bead only when not not_repro) (ATC:473-479). atm: sanity task events, QA task events on PASS (ATC:479-481). Sanity bead metadata `sanity_round`, `sanity_round_key` (ATC:227-249). Checked bead `metadata.role` and labels (ATC:853). Config `max_sanity_rounds`, `qa_member`, `team_lead_member` (ATC:218, 874-878).

Replay guards (check_replay, ATC:548-570):

| row | guard | outcome | error | governing | source |
|---|---|---|---|---|---|
| SL-R1 | latest sanity event is `refused` and verdict CANNOT_RUN | no-op ok (replay) | — | UNGOVERNED | ATC:553-555 |
| SL-R2 | latest event `refused`, verdict PASS/FAIL | refused | REPLAY.CONFLICT | UNGOVERNED | ATC:556 |
| SL-R3 | sanity_round_key set, vars round < assignment round, key == [task, verdict, commit] | no-op ok (older replay) | — | UNGOVERNED | ATC:558-562 |
| SL-R4 | as SL-R3 but key differs | refused | REPLAY.CONFLICT | UNGOVERNED | ATC:563 |
| SL-R5 | key's assignment id == current and verdict/sha differ | refused | REPLAY.CONFLICT | UNGOVERNED | ATC:564-565 |
| SL-R6 | sanity bead closed with close_reason ≠ "<verdict> at <commit>" | refused | REPLAY.CONFLICT | UNGOVERNED | ATC:566-569 |
| SL-R7 | sanity_round not a non-negative int / bad round key | refused | BEAD.INVALID | UNGOVERNED | ATC:229-231, 244-248 |

Outcomes:

| row | guard | outcome | effects | error | governing | source |
|---|---|---|---|---|---|---|
| SL1 | CANNOT_RUN | task refused | render dev-sanity-complete; sanity bead → open, assignee "" (if not already); `atm task close <sanity> refused --template dev-sanity-complete.md.j2` | — | TCH.17 | ATC:843-849 |
| SL-R8 | PASS, checked bead is a fix (metadata.role==fix or label stage:fix) and carry_forward ≠ checked | refused | — | VARS.INVALID | TCH.14 | ATC:853-856 |
| SL-R9 | PASS, carry_forward set and ≠ checked | refused | — | VARS.INVALID | TCH.14 | ATC:857-858 |
| SL2 | PASS | closed, QA assigned | render dev-sanity-complete + qa-template; `bd close <sanity> "PASS at <sha>"`; `atm task close <sanity> completed --template dev-sanity-complete.md.j2`; `atm task assign <qa_member> --task-id <qa_bead> --template qa-template.xml.j2 --vars <vars>` as lead, **only if** the QA task has no events, or is terminal with an open QA bead (ATC:709). The dev task is **not** closed here; D1 already closed it | — | bj:ANC-053, bj:ANC-051, TCH.14. Diverges from bj:ANC-055 | ATC:851-862 |
| SL3 | FAIL, round key ≠ current key | count+1 | write `sanity_round`, `sanity_round_key=[task, assignment_id, verdict, commit]` on sanity bead | — | bj:ANC-057, TCH.15 | ATC:866-871 |
| SL4 | FAIL, after SL3 the assignment is no longer current (events re-read) | **ok with no close, no send**; returns silently after incrementing | — | UNGOVERNED | ATC:872-873 |
| SL5 | FAIL, count > max_sanity_rounds | cap | `atm send <team_lead_member> "SANITY.ROUND_CAP <bead> <sha> assignment=<id> undone=<n,...>"` (if sanity task not terminal; skipped if recipient == caller); close sanity task completed with dev-sanity-complete; result `escalated: "SANITY.ROUND_CAP"`; checked bead **not** reopened | — | bj:ANC-057, TCH.16 | ATC:874-880, 937-938 |
| SL6 | FAIL, count ≤ max | reopen + notify dev | checked bead → open with its assignee (if not open); sanity bead → open, assignee "" (if needed); `atm send <checked assignee> --template sanity-fail.xml.j2` (if sanity task not terminal); close sanity task completed | — | bj:ANC-056, bj:ANC-072, TCH.15. bj:ANC-056 says "on the dev's still-open task", but the dev task is closed by D1 and this is a plain send | ATC:881-889 |

No rebase-required outcome exists in ATC or STH (bj:ANC-065/071 name one; `templates/rebase-required.xml.j2` exists). This report did not find where it is emitted; it is likely in sanity-stack, but that was not checked.

---

### Role sanity: daemon hook (STH)

Input facts: the ATM hook envelope (`hook`, `request`, `caller.identity`, `team`, `phase`, `custom.task_prefix`, `custom.sanity_agents`) on stdin (STH:94-106, 459-462). Selected vars come from `request.message_source.Template` (`task_id`, `checked_bead`, `commit`, `reviewer`, `run_id`, `verdict`, `findings`) (STH:261-270). bd via bd-cache: the sanity bead and the dev bead, refreshed with one batched `bd list --id` (STH:147-168, 283). atm: events for the sanity task and the dev task (STH:170-174, 313, 318). Phase lead from the phase toml (STH:181-188). Prepared plans and errors live in a local SQLite journal (STH:59-75, 176-179). Per-task flock (STH:78-90).

Routing:

| row | guard | reply | governing | source |
|---|---|---|---|---|
| H1 | envelope.hook ≠ task.close | not-applicable | bk:ANC-001, bk:ANC-013 | STH:396-397 |
| H2 | task id not str or lacks prefix (`custom.task_prefix`, default `atm-<phase>-`) | not-applicable | bk:ANC-001, bk:ANC-022 | STH:400-402 |
| H3 | task id contains `.close-error-` | pass-through | bk:ANC-005 | STH:403-404 |
| H4 | `.bd-repair-` task, exactly one journal record with matching repair_task and `repaired` | pass-through | SCH:116-122 | STH:405-414 |
| H5 | `.bd-repair-`, record missing/duplicate/unrepaired | refused + escalation | SCH:116-122 | STH:411-416 |
| H6 | `custom.sanity_agents` is a list without caller | pass-through | bk:ANC-022, SCH:33-35 | STH:418-420 |
| H7 | sanity bead lacks label stage:dev-sanity | pass-through | SCH:33-35 | STH:422-424 |
| H8 | flock held | refused SANITY_CLOSE.BUSY | UNGOVERNED | STH:84-86 |

prepare() refusals (SANITY_CLOSE.INVALID unless noted; field and owner as listed):

| row | guard | code | governing | source |
|---|---|---|---|---|
| HP1 | message_source not Template | INVALID | SCH:20-23 | STH:262-265 |
| HP2 | template value sections not objects | INVALID | UNGOVERNED (defensive) | STH:266-267 |
| HP3 | vars task_id ≠ closing task | INVALID | UNGOVERNED | STH:277 |
| HP4 | sanity metadata.dev_bead missing | INVALID (phase-lead) | SCH:54-57 | STH:279-280 |
| HP5 | vars checked_bead ≠ dev_bead | INVALID | UNGOVERNED | STH:281 |
| HP6 | sanity not blocked by dev bead | INVALID (phase-lead) | SCH:54-57 | STH:284-285 |
| HP7 | dev bead not stage:dev/fix, and not a stage:finding whose sanity_bead is this task | INVALID (phase-lead) | UNGOVERNED | STH:286-289 |
| HP8 | reviewer ≠ sanity-selected | INVALID | UNGOVERNED | STH:293 |
| HP9 | run_id missing | INVALID | SCH:59-61 | STH:294 |
| HP10 | verdict CANNOT_RUN | SANITY_CLOSE.LEGACY_VERDICT (phase-lead) | SCH:74-80. Contradicts TCH.17, which ATC implements (SL1) | STH:296-301 |
| HP11 | verdict not PASS/FAIL | INVALID | SCH:74 | STH:302 |
| HP12 | PASS and dev bead not closed | INVALID (phase-lead) | UNGOVERNED (no ATC equivalent) | STH:303-304 |
| HP13 | findings malformed | INVALID | SCH:59-61 | STH:305-310 |
| HP14 | PASS with findings / FAIL without findings | INVALID | bj:ANC-072 | STH:311-312 |
| HP15 | sanity task has no assignment event | INVALID (phase-lead) | UNGOVERNED | STH:314-315 |
| HP16 | caller ≠ last sanity assignee | INVALID | UNGOVERNED | STH:317 |
| HP17 | dev task no assignment / no assignee / bead assignee ≠ task assignee | INVALID (phase-lead) | SCH:54-57 | STH:319-323 |
| HP18 | phase lead unreadable | SANITY_CLOSE.CONFIG | UNGOVERNED | STH:182-187 |
| HP19 | journal has a prepared plan with a different signature (run_id, verdict, findings) | INVALID (phase-lead) | UNGOVERNED | STH:326-328 |
| HP20 | any command fails / budget (1.8 s deadline) exhausted | SANITY_CLOSE.COMMAND | bk:ANC-011/012 (2500 ms) | STH:101, 112-131 |
| HP21 | invalid JSON from a command | SANITY_CLOSE.DATA | UNGOVERNED | STH:133-137 |
| HP22 | bd-cache error | BD_CACHE.* (passed through) | UNGOVERNED | STH:139-155, 370-371 |
| HP23 | any other exception | SANITY_CLOSE.INTERNAL (hook-maintainer) | REQ-BK-ERRORS (indirect) | STH:430-432, 463-467 |
| HP24 | escalation itself impossible | exit 1 (ATM sees ATM_HOOK_EXIT) | REQ-BK-ERRORS | STH:468-470 |

Every refusal goes through `validation_error` (STH:210-258). Dry run: refused reply only, nothing persisted (STH:228-231). Otherwise one journal entry per error_id. If the lead has not been notified yet, `atm task assign <lead> --task-id <task>.close-error-<id> --template hook-close-escalation.xml.j2`. That assign runs as the hook env's identity, which is the caller (STH:105), not as the lead. The reply is `{"result":"refused"}` (bk:ANC-004, SCH:85-100).

Outcomes:

| row | guard | reply | effects | governing | source |
|---|---|---|---|---|---|
| H9 | dry_run, valid | handled + atm-action list | none | bk:ANC-006 | STH:427-428 |
| H10 | PASS | handled | journal write `close <sanity> "PASS at <commit>"` (if not closed); spawn detached drain; actions: (a) `task.close <dev task>` as dev_agent with sanity-dev-close.md.j2, only if the dev task's last event is not complete (normally it is, after D1); (b) original sanity `task.close` completed | bk:ANC-006/007/009/010, bj:ANC-055. **No QA assignment** (bj:ANC-053 not met; SCH:81) | STH:332-334, 346-358, 361-392 |
| H11 | FAIL | handled | journal writes: dev → open (precondition: assignee == dev_agent), if not open; sanity → open, assignee "" (precondition: assignee unchanged), if needed; drain; actions: `send` sanity-failed-template.md.j2 to dev_agent as caller; original sanity `task.close` completed. **No round count, no cap** (bj:ANC-057 not implemented here) | bj:ANC-056 (partially), bj:ANC-072, SCH:63-74 | STH:335-339, 352-357 |

doctor() codes (not transitions): SANITY_CLOSE.ESCALATION_PENDING, SANITY_CLOSE.REPAIR_PENDING, SANITY_CLOSE.JOURNAL_UNREADABLE (STH:435-455). Governed by bk:ANC-015.

---

### Role qa (ATC `Hook.qa`)

Input facts: vars (`verdict` PASS/FAIL, `commit`, `round`, `qa_bead`); bd: QA bead (ATC:466); atm: QA task events (ATC:467).

| row | guard | outcome | effects | error | governing | source |
|---|---|---|---|---|---|---|
| Q-R1 | QA bead closed with reason ≠ "<verdict> at <commit>" | refused | — | REPLAY.CONFLICT | UNGOVERNED | ATC:566-569 |
| Q-R2 | verdict CANNOT_RUN | refused | — | VARS.INVALID | TCH.18 | ATC:338-339 |
| Q1 | PASS or FAIL | closed | render qa-complete; `bd close <qa> "<verdict> at <sha>"` (if open); `atm send <lead> "QA <verdict> <qa> commit=<sha> round=<n>"`, skipped if the QA task is terminal or lead == caller; `atm task close <qa> completed --template qa-complete.md.j2`; assigns nothing | — | bj:ANC-070, TCH.18 | ATC:891-901 |

STH does not handle QA (STH:455 says "dev/fix/qa use default processing").

---

### Other "transition" code outside the close path

- **TR `next_transition`** (TR:7-25): returns one of "create QA bead", "PASS with backlog", "ROUND_CAP" (QA round ≥2), "one fix round", "wait". Its docstring says it is used by orchestration fixtures. `git grep` finds no production importer, only tests/test_transitions.py and team-lead.md:86 (item 59). It is not called by any close path, and its ROUND_CAP is a QA cap at 2, which is unrelated to the sanity cap.
- **AG gates** (pre-claim, not close): dev READY/PLAN_INVALID/NOT_READY/UNCLAIMABLE/PR_TARGET_MISMATCH/WRONG_BASE (AG:137-161); sanity COMMIT_REQUIRED/NOT_READY/SANITY_FROZEN/COMMIT_MISMATCH (AG:164-176); qa PR_REQUIRED/PR_TARGET_MISMATCH/SANITY_STALE/QA_HEAD_MISMATCH (AG:190-205); GATE_CANNOT_RUN (AG:217-218, 242, 253). None runs during a close; listed for the duplicates section.
- **CV / CF**: build close vars from the bead (no transition). Codes: CLOSE_VARS.NOT_A_TASK_BEAD, NOT_POURED, ROLE_MISMATCH, READ_FAILED, EVENTS_FAILED, NOT_STARTED, CONFIG; ASSIGN_VARS.NOT_QA, NOT_RECORDED, CONFLICT, NOT_PLANNED (CF:54, 66, 70, 98, 104, 110, 114; CV:62, 78-88, 96-108, 141, 166).
- **SCV**: builds sanity vars and stack input. Its errors are plain ValueError text with exit 2 (SCV:13-61, 72-73, 95-96).

---

### Error / refusal codes emitted

ATC (23 distinct, plus pass-through of bd_cache codes): ARGS.INVALID (115, 923), CONFIG.INVALID (190-198, 216, 222, 919), VARS.INVALID (126-134, 147, 159, 181, 300, 304, 326, 332, 335, 337, 339, 347, 349, 351, 362, 675, 856, 858), VARS.UNKNOWN (144, 316), VARS.MISSING (314), VARS.EMPTY (329, 580, 772), BEAD.INVALID (230, 247, 445), BEAD.NO_SANITY (788), REPLAY.CONFLICT (259, 556, 563, 565, 569, 823), COMMAND.INVALID (267, 438, 638, 945, 953), COMMAND.FAILED (405, 423, 426, 634, 949), BEADS_DIR.INVALID (461), BEADS.WRITE_ESCALATED (518), BEADS.WRITE_PENDING (521), STACK.TOP (583), HEAD.DIRTY (596), HEAD.NOT_PUSHED (609, 626), BASE.NOT_ANCESTOR (632), PR.NOT_FOUND (653), PR.AMBIGUOUS (655), ASSIGNMENT.TOO_LARGE (700), EVIDENCE.INVALID (754, 759, 763, 765). Pass-through `BD_CACHE.*` (492, 514). One non-error escalation: SANITY.ROUND_CAP (877, 880).

STH (7 refusal codes plus pass-through BD_CACHE.*, plus 3 doctor codes): SANITY_CLOSE.INVALID (52, 265), .BUSY (86), .COMMAND (120, 126, 129, 131, 151), .DATA (137), .CONFIG (186), .LEGACY_VERDICT (297), .INTERNAL (431, 465); doctor: .ESCALATION_PENDING (445), .REPAIR_PENDING (449), .JOURNAL_UNREADABLE (452). BD_CACHE.* (144, 155, 162, 371). 19 BD_CACHE codes are defined in bd_cache.py.

---

