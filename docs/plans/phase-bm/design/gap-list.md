# atm-bd close transitions: gaps between anchors, design docs and released code

This is phase bm step 1: a list of gaps, not a design. Read-only. Nothing in either repo was edited.

## Sources and revisions

- **A. Anchors** (atm-core `develop` = `472b50fca9ccd1daec7f0853ad63877e411a391e`): `docs/plans/phase-{bg,bh,bi,bj,bk}/anchors.json`. Every anchor is either `locked` or `retired`; none is `proposed`. REQ-BK-* ids are defined only in `phase-bk/anchors.json`, and grep finds no other requirements file for them. The close-relevant anchors are bj:ANC-051..061, 065..072, 078..081, 086, 089; bk:ANC-001, 004..012, 015, 022 and REQ-BK-ERRORS; bh:ANC-004, 017, 023. **Retired, so not authoritative:** bj:ANC-054 (dev-close check set; "retained as sprint design under lead ruling", so TCH.7 governs), bj:ANC-062 (replaced by ANC-065), bj:ANC-063 (replaced by ANC-078..081) and bj:ANC-064 (moved to bk).
- **B. Design docs** (atm-bd tag `v0.19.8` = `fa7aa629fc2ba6d78e6b7354630809e0a3bf1f68`). Abbreviations:
  - `CH` = docs/diagrams/close-hooks.md
  - `LC` = lifecycle.md
  - `SP` = sprint.md
  - `CO` = correlation.md
  - `BC` = bd-cache.md
  - `TCH.n` = docs/requirements/task-close-hook.md item n. Line = item + 11 for items 5-6, + 14 for 7-8, + 14 for 10-12, + 17 for 13-17, + 20 for 18, + 23 for 19-21; the timeouts paragraph is line 48.
  - `DS.n` = docs/requirements/dev-sanity.md item n (line = n + 11)
  - `TL:line` = docs/requirements/team-lead.md
  - `DEV.n` = docs/requirements/dev.md
  - `SCH:n` = scripts/SANITY-CLOSE-HOOK.md
  - `SK:n` = skills/atm-bd-orchestration/SKILL.md
  - `AGT:n` = plugins/atm-bd-orchestration/agents/dev-sanity.md
- **C. Code** (v0.19.8, `plugins/atm-bd-orchestration/skills/atm-bd-orchestration/scripts/`):
  - `ATC` = atm-task-close.py (963 lines)
  - `STH` = sanity_task_close_hook.py (471)
  - `CV` = close-vars.py
  - `CF` = ../../atm-beads/scripts/close_fields.py
  - `SCV` = sanity-close-vars
  - `ST` = stack_top.py
  - `AG` = assignment-gates.py
  - `TR` = transitions.py
  - `SSL` = sanity-stack-link
  - `CON` = apps/atm-bd-console/src/atm_bd_console/atm_bd.py
  - Templates are under `templates/`.

**Prior work check.** `current-close-transitions.md` was read against `origin/main`. `git diff v0.19.8 origin/main` shows no change to any script or diagram cited here; only docs/requirements/sanity-stack.md item 7 differs. Its code line numbers therefore hold, and I spot-checked about 40 of them. Corrections to it:
1. It cites bj:ANC-054 as governing, but that anchor is retired.
2. It did not record that the console refuses to dispatch sanity (GAP-012).
3. It did not record that the not_reproducible fix close breaks the QA resolver (GAP-006).
4. It did not record that timeouts are retried in the offline loop, against TCH (GAP-013).

---

## Plain explanations

### Offline GitHub fallback (dev and fix close)

**What it is.** Before closing a dev or fix bead, `atm-task-close.py` checks three things: the branch is pushed, HEAD descends from the stack top, and exactly one open PR exists at HEAD. The fallback works like this:
- The script tries `git ls-remote origin refs/heads/<branch>` and then `git fetch origin` up to 3 times in a row, with no sleep (ATC:600-620). Any non-zero exit counts as a failure, and so does a 180 s timeout, which is caught as COMMAND.FAILED and retried (ATC:617-620, 422-424).
- After 3 failures it sets `github = "unavailable"` and compares HEAD with the **cached** tracking ref `refs/remotes/origin/<branch>` instead of the live remote (ATC:621-627).
- The ancestry check then runs against the cached `origin/<top>` (ATC:628-634).
- It **skips the PR lookup altogether** and copies `pr_number`, `pr_url` and `pr_base` from the checked bead's metadata (ATC:640-642, 664-669).
- The same switch happens when the git calls succeed but `gh api .../pulls` fails 3 times for any reason, including invalid JSON (ATC:643-662).
- The result JSON carries `github: unavailable` and the last error (ATC:933-936).

**Who uses it.** No one chooses it. It happens to any dev or fix close during a network outage, a rate limit, or any other git or gh failure.

**What requires it.** bj:ANC-059: "must proceed with local git checks when GitHub is unreachable or rate limited, recording that in its result". TCH.6 (line 17) describes the same rule.

**What happens next.**
- The PR identity must already be on the bead. On a first close nothing is recorded yet: the lead records the PR only in *notes* (SK:351). So the close refuses with VARS.EMPTY (ATC:806, 772-773) and does not proceed.
- On a re-close after a sanity FAIL, the identity exists, so sanity is assigned as usual. Stack linking still happens later, through sanity-stack.
- Gaps: GAP-011 (does not proceed on a first close), GAP-013 (timeouts retried), GAP-014 (any failure, not just an outage, triggers it), GAP-022 (stale refs, bh:ANC-004), GAP-030 (not drawn).

### Fix outcome `not_reproducible`

**What it is.** A fix dev (`--role fix`) is assigned a QA finding. It checks the cited file:line, finds the defect absent and closes with result `outcome: not_reproducible` plus `evidence: <file:line>` (fix-assignment.xml.j2 step c; DEV.22, DEV.24). Only the fix role may use it; dev refuses it (ATC:41, 299-300).

**What the code does** (ATC:791-800, 826-835):
- It skips every git and PR check and requires no commit, PR number or URL (ATC:768-769).
- It checks that the evidence line exists in the worktree (ATC:750-765).
- It overwrites the fix bead's `metadata.branch` and `metadata.commit` with the **stack top's** branch and head, and records an empty `pr_number`.
- It closes the bead with "not reproducible: <file:line>".
- It sends fix-complete, **assigns sanity** at the stack-top commit and closes the dev task.
- On sanity PASS, SCV writes no stack input, so nothing is linked (SCV:67).

**What it breaks.** The QA resolver used by the lead and the console refuses this bead: ASSIGN_VARS.NOT_RECORDED for the empty pr_number (CF:102-106) and ASSIGN_VARS.CONFLICT when the branch differs from the QA bead's poured branch (CF:107-111). The QA gate refuses too: `PR_REQUIRED` and SANITY_STALE (AG:191-202). Only the legacy sanity PASS assigns QA directly (ATC:861).

**Which anchor requires it.** None; no anchor mentions it.

**What the docs say.** The docs disagree with each other:
- TCH.10 (line 24): sanity verifies the absence and QA's filing reviewer confirms it.
- SK:358 and TL:43 (item 28): "no commit, no sanity check"; the **lead** closes the group's sanity and QA beads by hand.

The code follows TCH.10 for sanity, but the QA half cannot work. See GAP-006.

### `--skip-checks`

- **Anchor bj:ANC-059.** The flag "bypasses the PR and stack checks, described only in the script's --help as requiring the phase lead's permission and named in no template or prompt".
- **Verified.** The help text is ATC:909, and it matches TCH.5 word for word. `git grep -- --skip-checks` at v0.19.8 finds only TCH.5 (line 16) and ATC; no template, agent or prompt names it. So "intentionally undocumented" means *absent from templates and prompts*. A requirements doc may describe it, and does (TCH.5). Nothing in the anchor forbids drawing it in close-hooks.md, which today does not show it.
- **What it does** (ATC:801-805). It skips all of `checks()`, which also drops the clean-worktree and pushed-head checks. It takes PR identity from bead metadata, and VARS.EMPTY still applies if that is empty. It sets `rebased_onto` to `origin/<pr_target>` and ignores the stack ledger.
- Gaps: GAP-015 (it skips more than "PR and stack checks"), GAP-026 (rebased_onto meaning; flag not drawn), GAP-035 (flag silently accepted for sanity and qa).

### Daemon vs legacy sanity close mode

There are two complete sanity-close implementations:

| | daemon mode | legacy mode |
|---|---|---|
| How a phase gets it | registers `sanity_task_close_hook.py` as the ATM `task.close` hook | no registration |
| Closer | dev-sanity runs `atm task close <sanity> completed --template sanity-task-close.md.j2`; ATM runs STH | dev-sanity runs `sanity-close-vars`, then `atm-task-close.py --role sanity` (ATC:837-889) |
| PASS | journals the sanity close and closes the dev task if still open | closes the sanity bead and task, then **assigns QA** |
| FAIL | reopens the dev bead, sends `sanity-failed-template.md.j2` | **counts rounds**, sends `sanity-fail.xml.j2`, **caps** at max_sanity_rounds |
| QA assign | none | yes |
| Round count and cap | none | yes |
| CANNOT_RUN | refused | accepted |

- **How the mode is chosen.** The dev or fix close picks it, not the sanity close. It reads `atm hook list` as the lead and writes `sanity_close_mode` into the sanity assignment (ATC:68-80, 693-694; SCH:39-42). The dev-sanity template branches on that value and defaults to `legacy` (dev-sanity-template.xml.j2:18, 54-69).
- **What the anchors cover.** bj:ANC-053, 056 and 057 have scope "atm-task-close.py sanity role", which is the legacy path. The bk anchors define the hook mechanism but say nothing about QA, rounds or the fail template. So the daemon hook, the mode the docs treat as primary (LC:75-79, SP:14), meets fewer anchors.
- Gaps: GAP-007 (the split itself), GAP-002/003/009 (daemon misses 053/057/056).

### The four "sanity round" definitions

1. **FAIL counter.** `metadata.sanity_round` on the sanity bead, +1 per FAIL, de-duplicated by `sanity_round_key` (ATC:227-249, 866-871). This is the one bj:ANC-057 names. Only legacy mode keeps it; the daemon hook never touches it (SP:14).
2. **Assignment round.** 1 + the number of `completed` events before the last assignment event of the sanity task (ATC:252-258). It is used only in the replay check (ATC:560). DS.16 (line 27) defines "iteration" the same way.
3. **close-vars round.** The number of `started` events on the sanity task (CV:91-109, 124; CH:143).
4. **sanity-close-vars round.** `close_context.round`, defaulting to 1 (SCV:44). The dev close vars never carry `round` (task-close-vars.j2:54-73), so in practice it is always 1. This is the round `sanity-fail.xml.j2` prints as "round n of max", although TCH.15 means the count.

QA has its own `metadata.round` (LC:87; TR:21). Gaps: GAP-018 and GAP-021.

### The three verdict sets

1. Legacy sanity: PASS, FAIL or CANNOT_RUN (ATC:334; SCV:17). CANNOT_RUN reopens the bead and closes the task `refused` (ATC:843-849; TCH.17; DS.24, 26, 27, 31).
2. Daemon sanity: PASS or FAIL. CANNOT_RUN is refused as SANITY_CLOSE.LEGACY_VERDICT and escalated (STH:296-302; SCH:74-80; AGT:123-129).
3. QA: PASS or FAIL. ATC:334 admits CANNOT_RUN and then ATC:338-339 rejects it.

No anchor names CANNOT_RUN. bj:ANC-086 says "A sanity FAIL or refusal must be sent only as an ATM message". See GAP-020.

### The two dev-task closers

1. ATC:835. The dev's own close script closes the dev's ATM task as the caller, right after assigning sanity. TCH.21 (line 44), CH:37 and 127, SP:13, LC:27 and CO:37 all describe this.
2. STH:349-351. On a daemon PASS the hook returns a `task.close` action for the dev task, as `dev_agent`, but only if the dev task's last event is not `complete`. After closer 1 has run it always is complete, so in practice closer 2 never fires.

**bj:ANC-055** (locked) says the opposite of closer 1. The dev task must stay open after the dev close and be closed *only* by the sanity close hook on PASS, *as the lead*. bj:ANC-060 and ANC-056 ("on the dev's still-open task") depend on that. A further, non-authoritative voice: the atm-core CLAUDE.md interim note says the dev closes its own task by hand. See GAP-001.

### SANITY.ROUND_CAP recipient

- **Code.** Legacy mode only. `atm send <team_lead_member> "SANITY.ROUND_CAP ..."` (ATC:874-880). `team_lead_member` comes from the install-generated repo config; it is "team-lead" in atm-core (develop:.claude/project/atm-bd-orchestration.yaml:7).
- **Docs.** "the lead" (CH:106, SP:164, TCH.16, DS.32). In those same docs "the lead" means the phase lead from `.atm-bd/phase-<x>.toml`, who receives every other escalation (CO:23, 35; SCH:85). That is `solar` for phase bk (develop:.atm-bd/phase-bk.toml:4).
- **Anchors.** bj:ANC-057 says "escalate to team-lead": either the literal identity or the role. bh:ANC-023 says the install yaml "must not carry a lead", though its scope is the console and bead-queues.
- The daemon hook has no cap, so it never sends this at all. See GAP-019.

---

## Gap table

Sorted by severity (blocking, important, minor), then kind (anchor-vs-doc, anchor-vs-code, doc-vs-code, undrawn-code-decision, undocumented-anchor, inconclusive).

### Blocking

| id | kind | A: anchors | B: docs | C: code | closure | sev |
|---|---|---|---|---|---|---|
| GAP-001 | anchor-vs-code | bj:ANC-055 "must stay open after the dev's close hook runs and must be closed only by the sanity close hook on PASS, as the lead"; bj:ANC-060 "starts its next queued task when the sanity hook closes its current task" | Docs follow the code: TCH.21 (l.44) "the hook closes the dev's own task (as the caller) at dev complete"; TCH.12, TCH.14, CH:37, CH:127, SP:13, LC:27, CO:37. DS.28 (l.39) says the sanity close "closes the sanity bead, dev and sanity tasks" (it disagrees with TCH.14) | ATC:835 closes the dev task at dev complete. STH:349-351 closes it on PASS only if not complete (never, after ATC:835), and as `dev_agent`, not the lead (STH:350, 190-208) | change-code: remove the ATC:835 close; make the STH PASS action close the dev task as the phase lead; then update TCH.12/14/21, CH, SP, LC, CO and DS.28 | blocking |
| GAP-002 | anchor-vs-code | bj:ANC-053 "When a sanity bead closes with PASS, the sanity close hook must assign the group's qa bead ... as the phase lead, with no lead action" | CH:93-96 "The hook neither dispatches QA ... the lead builds its vars with close-vars.py ... and runs the atm task assign"; SCH:81; LC:52-54; CO:45 (lead, `templates.qa`); CH:145 (the console dispatches QA). TCH.14 and DS.28 say the hook assigns QA | Daemon: STH:346-358 has no QA action. Legacy: ATC:861 assigns QA. Console: CON:454-471 auto-dispatches QA from bd ready | change-code: the daemon PASS path assigns QA as the lead (as legacy ATC:861 does), then the docs are updated | blocking |
| GAP-003 | anchor-vs-code | bj:ANC-057 "must count FAIL rounds per sanity bead in metadata.sanity_round and, when the count exceeds max_sanity_rounds ... escalate ... instead of reopening" | SP:14 "no round is counted (the legacy close mode counts rounds ...)"; CH:103-107 (legacy only); TCH.15 and TCH.16; DS.29 says "it increments" | Daemon: STH:331-339 never reads or writes sanity_round, has no cap, and always reopens. Legacy: ATC:863-889 counts and caps | change-code: count and cap in the daemon FAIL path | blocking |
| GAP-004 | anchor-vs-doc | bj:ANC-056 and ANC-072: the FAIL is delivered "on the dev's still-open task", with no lead action, "must not create finding beads, a new layer or a new task" | The docs say the lead re-assigns the dev task: CH:96, LC:48, SP:139, SP:156, CO:42, TCH.12, TCH.21. TL:38 (item 23): the lead "assign[s] `dev-fix.xml.j2` on a new layer cut from the current top" and verifies "children" (finding children were retired, DS.47). Only SK:353 matches the anchor: "on the existing task; no child finding or new dev assignment" | No code re-assigns. STH:352-354 and ATC:888 send a message to a dev whose task is already closed (ATC:835), so the dev has no queued work and stays idle until the lead acts | update-doc: once GAP-001 is fixed, the code meets the anchor; rewrite TL item 23, CH:96, LC:48, SP:139/156, CO:42 and TCH.12/21 to "dev's task stays open; no new layer or task" | blocking |
| GAP-005 | anchor-vs-doc | bj:ANC-065: link "before the close hook runs"; when the head does not descend from the top, "send the dev rebase-required on the dev's open task". bj:ANC-066: "the close procedure runs only after it returns stacked". bj:ANC-068: on lock timeout "keep the sanity task active, ask the holder once ... retry; neither may refuse the task, fail the dev or escalate to a person" | Close first, then link: TCH.13 (l.30), TCH.20, DS.6, DS.14, DS.28 ("no stack link or human response gates the close"), CH:72/81, LC:34-42 ("notify the lead, sanity stays closed"), AGT:137-156 | dev-sanity-template.xml.j2:65-67 closes "without waiting for any stack result". SSL:332-337 sends the lead "Sanity task already closed; stack handoff needs lead action" for rebase_required, lock_timeout and stack_failed | ruling-needed: (a) link, then close, as the anchors say; or (b) close, then link, as the docs and code do, and retire those clauses of 065/066/068. Recommend (a): it is the locked intent, and it is the only order in which a rebase_required can reach the dev while the dev's task is still open | blocking |
| GAP-006 | doc-vs-code | None; no anchor mentions not_reproducible | TCH.10 (l.24): sanity at the stack top verifies the absence; "QA's filing reviewer confirms it". SK:358 and TL:43 (item 28): "no commit, no sanity check"; the lead closes the group's sanity and QA beads "not_reproducible: <fix bead>". DEV.22, DEV.24 | ATC:791-800 always assigns sanity (this races with the lead's manual sanity close in SK:358). ATC:826 overwrites the fix bead's branch and commit with the stack top's and records `pr_number ""`. QA is then impossible through the resolver (CF:102-111, NOT_RECORDED and CONFLICT) and the gate (AG:191-202, PR_REQUIRED and SANITY_STALE); only legacy ATC:861 assigns QA | new-anchor-needed: Rand states the not_reproducible path (sanity yes/no, QA yes/no, which branch and commit are recorded); docs and code then follow | blocking |
| GAP-007 | inconclusive | bj:ANC-053, 056 and 057 are scoped to "atm-task-close.py sanity role" (legacy). bk:ANC-001..012 define the daemon hook mechanism, not its sanity behaviour. bj:ANC-064 (the stand-in clause) is retired. No anchor requires two modes | CH:99-107, LC:75-79, SP:14 and 161-170, SCH:10-12 and 39-42 document both modes. DS.28/29 describe legacy behaviour as "the close hook" | The mode is fixed at dev close (ATC:68-80, 693-694); the template default is `legacy` (dev-sanity-template.xml.j2:18). A legacy close in a phase that has the hook registered sends `atm task close ... --template dev-sanity-complete.md.j2` through STH. STH requires `reviewer`/`run_id` (STH:293-294), which SCV strips because they are not contract keys (SCV:50-51). Likely result: the sanity bead is journaled closed but the task close is refused and escalated (not run; inferred from code) | ruling-needed: (a) daemon only: delete the ATC sanity role, SCV and the mode switch, and port 053/056/057 into STH; or (b) keep both, with a mismatch guard. Recommend (a): one machine is the goal of phase bm, and today the same anchors give two behaviours | blocking |

### Important

| id | kind | A: anchors | B: docs | C: code | closure | sev |
|---|---|---|---|---|---|---|
| GAP-008 | anchor-vs-doc | bj:ANC-058 "must pass that same file unchanged to the atm task assign and atm send templates it runs" | TCH.2 (l.12) and TCH.8 (l.22): a separate dispatch file plus `--result`, and "ephemeral completion vars" built from dispatch + result + derived | ATC:789-799 and 808-809 send a temporary completion file; ATC:712-716 assigns from a temporary dispatch file (close_context, beads_dir, sanity_close_mode) | ruling-needed: (a) amend ANC-058 to the two-file plus derived-vars design; or (b) pass the lead file unchanged. Recommend (a): the derived commit and PR fields cannot be in the lead's immutable file | important |
| GAP-009 | anchor-vs-code | bj:ANC-056 and ANC-072: the FAIL is "only the sanity-fail.xml.j2 message ... listing exactly the deliverables that were NOT complete ... and referencing the sprint" | CO:42 documents both templates: daemon `sanity-failed-template.md.j2`, legacy `sanity-fail.xml.j2`; SP:141 "sanity-failed" | STH:353 uses sanity-failed-template.md.j2, which has no sprint, branch, PR or round. ATC:888 uses sanity-fail.xml.j2 | change-code: the daemon path sends sanity-fail.xml.j2; delete sanity-failed-template.md.j2 | important |
| GAP-010 | anchor-vs-code | bj:ANC-065 "send the dev rebase-required on the dev's open task" | LC:37-38, DS.32, DS.50, TL:62: rebase_required is sent to the lead | SSL:251-252 returns rebase_required; SSL:332-337 sends plain text to the lead. `templates/rebase-required.xml.j2` is shipped but never sent by any script (git grep) | change-code: send rebase-required.xml.j2 to the dev (this depends on GAP-001/005) | important |
| GAP-011 | anchor-vs-doc | bj:ANC-059 "must proceed with local git checks when GitHub is unreachable or rate limited"; quote: "we can't afford to stop due to rate limiting" | TCH.6 (l.17) narrows it: "sanity and QA proceed only when that identity is non-empty"; otherwise VARS.EMPTY | ATC:664-669, 806, 772-773: offline uses the PR identity recorded in bead metadata. On a first close nothing is recorded (the lead records PRs in notes, SK:351), so the close refuses | ruling-needed: (a) keep the refusal and amend the anchor; or (b) proceed to sanity with an empty PR identity and let the QA resolver, which already refuses an empty PR (CF:102-106), hold QA. Recommend (b): it honours the locked "must proceed" and still never ships a QA without a PR | important |
| GAP-012 | anchor-vs-code | bj:ANC-052 "...and by atm-bd when bd ready lists the sanity bead with no ATM task"; bj:ANC-061 | TCH.19 (l.42): "The fallback ... is the console or the lead assigning it as the lead" | CON:443-447 refuses every sanity dispatch whose template is dev-sanity-template.xml.j2 (config_invalid "close vars unavailable"); atm-core's phase toml uses exactly that template (develop:.atm-bd/phase-bk.toml). So a dev close that dies after the bead close strands sanity | change-code: the console builds the sanity dispatch (close_context, beads_dir, sanity_close_mode) from the bead, as CF does for QA | important |
| GAP-013 | doc-vs-code | — | TCH timeouts paragraph (l.48): "A call that times out is not retried: it fails the close with COMMAND.FAILED"; ATC:46 comment says the same | ATC:617-620 catches COMMAND.FAILED, including a timeout (ATC:422-424), in the ls-remote/fetch loop and retries 3x, then goes offline; ATC:646-650 does the same for gh api. Worst case before offline is about 3x180 s plus 3x180 s | change-code: do not retry a timeout; go offline at once (or fail as documented) | important |
| GAP-014 | undrawn-code-decision | bj:ANC-059 is limited to "unreachable or rate limited" | TCH.6 is limited to "unreachable or rate limited" | ATC:604-606 and 612-614: *any* non-zero ls-remote or fetch exit (auth failure, missing repo) triggers offline. ATC:647-650: any gh failure, including COMMAND.INVALID (bad JSON), triggers offline. So an auth or config error is reported as `github: unavailable` and the close proceeds on cached refs | change-code: go offline only on network or rate-limit errors; other failures refuse with their code | important |
| GAP-015 | anchor-vs-code | bj:ANC-059 "a --skip-checks flag that bypasses the PR and stack checks" | TCH.5 (l.16) "skips the PR and stack checks only, never vars validation, derived identity validation or state writes" | ATC:801-805 skips all of `checks()`, which also drops HEAD.DIRTY (ATC:594-596) and HEAD.NOT_PUSHED (ATC:598-627) | change-code: keep the clean-worktree and pushed-head checks under --skip-checks | important |
| GAP-016 | doc-vs-code | — | CH:65 "refused, no bead write"; LC:50; SCH:90-91 "They do not close the task or mutate beads" | STH:210-226 and 361-374: a failure inside execute() after some writes were journaled is still refused, but those writes stay and drain (`progress.journaled`) | update-doc: state that execute-time refusals keep their journaled writes | important |
| GAP-017 | undrawn-code-decision | — | SCH:57-61 "An observed commit may be retained as evidence provenance" | STH:291 falls back to `commit = "unspecified"`, so the sanity bead closes "PASS at unspecified" (STH:334). AG:179-202 then cannot match a sha, so QA is refused SANITY_STALE | change-code: refuse PASS without a commit (or use the dev bead's recorded `metadata.commit`, refusing if that is empty) | important |
| GAP-018 | undrawn-code-decision | bj:ANC-057 defines only the FAIL counter | CH:143 (round = started events); DS.16 (iteration = completed events + 1); TCH.15 ("round n of max_sanity_rounds" = the count) | Four definitions (see Plain explanations): ATC:227-249 and 866-871; ATC:252-258; CV:91-109 and 124; SCV:44. sanity-fail.xml.j2 prints SCV's round, which is always 1 | change-code: keep `metadata.sanity_round` as the one sanity round; derive the displayed round and the replay key from it; delete the CV and SCV sanity round derivations | important |
| GAP-019 | inconclusive | bj:ANC-057 "escalate to team-lead" (the identity or the role?). bh:ANC-023: the install yaml "must not carry a lead" (scope: console and bead-queues) | CH:106, SP:164, TCH.16, DS.32: "the lead". Every other escalation goes to the phase-toml lead (CO:23 and 35, SCH:85). README:192 calls `team_lead_member` "the ANC-057 round-cap recipient" | ATC:878 sends to config `team_lead_member` ("team-lead" in atm-core); the phase lead differs (`solar` for bk). Daemon mode sends nothing (GAP-003) | ruling-needed: (a) the phase lead from `.atm-bd/phase-<x>.toml`; or (b) `team_lead_member`. Recommend (a): every other close escalation goes there, and bh:ANC-023 moves the lead out of the install yaml | important |
| GAP-020 | inconclusive | No anchor names CANNOT_RUN. bj:ANC-086 "A sanity FAIL or refusal must be sent only as an ATM message"; bj:ANC-070 says QA is "PASS and FAIL" | Legacy accepts it: TCH.17, DS.24, DS.26, DS.27, DS.31. Daemon rejects it: SCH:74-80, AGT:123-129 | Three sets: ATC:334 and SCV:17 {PASS, FAIL, CANNOT_RUN}; STH:296-302 {PASS, FAIL}; QA ATC:334 then 338-339 {PASS, FAIL} | ruling-needed: (a) {PASS, FAIL} only, with "cannot run" as an ATM task `refused`; or (b) keep CANNOT_RUN as a verdict. Recommend (a): the daemon docs already reject it, and a refusal is an ATM-native outcome, as ANC-086 implies | important |
| GAP-021 | inconclusive | — | — | Legacy replay check: vars `round` comes from SCV and is in practice always 1 (SCV:44; the dev vars carry no round, task-close-vars.j2:54-73). After the first FAIL the assignment round is ≥2 (ATC:257), so ATC:560-563 treats the round-2 close as "older assignment vars" and refuses REPLAY.CONFLICT when the sha changed. Not run; inferred from code, and no test covers round ≥2 | change-code: add a deterministic legacy round-2 close test; fix if it fails (moot if GAP-007 (a) deletes legacy) | important |
| GAP-022 | inconclusive | bh:ANC-004 "failed fetch means unavailable, not stale-ref fallback" (scope: when the console dispatches or holds sanity/QA) | TCH.6 "the pushed head is on origin/<branch>", without saying cached or live | Offline, ATC:623-627 compares the **cached** `refs/remotes/origin/<branch>`, and the top and ancestry come from cached `origin/<top>` (ATC:581, 588, 630). This is a stale-ref fallback that ends in a sanity dispatch | ruling-needed: does bh:ANC-004 bind the close script? (a) yes: offline mode must not trust cached refs; (b) no. Recommend (a): the dev close is now the main sanity dispatcher, so its scope "defines when sanity ... is dispatched" fits | important |
| GAP-023 | inconclusive | bj:ANC-051 "...while every notice atm-bd sends keeps the constant sender atm-bd@tool" | TCH.4 (l.15) "Every other bd and atm write runs as the caller" | ATC:680-684 (dev-complete, QA status, ROUND_CAP) and STH:353 (sanity-failed) send as the caller identity | ruling-needed: are close-script notices "atm-bd" notices? (a) yes: send as atm-bd@tool; (b) no: keep the caller. Recommend (b): the close runs in the agent's own shell, and TCH.4 is the later, more specific ruling | important |

### Minor

| id | kind | A: anchors | B: docs | C: code | closure | sev |
|---|---|---|---|---|---|---|
| GAP-024 | anchor-vs-doc | bj:ANC-057 cap = max_sanity_rounds, default 5 | TL:39 (item 24) "Sanity FAIL (second, `SANITY.ROUND_CAP`)" implies a cap of 2 | ATC:874 uses config max_sanity_rounds (5 in atm-core) | update-doc: TL item 24 → "FAIL above max_sanity_rounds" | minor |
| GAP-025 | doc-vs-code | — | CH:45 draws REPORT and ASSIGN after every applied write; TCH.8 says "When a sanity assignment is due and none is already open" | ATC:821 and 830-832: report only if `assign and not open_assignment` and lead ≠ caller; assign only when due | update-doc: draw the guard in CH | minor |
| GAP-026 | doc-vs-code | bj:ANC-059 | close-hooks.md does not draw --skip-checks; TCH.2 says `rebased_onto` is the top rebased onto | ATC:805 under skip sets rebased_onto = origin/<pr_target> and ignores the stack ledger (ST:9-29) | update-doc: draw --skip-checks in CH (allowed; it is not a template or prompt) and state its rebased_onto | minor |
| GAP-027 | doc-vs-code | — | CH:42 order is DERIVE → WRITE → REPORT → ASSIGN | ATC:824-825 builds and size-checks the sanity dispatch (`atm hook list`, render, ASSIGNMENT.TOO_LARGE) **before** the bead write | update-doc | minor |
| GAP-028 | doc-vs-code | — | CH:79-80: every verdict path → DRAIN | STH:376: drain only when the plan has writes | update-doc | minor |
| GAP-029 | doc-vs-code | — | CH:139 "QV → atm task assign"; CH:95-96 says the lead runs the printed command | CV:159-169 and 190-200 only print it; CON:454-471 runs it automatically | update-doc (moot if GAP-002 moves QA into the hook) | minor |
| GAP-030 | undrawn-code-decision | bj:ANC-059 | Offline mode is not drawn in CH:30 (DERIVE) | ATC:640-642: a git-transport failure skips the gh PR lookup entirely, even when gh would answer | update-doc: draw the offline branch and its trigger | minor |
| GAP-031 | doc-vs-code | — | dev-sanity-template.xml.j2:65 (daemon) "do not invoke atm-task-close.py" | CV:172-177 prints `atm-task-close.py --role sanity` as the next step for every sanity bead | change-code: print the mode-correct close (or nothing for daemon) | minor |
| GAP-032 | undrawn-code-decision | — | — | Assign and replay selector ATC:816-823 (terminal, open_assignment, sha_changed; REPLAY.CONFLICT "sanity is closed at a different sha") is not drawn | update-doc | minor |
| GAP-033 | undrawn-code-decision | bj:ANC-057 "max_sanity_rounds from the repository configuration" | TCH.16 | Two sources: ATC:874 caps on config; sanity-fail.xml.j2 prints vars `max_sanity_rounds`, poured into the close vars (task-close-vars.j2:62, CF:48) | change-code: one source (config) | minor |
| GAP-034 | undrawn-code-decision | — | — | STH:303-304: PASS refused while the dev bead is not closed. SCV:27-31: a FAIL whose only findings are deliverable 0 becomes "all deliverables, reason = lint". Neither is drawn | update-doc | minor |
| GAP-035 | undrawn-code-decision | bj:ANC-059 (dev and fix PR checks) | — | ATC:909 and 932: `--skip-checks` is accepted and silently ignored for `--role sanity` and `qa` | change-code: refuse it with ARGS.INVALID for sanity and qa | minor |
| GAP-036 | undrawn-code-decision | — | — | ATC:581-583 STACK.TOP: `run` already raises COMMAND.FAILED on a git failure, so an empty head from a successful rev-parse is the only path left, and it is practically unreachable | delete-code | minor |
| GAP-037 | undrawn-code-decision | — | SCH:116-122 itself says delete "once none remains open" | STH:405-416 `.bd-repair-` routing and the doctor's REPAIR_PENDING (STH:448-450); atm-bd PR #607 (OPEN) removes them | delete-code (land #607) | minor |
| GAP-038 | undrawn-code-decision | No anchor on a QA round cap | TL:42 (item 27), TL:86 (item 59), quality-mgr.md item 18: QA ROUND_CAP at round 2 | TR:7-25 `next_transition` has no production caller (git grep: only the manifest and tests); no close path enforces a QA cap | ruling-needed: (a) delete TR and the doc rows; or (b) enforce the QA cap in the QA close. Recommend (a): nothing runs it, and no anchor asks for it | minor |
| GAP-039 | inconclusive | bj:ANC-051 covers assigns "for a sanity or qa bead" only | SCH:93-97 (assigns a phase-lead escalation task) | STH:105 and 251: the escalation `atm task assign <lead>` runs as the caller (dev-sanity), not the lead; the escalation task has no bead | ruling-needed: (a) as the lead, like other assigns; or (b) as the caller. Recommend (a), for one assigner identity across close paths | minor |

---

## Agreed: anchor, doc and code match

- AGR-01 bj:ANC-051: sanity and QA `atm task assign` run as the phase lead from `.atm-bd/phase-<x>.toml` (ATC:365-368, 716, 719; TCH.4; CO:30-35).
- AGR-02 bj:ANC-058 (validation part): a strict header-only render runs before any live check (ATC:164-182, 927; TCH.2).
- AGR-03 bj:ANC-059 (flag text): `--skip-checks` help text is exact, and the flag appears in no template or prompt (ATC:909; TCH.5; grep).
- AGR-04 bj:ANC-059 (recording): `github: unavailable` and `github_error` are in the result (ATC:933-936; TCH.6).
- AGR-05 bj:ANC-056 (reopen): a FAIL reopens the checked bead to its dev assignee, in both modes (ATC:881-883; STH:336-337; SP:150-153).
- AGR-06 bj:ANC-072 and ANC-086: no finding or child beads, and no class bead, on a sanity FAIL (STH, ATC; DS.47; SP:159).
- AGR-07 bj:ANC-070: QA close closes the bead and task, sends the lead a PASS/FAIL status and assigns nothing (ATC:891-901; TCH.18; LC:59-61).
- AGR-08 bj:ANC-071: no stack outcome touches sanity_round (SSL; DS.32, DS.50; TCH.12).
- AGR-09 bj:ANC-061: the hook's sanity assign is not repeated while an assignment is open (ATC:709, 821; TCH.19).
- AGR-10 bj:ANC-052 (dev-close clause): the dev or fix close assigns sanity to dev_sanity_member with dev-sanity-template (ATC:832; TCH.8; CO:44).
- AGR-11 bk:ANC-001 and ANC-005: a non-sanity task or caller gets not-applicable or pass-through and the default close (STH:396-404, 418-424; CH:75; SCH:33-35).
- AGR-12 bk:ANC-004: an invalid sanity result is refused and the task stays open (STH:210-258; CH:65; SCH:90).
- AGR-13 REQ-BK-ERRORS: when escalation is impossible the hook exits 1, which ATM reports as ATM_HOOK_EXIT (STH:463-470).
- AGR-14 bk:ANC-012: hook work budget is 1.8 s, plus up to 2.3 s for escalation, within the 2.5 s limit (STH:101, 250; CH:15; SCH:114).
- AGR-15 bk:ANC-009 and 010: hook actions carry `as`, and every bd call stays inside the hook (STH:190-208, 361-374).
- AGR-16 Dev-close checks with GitHub reachable: clean worktree, pushed head, top ancestry, exactly one PR (PR.NOT_FOUND / PR.AMBIGUOUS) (ATC:591-660; TCH.7). bj:ANC-054 is retired, so TCH.7 governs.
- AGR-17 Bead writes go through the bd-cache journal, with BEADS.WRITE_PENDING / ESCALATED (ATC:502-524; CH:43-44; BC). No anchor covers this; doc and code agree.
- AGR-18 TCH.11: refused and cancelled dev closes bypass the script (no code path; dev templates use `atm task close ... refused`).
- AGR-19 Legacy PASS (ATC:851-862) matches CH:103 and bj:ANC-053, but in legacy mode only.

## Added from live sc-lint phase-l reports (2026-10-27)

| GAP-040 | doc-vs-code | Repo config reviewers_round1 (sc-lint: ["req-qa","arch-qa"]) is the documented source for qa-template reviewers | Round-2 QA pour for a fix of a fix (lint-phase-l.r-1-r2-qa, lint-phase-l.review-f22-r2-qa) stamps reviewers_round1=["team-lead"]; validate-plan then refuses with "unregistered reviewer". It has happened twice, and the sc-lint lead corrects it by hand at dispatch | change-code: the round-2 pour takes reviewers_round1 from repo config (or from the round-1 QA bead), never from the lead identity; add a pour-plan test | important |
| GAP-041 | doc-vs-code (live instance of GAP-006) | SK:358, a not_reproducible fix: no sanity check, the lead closes sanity and QA | lint-phase-l.review-f24 closed not_reproducible. Its close record pinned f26's branch and commit (bbe8b0a0) and base f22, and a sanity task was still assigned. lint-sanity refused with COMMIT_MISMATCH. The repair was the SK:358 hand close | covered by the GAP-006 ruling; also add a test that a not_reproducible close records the fix bead's own branch and assigns no sanity task | blocking (with GAP-006) |

## Rand rulings (2026-10-28)

Rand's words, verbatim: "10 should be phase-lead.  A - approve"

- GAP-039: the sanity hook's escalation `atm task assign` runs as the phase lead, from `.atm-bd/phase-<x>.toml` `lead`, not as the caller.
- GAP-006 / GAP-041: approved path for not_reproducible:
  - no sanity check and no stack layer;
  - the fix bead keeps its own branch and commit;
  - the lead closes the group's sanity bead and QA bead with "not_reproducible: <fix bead>" (SKILL.md:358).
  - Code changes: atm-task-close.py stops assigning sanity on not_reproducible, and stops overwriting the branch and commit with the stack top's.
  - To become a locked bm anchor.
- Items 1-9 (GAP-005, 007, 008, 011, 019, 020, 022, 023, 038): not yet ruled.
- Rand (2026-10-28): "accept the rest". Items 1-9 take the recommended option: GAP-005 (a), GAP-007 (a), GAP-008 (a), GAP-011 (b), GAP-019 (a), GAP-020 (a), GAP-022 (a), GAP-023 (b), GAP-038 (a).
| GAP-042 | doc-vs-code (live, phase bn) | The CLAUDE.md interim note and bj:ANC-089 say the vars file is the union of the close vars and the template vars | atm-task-close.py rejects the union file with VARS.UNKNOWN (phase_root, test_command) at bn-1's close (2026-10-28). The lead has to hand out a second, contract-only file from close-vars.py | covered by GAP-008 (a): one design for the lead file, result file and derived values. Until then, the lead hands the dev a close-vars.py file at dispatch and keeps it separate from the template vars file | important |
| GAP-043 | doc-vs-code (live, phase bn) | close-vars.py:125 takes the sanity close's reviewers_round1 (a list) from the QA bead | At bn-1's sanity close (2026-10-28), the vars file dev-sanity built had reviewers_round1 "" (a string). atm-task-close.py --role sanity refused with VARS.INVALID ERR_VAL_ARRAY_SHAPE_MISMATCH at qa-template.xml.j2:18. Two builders disagree (sanity-close-vars/dev-sanity flow versus close-vars.py) | covered by GAP-007 (a): delete the legacy sanity close and sanity-close-vars. Until then, dev-sanity builds its vars with close-vars.py | important |

## GAP-044: fix-group pour writes the beads repo's worktree_base for a cross-repo phase
- Observed 2026-10-28: atm-bn-1.qa1-f1-r2-{fix,sanity,qa} were poured with worktree ../atm-core-worktrees/fix/bn-1-qa1-f1. Phase bn's code repo is atm-bd (code_checkout), so the path must come from the code repo's worktree base. The lead re-stamped all three by hand. Same class as atm-bd #608.
- Recommendation: the pour resolves worktree from the code_checkout repo's config when code_checkout is set; a test covers a cross-repo phase.

## GAP-045: Parallel Quick Fix QA has no close path
- Observed 2026-10-28 on atm-bn-qf-typeddict-guard-qa (QA of #613). close-vars.py refuses with CLOSE_VARS.NOT_POURED ("metadata lacks sanity_bead"), but SKILL.md says sanity_bead is empty for a quick fix. close_fields_backfill.py derives the group from checked_bead (the finder's bead, atm-bn-2.group-dev), so it would bind bn-2's sanity bead and title to the quick-fix QA. With ok=false it wrote nothing.
- Recommendation: give quick_fix=true QA beads their own close contract (no sanity_bead or dev_task_id, and no group derivation from checked_bead), add one table row in close-hook-qa, and add a test.
- GAP-045 (cont.): the stack bead's ledger gains layers only on a sanity PASS, so a quick-fix layer (no sanity) never enters it, and assignment-gates.py stack-top keeps printing the layer below. Devs need a lead override to rebase onto the hosted top. Recommendation: the quick-fix QA PASS (or the lead's link) records the layer.

## GAP-046: dev close did not assign the sanity task (bn-2)
- Observed 2026-10-28 05:32Z: atm-bn-2.group-dev closed completed (PR #614), but atm-bn-2.group-sanity stayed open and unassigned with no ATM task. The r2 fix close at 04:45Z did assign its sanity, so the behavior is inconsistent. A possible cause is the lead-override base (fix/bn-typeddict-guard, absent from the stack ledger). Not verified.
- The lead assigned it by hand.
- GAP-046 (cont.): atm-bn-2.group-sanity: the PASS task close left the bead in_progress, and the lead closed it.
- GAP-046 root cause (verified): the lead dispatched with sanity_close_mode=daemon, but no daemon bead write exists in bn, so the bead stayed in_progress. Legacy mode is required until bk ships. The template default is legacy; the lead overrode it by mistake.

## GAP-047: QA-poured finding beads lack the group fields the dev gate requires
- Observed 2026-10-28 on atm-bn-2.group-qa-f1/f2: validate-plan exited 5 (PLAN_INVALID) for missing role, stack_bead, checked_bead, checked_title, qa_bead, phase_feature, lint_command, policy_path and max_sanity_rounds. close_fields_backfill --apply returned ok with no fills for them. The lead stamped them by hand.
- Recommendation: either the finding pour carries the group fields, or backfill fills them once the sanity and QA beads are imported.

## GAP-048: a pending layer enters the stack ledger before its own sanity, so the fix close derives rebased_onto=self
- Observed 2026-10-28: #617 (f2) sat in atm-phase-bn.stack layers before cipher's close, and assignment-gates stack-top returned its own branch. The lead removed it. bd --set-metadata on a list stores a string, so use --metadata @file.
- Recommendation: stack-top and the fix hook exclude the closing task's own branch, or the ledger records a layer only on its sanity PASS.

- GAP-049: dev-sanity-template accepts any close_context object. The bn-4 re-dispatch (lead error) carried only notes. sanity-close-vars then refused at close time ("lacks phase, primary_checkout, qa_bead, qa_task_id") after the whole review had run. The render should fail when close_context lacks the role's contract keys (from close-vars.py sanity output + budget + phase_lead).
- GAP-050: Jev split of a 13-deliverable sanity into 101 parts per deliverable: 12 deliverables 101/101 `no`, D7 90 no / 11 yes; the LLM review passed all 13. Receipts: /private/var/tmp/atm-bn-4-group-sanity/reviews/raw-*.json. P1 issue to file.
- GAP-051: the fix close hook journals bd writes with actor "lumen" (bare identity), but the dev claim set assignee "lumen@atm-dev". bd refuses the close (assignee/actor mismatch), the drain retries 5x, then escalates (bd-cache seq 244, atm-bn-4.group-qa-f1). The actor must use the same identity form as the claim.
- GAP-052: the lead-run `just lint` and `just test` use local Python 3.14, but CI's console and package-tests jobs run 3.11. bn-4 (#615, c8c70055) passed sanity and the lead's local gates while red on 3.11: Pydantic TypeAdapter refuses typing.TypedDict below 3.12 (install.py:74/341/388). Sanity first saw it through hosted checks on #618. Candidate fix: the lead evidence step runs the CI floor interpreter, or sanity reads exact-head check-runs.
- GAP-053: the atm-bd oversight cron (2026-10-28 15:59Z, msg 01M61KTH8HPC499CTEHE2HCN28) reported "top PR #615 CI failed" on phase bn. #615 is a lower layer. Its 3.11 failure is fixed by #618 above it (sanity and QA PASS), and the ledger top is #619, with #620 linked above. The cron picks the wrong top layer and asks for a fix on a frozen layer, which would violate the stack rules.
- GAP-054: stack-top reads only the ledger, and a layer enters the ledger only at its sanity PASS. Parallel fixes therefore resolve to a stale top while a sibling PR is already linked and in sanity. On bn-3, f2's #621 was based on #619 although #620 was linked above it, which forks the stack. stack-top should take the linked hosted top, or linking should record a pending layer.
