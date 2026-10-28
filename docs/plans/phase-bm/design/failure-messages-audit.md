# Failure-path message audit: close-hooks target vs atm-bd v0.19.8

Question: "for all of the failure/escalation paths, is there a message that directs the agent to solve the issue?"

Code: atm-bd-orchestration tag `v0.19.8`, files read in full: `scripts/atm-task-close.py` (ATC), `scripts/sanity_task_close_hook.py` (HOOK),
`scripts/close-vars.py` (CV), `atm-beads/scripts/close_fields.py` (CF; it lives under atm-beads, not atm-bd-orchestration/scripts),
`scripts/bd_cache.py` (BDC), plus the senders they hand off to: `scripts/bd_cache_cron.py` (CRON), `scripts/sanity-stack-link` (SSL),
and templates `hook-close-escalation.xml.j2` (HCE), `sanity-failed-template.md.j2` (SFT), `rebase-required.xml.j2`.
Target: `close-hooks.target.md` diagrams A-E.

How v0.19.8 delivers failures:

- **ATC (dev, fix, qa close):** prints `{"err":{"code","message","fix"}}` to the caller's stdout and stderr and exits 1
  (ATC:940-959). It sends **no ATM message for any refusal**. A refusal the phase lead owns reaches only the dev or quality-mgr.
- **HOOK:** returns `{"result":"refused","message":<entry JSON>}` to the closing caller. It also assigns the escalation task
  `<task>.close-error-<id>` to the phase lead, as the caller, using HCE (HOOK:210-258). This audit assumes ATM shows the refused
  message to the caller of `atm task close`. The `recovery` text is fixed and always starts with "Phase lead: ..." (HOOK:219-221),
  even when `owner` is `sanity`.
- **CV/CF:** prints `{"ok":false,"error":{code,message,fix}}` and exits 2 (CV:192-194). It sends no ATM message.
- **CRON:** sends `escalation_text` to the phase lead for each escalated bd-cache write (CRON:77-80, 132-141).

Legend: FIX = the text names the exact command or action. VAGUE = it names the problem but not the action. NONE = a code only,
or a misleading message. silent = no message reaches anyone who acts. missing = the edge does not exist in v0.19.8.
"right owner" compares the receiver with the target doc's owner for that edge.

## Diagram A: dev or fix close (`atm-task-close.py --role dev|fix`)

| edge | error code | code @v0.19.8 | receiver | message (quoted, file:line) | class | right owner | recommended message (recipient) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A1 (`--result` absent) | ARGS.INVALID | yes | dev stdout/stderr JSON | "--result is required for dev and fix" / "Pass the fixer's result file with --result <result-file>." ATC:923-924 | FIX | yes | — |
| A1 (`--skip-checks` with `--role qa`) | ARGS.INVALID | **missing** (the flag is silently ignored for qa, ATC:932) | nobody | — | missing | — | (quality-mgr) "ARGS.INVALID: --skip-checks applies only to dev and fix; rerun without it." |
| A1 (argparse) | ARGS.INVALID | yes | dev JSON | "Supply --role dev\|fix\|sanity\|qa and --vars <shared-vars.json>." ATC:115 | FIX | yes | Drop `sanity` from the role list (target deletes it). |
| A2 (timeout env invalid) | CONFIG.INVALID | yes | dev JSON | "Unset ATM_BD_GATE_TIMEOUT_S or set it to a positive integer number of seconds, e.g. ...=180." ATC:919-920 | FIX | partly (the dev's own env; target names the phase lead) | — |
| A2 (repo config unreadable or missing) | CONFIG.INVALID | yes | dev JSON | "Run the atm-bd-orchestration installer for {root} to create {path}." / "Repair the readable UTF-8 YAML mapping in {path}..." ATC:207-217 | FIX (for the installer) | **no**: the dev gets the installer's fix | (dev) "Repo config {path} is unreadable. Do not edit it: send this JSON to the phase lead with `atm send <lead> --stdin`, then wait for its reply." The script should also send that notice itself, as the caller. |
| A2 (`dev_sanity_member` empty) | CONFIG.INVALID | yes | dev JSON | "Set {field} in the consumer registry and rerun the installer to regenerate {path}." ATC:221-223 | FIX (for the installer) | **no** | Same as above: forward to the phase lead (auto-notify). |
| A2 (phase toml `lead` or `.atm.toml` default_team missing) | CONFIG.INVALID | yes | dev JSON | "Set {name} to a non-empty string in {path}." ATC:192-199 | FIX (for the lead) | **no**: the dev cannot reach an unnamed lead | (dev) "`.atm-bd/phase-<x>.toml` has no lead: send this JSON to `team-lead` (`atm send team-lead --stdin`) and stop." |
| A2-adjacent (ATM_IDENTITY unset) | VARS.INVALID | yes | dev JSON | "ATM_IDENTITY is not set" / "Run as the assigned ATM member." ATC:362 | VAGUE | yes | "Export ATM_IDENTITY=<your member name> (and ATM_TEAM) and rerun with the same --vars and --result." |
| A3 | VARS.UNKNOWN | yes | dev JSON | "Remove fields that are not {role} result fields from --result." ATC:144 | FIX | yes | — |
| A4 (field in both files) | VARS.INVALID | yes | dev JSON | "Pass each completion field once: remove it from the lead's dispatch file or from --result." ATC:147-148 | FIX | partly: it invites the dev to edit the lead's immutable file | "Remove {fields} from --result; never edit the lead's vars file." |
| A4 (bad JSON) | VARS.INVALID | yes | dev JSON | "Correct JSON at line {l}, column {c} in {path}." ATC:131-132 | FIX | yes for the result file; **no** for the lead's vars file | When the path is `--vars`: "The lead's vars file is malformed: send this JSON to `<lead>` with `atm send` and wait for a corrected file." |
| A4 (outcome not allowed) | VARS.INVALID | yes | dev JSON | "Set outcome to {allowed} in {path}, then rerun the hook." ATC:159-160 | FIX | yes | — |
| A4 (dev role + not_reproducible) | VARS.INVALID | yes | dev JSON | "Use the fix role for an unreproducible finding." ATC:300 | VAGUE (the dev cannot change its role) | yes | "not_reproducible is a fix outcome: set outcome to fixed in --result, or ask `<lead>` (`atm send`) to re-dispatch the work as a fix bead." |
| A4 (strict header render fails) | VARS.INVALID | yes | dev JSON | "Supply every field required by the shared task-close vars contract." ATC:181-182; also "Supply vars compatible with {template}." ATC:675 | VAGUE | **no** when the field is the lead's | Name the field and its file: "{field} is a result field: add it to --result" or "{field} is in the lead's vars: send this JSON to `<lead>`." |
| A4 (task id ≠ bead id) | VARS.INVALID | yes | dev JSON | "task ids and bead ids differ" / "Use bead ids as the corresponding task ids." ATC:350-351 | VAGUE | **no** (the lead's vars) | (dev) "The lead's vars give {task}≠{bead}: send this JSON to `<lead>`; the lead regenerates it with `close-vars.py <bead> --out <vars>`." |
| A5 | VARS.MISSING | yes | dev JSON | "{fields}" / "Supply every shared template variable in the lead's vars file." ATC:313-314 (the set also includes result fields, ATC:305) | VAGUE | **no** | Split by owner: missing result fields → "add {f} to --result"; missing shared fields → "send this JSON to `<lead>`; it regenerates the vars with `close-vars.py <bead> --out <vars>`". |
| A6 | VARS.EMPTY | yes | dev JSON | "{name}" / "Populate {name} in the shared vars file." ATC:328-329 | VAGUE | **no** | As A5, shared-field branch. |
| A9 | EVIDENCE.INVALID | yes | dev JSON | "Supply a repo-relative file:line citation." / "Cite a file inside the assigned worktree." / "Cited line {n} does not exist in {f} ({m} lines)." ATC:753-765 | FIX | yes | — |
| A9 (pre-check in validate) | VARS.INVALID | yes | dev JSON | "not_reproducible requires evidence file:line" / "Supply the authoritative absence location." ATC:325-326 | VAGUE | yes | "Set evidence in --result to a repo-relative path:line that shows the absence." |
| A10/A11 (OPEN GAP-006) | — | **differs**: v0.19.8 assigns sanity against the stack top (ATC:792-800) and closes the dev task itself (ATC:835) | — | — | missing (in the target) | — | The target needs a closer. Suggested: the script closes the dev task as the phase lead and sends `fix-complete` to `<lead>` with "not_reproducible: close sanity <s> and qa <q> with `bd close ... --reason 'not_reproducible: <fix>'`". |
| A16/A17 (PR_LOOKUP_UNAVAILABLE / OFFLINE) | — | **differs**: any non-zero ls-remote, fetch or gh exit counts as unavailable, and OFFLINE trusts cached `refs/remotes/origin/*` (ATC:600-628) | dev `ok` JSON `github: unavailable`, `github_error` ATC:936-937 | no instruction | NONE (ok result; no action needed) | yes | — |
| A18 (other command failed) | COMMAND.FAILED | yes | dev JSON | "Repair {argv[:3]} and rerun with the same vars file." ATC:405-406 | VAGUE | yes | Include stderr plus a per-tool action, e.g. "`gh auth status`; `git remote -v`; then rerun with the same --vars/--result." |
| A18 (timeout) | COMMAND.FAILED | yes | dev JSON | "Retry once, or raise ATM_BD_GATE_TIMEOUT_S if the host is loaded, then rerun with the same vars file." ATC:437-438 | FIX | yes | — |
| A18 (auth or missing repo on ls-remote, fetch or gh) | COMMAND.FAILED | **differs**: the failure is swallowed as "unavailable" (ATC:604-606, 612-614, 646-650) and becomes HEAD.NOT_PUSHED "cached origin head differs" or VARS.EMPTY | dev JSON (misleading code) | — | NONE | yes | "COMMAND.FAILED: `git ls-remote origin` failed: <stderr>. Repair credentials (`gh auth status`, `ssh -T git@github.com`), then rerun with the same vars." |
| A18 (merge-base error) | COMMAND.FAILED | yes | dev JSON | "Repair the local git ancestry check." ATC:634-635 | VAGUE | yes | "`git merge-base --is-ancestor origin/{top} HEAD` failed: <stderr>; run `git fetch origin` and rerun." |
| A19 (origin names no owner/repo) | COMMAND.INVALID | yes | dev JSON | "Configure the repository origin." ATC:638-639 | VAGUE | yes | "Run `git remote set-url origin https://github.com/<owner>/<repo>.git` in {worktree}, then rerun." |
| A19 (gh invalid JSON) | COMMAND.INVALID | **differs**: swallowed as unavailable (ATC:646-650) | — | — | NONE | — | Refuse COMMAND.INVALID: "file an issue on randlee/atm-bd-orchestration with this JSON; rerun later." |
| A19 (bd/atm JSON shape or KeyError/ValueError) | COMMAND.INVALID | yes | dev JSON | "Repair {field} in the JSON returned by `{producer}` to be {expected}, then retry." ATC:266-268, 438-439, 942-954 | VAGUE | **no** (producer defect = hook maintainer; vars-file KeyError = lead) | "Producer defect in `{producer}`: send this JSON to `<lead>` (`atm send`); the dev cannot repair it." |
| A20 | HEAD.DIRTY | yes; **differs**: not run under `--skip-checks` (ATC:801-805) | dev JSON | "Commit or remove worktree changes before retrying." ATC:596 | FIX | yes | — |
| A21 | HEAD.NOT_PUSHED | yes; **differs**: OFFLINE checks the cached ref (ATC:626-628); skipped under `--skip-checks` | dev JSON | "Push this branch and retry." ATC:609-610; "Push this branch and refresh origin before retrying." ATC:626-628 | FIX | yes | — |
| A22 | BASE.NOT_ANCESTOR | yes; **differs**: also runs on cached refs offline | dev JSON | "Rebase onto origin/{top}, push, then rerun the hook." ATC:632 | FIX | yes | — |
| A23 | PR.NOT_FOUND | yes | dev JSON | "Open the PR at the pushed head, then rerun the hook." ATC:653 | FIX | yes | Optionally name `gh pr create --base <pr_target> --head <branch>`. |
| A24 | PR.AMBIGUOUS | yes | dev JSON | "Close the duplicate PRs so exactly one open PR names this branch and pushed head, then rerun the hook." ATC:655-656 | FIX | yes | — |
| A25 | BEAD.NO_SANITY | yes | dev JSON | "Ask {lead} to create or correct the group sanity bead." ATC:788 | FIX (relay; the lead is named) | no (relayed by the dev) | Name the command: "`atm send {lead} 'BEAD.NO_SANITY {bead}: ...'`". |
| A26 | BEAD.INVALID | yes | dev JSON | "{bead}" / "Supply an existing unique bead id." ATC:445 | VAGUE | **no** | "Bead {id} named in the lead's vars does not exist: send this JSON to `<lead>`." |
| A27 | BEADS_DIR.INVALID | yes | dev JSON | "Set BEADS_DIR or repair `bd where --json` to return an absolute beads path." ATC:461 | FIX | no (target: the phase lead) | Acceptable, but add "or send this JSON to `<lead>`". |
| A28 (derived commit/PR empty) | VARS.EMPTY | yes; **differs**: v0.19.8 refuses an empty PR offline, while the target (GAP-011 b) proceeds | dev JSON | "Push a non-empty HEAD and open exactly one PR; when GitHub is unavailable or checks are skipped, record its pr_number and pr_url on the checked bead before retrying." ATC:772-773 | VAGUE (the offline branch: the dev has no command and must not write beads) | partly | (dev) "GitHub is unreachable and {bead} records no PR: rerun when `gh api` answers, or send this JSON to `<lead>`, who records the PR with `bd update {bead} --set-metadata pr_number=<n>`." |
| A28 (pr_target empty / STACK.TOP) | VARS.EMPTY / STACK.TOP | yes (STACK.TOP is deleted in the target) | dev JSON | "Set pr_target to the checked layer's lower-bound branch." ATC:579-581; "Fetch and verify origin/{branch} before retrying." ATC:583-584 | VAGUE | **no** (the lead's vars) | "pr_target is empty in the lead's vars: send this JSON to `<lead>`." |
| A30 | REPLAY.CONFLICT | yes | dev JSON | "sanity is closed at a different sha" / "Request a new sanity assignment before changing the checked sha." ATC:822-823 | VAGUE (from whom? how?) | **no** | (dev) "Sanity {s} already passed at {old}; send `atm send <lead> 'REPLAY.CONFLICT {task}: HEAD {new} needs a new sanity assignment'` and stop." |
| A30-adjacent (no assignment event) | REPLAY.CONFLICT | yes | dev JSON | "{task} has no assignment event" / "Assign the tracked task before closing it." ATC:259 | VAGUE | **no** (only the lead assigns) | "Task {t} was never assigned: ask `<lead>` to run `atm task assign <you> --task-id {t} ...`." |
| A34 | ASSIGNMENT.TOO_LARGE | yes | dev JSON | "Shorten {field} in --result (pass the stack view with --compact-json, or cite the full view by file path), then rerun with the same vars file; nothing was closed." ATC:700-703 | FIX | yes | — |
| A36 | BEADS.WRITE_PENDING | yes | dev JSON | "Rerun with the same vars file once bd answers; the journaled write is applied once (the bd-cache cron also applies it within a tick)." ATC:521-524 | FIX | yes | — |
| A36 (bd-cache error passed through) | BD_CACHE.* | yes | dev JSON | the bd-cache fix verbatim, e.g. "transient: retried by the next run" BDC:315, "upgrade bd_cache, or move {path} aside to rebuild it from bd" BDC:285, "move {path} aside; it was built from a different dolt database" BDC:294 (passed through at ATC:491-492, 513-514) | VAGUE (a transient error does not tell the dev to rerun; schema and wrong-db errors are not the dev's) | partly | Transient: "rerun with the same --vars/--result." Schema/DB: "send this JSON to `<lead>`." |
| A37 | BEADS.WRITE_ESCALATED | yes | dev JSON, **and** CRON → the phase lead | dev: "Ask the phase lead to set {bead} in bd and run `bd_cache.py resolve {seq} --note <what> --actor <lead>`, then rerun with the same vars file." ATC:518-520; lead: "...set the intended state with bd, then clear the entry: bd_cache.py resolve {seq} --note <what you did> --actor <you>." CRON:78-80 | FIX | yes | — |
| A38 (atm send / atm task assign fails after the bead close) | COMMAND.FAILED | yes | dev JSON | "Repair atm send and rerun with the same vars file." ATC:405-406 | VAGUE | yes | "The bead is closed; the sanity assign failed: <stderr>. Rerun with the same vars (the assign is idempotent), or atm-bd assigns it from `bd ready`." |

## Diagram B: sanity close hook

| edge | error code | code @v0.19.8 | receiver | message (quoted, file:line) | class | right owner | recommended message (recipient) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| B3 (`refused` close, OPEN GAP-020) | — | **differs**: the hook never reads `task_op.outcome`. A refused close by a sanity agent runs `prepare()` and is refused as SANITY_CLOSE.INVALID ("message_source: use --template sanity-task-close.md.j2..." HOOK:265, or "reviewer: must be sanity-selected" HOOK:293), and an escalation goes to the lead | dev-sanity (refused reply) + lead (HCE) | the message describes a malformed result, not "refusal not supported" | NONE (misleading) | no | Route `refused` to pass-through, or refuse with "SANITY_CLOSE.REFUSE_UNSUPPORTED: send the blocker to `<lead>` with `atm send`; do not close the task." (dev-sanity) |
| B5 | SANITY_CLOSE.BUSY | yes | dev-sanity reply + lead escalation | "another close owns this task; retry after it completes" HOOK:86 | FIX (for the caller) | yes for the caller; the lead escalation is noise, and its recovery says "Phase lead: correct..." HOOK:219 | Do not escalate BUSY; reply "retry `atm task close {task} completed --template ... --vars <same>` in a few seconds." |
| B7 | SANITY_CLOSE.INVALID (owner sanity) | yes; **missing** sub-check "PASS with no commit selected and none recorded": commit defaults to "unspecified" (HOOK:291) | dev-sanity reply + lead escalation | "{field}: {detail}", e.g. "task_id: must match the closing sanity task" HOOK:277, "reviewer: must be sanity-selected" HOOK:293, "findings: FAIL must identify missing items or lint failure" HOOK:312, "caller: only assigned sanity agent may close" HOOK:317 | FIX (field and expected value) | yes, but the `recovery` addresses the phase lead (HOOK:219-221) | Owner-specific recovery: "dev-sanity: correct {field} in <selected-vars.json> and rerun the same `atm task close`." |
| B8 | SANITY_CLOSE.INVALID (owner phase-lead) | yes | lead (HCE escalation task), dev-sanity reply | e.g. "metadata.dev_bead: required on sanity bead" HOOK:280; "dependencies: sanity must be blocked by dev bead" HOOK:285; "checked_bead.status: PASS requires completed dev work; reopened prerequisites must be resolved by the developer" HOOK:303-304; "dev assignee: bead and task assignees differ" HOOK:323; "result: conflicts with an already prepared close; lead must reconcile" HOOK:328 | VAGUE (no command) | yes | Per check: e.g. "`bd dep add {sanity} {dev} --type blocks`, then tell `<caller>` to rerun its close"; "`bd update {dev} --assignee {dev_agent}`"; "reconcile: inspect `.atm-bd/sanity-close/journal.sqlite3` key {key}". |
| B9 | SANITY_CLOSE.LEGACY_VERDICT | yes | dev-sanity reply + lead escalation | "CANNOT_RUN is not an accepted verdict. Missing deliverables or missing/stale evidence are FAIL. ... escalate the exact conflicting records or failed operation and error to the phase lead." HOOK:297-301 | FIX (for sanity) | **no**: coded owner `phase-lead` (HOOK:301); target owner is sanity | Owner `sanity`: "Use FAIL with findings, or refuse the task; rerun the close." |
| B10 | SANITY_CLOSE.CONFIG | yes | the escalation goes to the placeholder `phase-lead` (HOOK:213-215). Its `atm task assign self.lead()` raises again → NOTIFY_FAILED (HOOK:251-255) | dev-sanity reply only; doctor | "phase lead unavailable: {error}" HOOK:186 | silent (to anyone who can act) | no | (dev-sanity) "SANITY_CLOSE.CONFIG: .atm-bd/phase-<x>.toml has no lead. Send this JSON to `team-lead` (`atm send team-lead --stdin`) and wait." The hook should also try `team-lead`. |
| B11 | SANITY_CLOSE.COMMAND | yes | dev-sanity reply + lead escalation | "hook work budget exhausted; retry the close" HOOK:120,131; otherwise the raw stderr of the failed command HOOK:126,129 | VAGUE | yes | "`{command}` failed: <stderr>. Lead: repair <tool>, then tell `<caller>` to rerun the same close." Budget case: "caller: rerun the same close." |
| B12 | SANITY_CLOSE.DATA | yes | lead escalation | "invalid JSON from {command}: {error}" HOOK:137 | NONE | yes (target: the phase lead) | "Producer `{command}` returned invalid JSON: check `bd version`/`atm --version` against the pinned versions; file an issue on randlee/atm-bd-orchestration." |
| B13 | BD_CACHE.* | yes | lead escalation | "{detail}; {fix}", e.g. "...; transient backend outage: the drain retries; a hook escalates or defers" HOOK:144,155,371 + BDC:309-310 | VAGUE | yes | "bd is unreachable: restore it (`bd version`, `bd list -n 1 --json`), then tell `<caller>` to rerun the same close." |
| B14 | ASSIGN_VARS.CONFLICT | **missing** in the hook (it never assigns QA) | — | — | missing | — | (lead, escalation) "qa {q} branch {a} ≠ checked {b}: `bd update {q} --set-metadata branch={b}`, then assign QA." |
| B15 | ASSIGN_VARS.NOT_PLANNED | **missing** in the hook | — | — | missing | — | (lead) "qa {q} lacks {field}: run `close_fields_backfill.py --phase <x> --apply`, then assign QA." |
| B16 | SANITY_CLOSE.INTERNAL | yes | lead escalation (owner `hook-maintainer`) | "{Type}: {error}" HOOK:431, 465 | NONE | **no** (the maintainer owns it; the lead receives it) | (lead) "Hook defect: file an issue on randlee/atm-bd-orchestration with this entry; disable with `atm hook disable --phase <x> task.close` if it repeats." |
| B16a (OPEN GAP-005) | — | **missing**; v0.19.8 links after the close | — | — | missing | — | Needs a code, e.g. "SANITY_CLOSE.NOT_STACKED: run `sanity-stack-link --input <stack-input.json>` and close after STACKED." (dev-sanity) |
| B17 NOTIFIED | — | yes | lead: escalation task HCE | "Resolve sanity close failure for {original_task}. ... Route the concrete correction to the named repair owner ... Retry the original close or its existing repair obligation after the cause is corrected." HCE:10-21; recovery "Phase lead: correct the reported data/operation, then retry the same native task close." HOOK:219-221 | VAGUE (generic). The lead **cannot** "retry the same native task close": only the assigned sanity agent may close it (HOOK:317) | yes | Per-code recovery naming the command, ending "then tell `<caller>` to rerun `atm task close {task} completed --template sanity-task-close.md.j2 --vars <same>`." |
| B18 ALREADY_NOTIFIED | — | yes | nobody | the reply only; nothing is resent (HOOK:241-243) | silent | — | If the prior escalation task is terminal, escalate again (the cause recurred after it was closed). |
| B19 NOTIFY_FAILED | — | yes | dev-sanity reply (with `escalation_error`); `atm doctor` only | doctor: "Lead: inspect the journal and deliver the pending escalation." HOOK:445-447 | silent (visible only to someone who runs doctor) | no | (dev-sanity reply) "The escalation to `<lead>` failed: send this JSON yourself with `atm send <lead> --stdin` and wait." |
| B20 UNPERSISTABLE | ATM_HOOK_EXIT | yes | the daemon or hook stderr; the caller sees ATM_HOOK_EXIT | "SANITY_CLOSE.INTERNAL: {error}; escalation_to=phase-lead; could not persist/send escalation; caller must notify phase lead" HOOK:469-470 | VAGUE (stderr only; names no lead or command) | partly | Print JSON on stdout naming the lead: "send this to `<lead>` with `atm send`." |
| B22 FAIL | — | yes; **differs**: it sends SFT, not `sanity-fail.xml.j2` (no round or max), and the dev task was **already closed** by ATC:835 | dev (ATM send as the caller, HOOK:353-354) | "Fix these missing items, then complete your dev/fix workflow again: - Deliverable N: <issue>" SFT:14-16 | FIX | yes. But dev-template step g says "A sanity-fail ... message means the lead re-assigns this task", and the lead is never told → the dev waits | (dev) "Your task {t} is reopened/still open: fix the items, push, rerun `atm-task-close.py --role <r> --vars <same> --result <new>`." Keep the dev task open (target A38). |
| B23/B29 ROUND_CAP | SANITY.ROUND_CAP | **missing** in the hook (only the deleted legacy role, ATC:874-880) | — | — | missing | — | (phase lead) "SANITY.ROUND_CAP {sanity} {sha} undone=…: round {n} > {max}; decide: re-scope, reassign, or close." |
| B25/B27 HELD and QA assign | HELD:ASSIGN_VARS.NOT_RECORDED | **missing** (no QA assign or notice in the hook) | nobody | — | missing | — | (lead) "Sanity PASS {s}: QA {q} is ready; `close-vars.py {q} --qa-assignment --out v.json` then the printed assign." |
| B30 SPAWN_FAILED | — | yes | stderr only; the cron drains it | `{"drain_spawn_error": ..., "fallback": "bd-cache cron drains the journal"}` HOOK:389-392 | NONE (self-healing; no action needed) | yes | — |

## Diagram C: sanity PASS link

v0.19.8 closes first and links afterwards (dev-sanity-template step e; dev-sanity.md:140-155).

| edge | error code | code @v0.19.8 | receiver | message (quoted, file:line) | class | right owner | recommended message (recipient) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| C4/C10 (inline failed / stack_failed, OPEN) | stack_failed | yes (after the close) | dev-sanity exit 1 + JSON; ATM notice to the lead | "Sanity task already closed; stack handoff needs lead action: {json}" SSL:336 | VAGUE | open | (lead) "stack_failed for {pr}: <evidence>. Run `sanity-stack-link --input <file>` after fixing <cause>." |
| C6 REBASE_REQUIRED | rebase_required | **differs**: it goes to the lead, not to the dev. Nothing sends `rebase-required.xml.j2` | lead (SSL:251-252, 336) | "Sanity task already closed; stack handoff needs lead action: {...rebase_required...}" | VAGUE | **no** (target: the dev, on its open task) | (dev) rebase-required.xml.j2: "Rebase {branch} onto {top} at {head}. Push and run the close hook again on the same task." |
| C7 (OPEN GAP-005) | — | **missing**: the sanity bead is already closed PASS on an unlinked layer | — | — | missing | — | — |
| C8 LOCK_TIMEOUT | lock_timeout | **differs**: it goes to the lead; the target says dev-sanity retries and never escalates | lead (SSL:94, 336) | as above, with holder and claimed_at | VAGUE | **no** | (dev-sanity, in the link result) "The stack bead is held by {holder} since {t}: `atm send {holder}` once, do other sanity work, then rerun `sanity-stack-link`." |
| C9 RETRY | — | **missing** (dev-sanity.md:151-155: "leave the sanity task closed") | — | — | missing | — | — |

## Diagram D: QA close (`--role qa`)

| edge | error code | code @v0.19.8 | receiver | message (quoted, file:line) | class | right owner | recommended message (recipient) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D1 | ARGS.INVALID | **missing** (the flag is ignored) | — | — | missing | — | (quality-mgr) "--skip-checks is not valid with --role qa; rerun without it." |
| D2 | CONFIG.INVALID | yes | quality-mgr JSON | as A2 (ATC:192-199, 207-217) | FIX (for the lead or installer) | **no** | "Send this JSON to `<lead>` (`atm send`); do not edit config." |
| D3 | VARS.MISSING | yes | quality-mgr JSON | "Supply every shared template variable in the lead's vars file." ATC:314 | VAGUE | **no** | "Regenerate with `close-vars.py <qa> --out <vars>`; if the fields are still missing, send this JSON to `<lead>`." |
| D4 | VARS.UNKNOWN | yes | quality-mgr JSON | "Remove variables no shared template declares." ATC:316 | FIX | yes | — |
| D5 (bad verdict) | VARS.INVALID | yes | quality-mgr JSON | "verdict must be PASS, FAIL or CANNOT_RUN" / "Supply the selected review verdict." ATC:334-335 | FIX | yes | Drop CANNOT_RUN from the text. |
| D5 (CANNOT_RUN) | VARS.INVALID | yes | quality-mgr JSON | "Use the QA refusal workflow." ATC:338-339 | VAGUE | yes | "Run `atm task close {qa} refused --template task-refused.md.j2 --vars <v>`; do not use this script." |
| D5 (render / task id ≠ bead id) | VARS.INVALID | yes | quality-mgr JSON | "Supply vars compatible with {template}." ATC:675; "Use bead ids as the corresponding task ids." ATC:351 | VAGUE | yes | Name the failing field and regenerate with close-vars.py. |
| D6 | VARS.EMPTY | yes | quality-mgr JSON | "Populate {name} in the shared vars file." ATC:329 | VAGUE | **no** | As D3. |
| D8 | REPLAY.CONFLICT | yes | quality-mgr JSON | "recorded verdict or sha differs" / "Use the original vars for a replay or start a new review assignment." ATC:568-569 | VAGUE (only the lead assigns) | yes | "{qa} is already closed '{recorded}': rerun with those vars, or ask `<lead>` for a new QA assignment (`atm send`)." |
| D11 | BEADS.WRITE_PENDING | yes | quality-mgr JSON | ATC:521-524 (as A36) | FIX | yes | — |
| D12 | BEADS.WRITE_ESCALATED | yes | quality-mgr JSON + CRON → lead | ATC:518-520; CRON:78-80 | FIX | yes | — |
| D14 (QA FAIL notice) | — | yes | the phase lead (`atm send` as the caller; skipped when the qa task is terminal or the caller is the lead) | "QA FAIL {qa} commit={c} round={r}" ATC:899-900 | VAGUE (status only) | yes | "QA FAIL {qa}: assign the poured fix beads from `bd ready` (`atm task assign <dev> --task-id <fix> --template fix-assignment.xml.j2 ...`)." |

## Diagram E: close vars

| edge | error code | code @v0.19.8 | receiver | message (quoted, file:line) | class | right owner | recommended message (recipient) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E1 | CLOSE_VARS.NOT_POURED | yes | the caller of close-vars.py (JSON, exit 2) | "The pour or backfill did not stamp {id}: ask the phase lead to run close_fields_backfill.py for its phase; never assemble these fields from other beads." CF:66-68; "Ask the phase lead to run close_fields_backfill.py for its phase." CV:141-142 | FIX (no args) | yes (the lead, relayed when someone else runs it) | Name the command: "`close_fields_backfill.py --phase <x> --apply`". |
| E4 | ASSIGN_VARS.NOT_RECORDED | **differs**: a refusal, not a hold | the caller of `close-vars.py --qa-assignment` | "{id} has no recorded close: finish its dev or fix close (atm-task-close.py records commit, branch and pr_number), then rebuild these vars." CF:104-106 | VAGUE (misleading: a not_reproducible fix close **is** finished but records `pr_number=""`, ATC:768-769, 826) | yes | "{id} closed without a PR (offline or not_reproducible): record the PR with `bd update {id} --set-metadata pr_number=<n>`, or close {qa} as not_reproducible." |
| E5 | ASSIGN_VARS.CONFLICT | yes | close-vars caller | "Correct the stale value on {qa} to the checked bead's recorded close." CF:110-111 | VAGUE (no command) | yes | "`bd update {qa} --set-metadata {name}={checked value}`, then rebuild the vars." |
| E6 | ASSIGN_VARS.NOT_PLANNED | yes | close-vars caller | "Set {name} on {qa} (layer when its PR is linked, round and reviewers at pour)." CF:114-115 | VAGUE (no command) | yes | "Run `close_fields_backfill.py --phase <x> --apply` (round, reviewers_round1); layer is set when sanity-stack links the PR." |

Close-vars refusals outside the target edges (not counted): CLOSE_VARS.READ_FAILED (CV:62-63, 78-88, FIX: "`bd version`");
EVENTS_FAILED (CV:96-105, FIX); NOT_STARTED (CV:107-108, FIX: "`atm task start {task} \"<plan>\"`"; the target deletes it);
ROLE_MISMATCH (CF:70-71, VAGUE: no command); NOT_A_TASK_BEAD (CF:54-56, FIX); CLOSE_VARS.CONFIG (CV:166-168, FIX).

## Summary

Rows: 88 failure, refusal, pending, escalation and cap edges and sub-edges.

| class | count |
| --- | --- |
| FIX | 32 |
| VAGUE | 36 |
| NONE | 7 (2 of them, A16/A17 and B30, need no action) |
| silent | 3 |
| missing in code | 10 |
| wrong owner (the receiver is not the target owner, any class) | 20 |

Structural findings:

1. **atm-task-close.py notifies no one on refusal.** Every lead-owned refusal goes only to the dev's or quality-mgr's terminal:
   A2, A5, A6, A26, A28 (pr_target), A30, D2, D3 and D6. Most fix texts say "in the lead's vars file" but never say "send this
   to `<lead>`" or name `atm send`. The dev template's generic "ask the lead to supply the correct file" is the only bridge.
2. **The hook's `recovery` text is the same for every refusal and always addresses the phase lead** (HOOK:219-221). It tells the
   lead to "retry the same native task close", which the lead cannot do: only the assigned sanity agent may close (HOOK:317).
3. **v0.19.8 still closes the dev task at dev complete** (ATC:835). So FAIL, rebase_required and lock_timeout all arrive after the
   dev's task is gone, and the dev prompt says to wait for the lead's re-assignment.

### Edges where an agent is left stuck

- **B3:** dev-sanity closing `refused` (as dev-sanity.md:33 says) is rejected by the hook as SANITY_CLOSE.INVALID with a misleading field error. dev-sanity cannot refuse.
- **B10 → B19:** a phase toml with no `lead` makes the escalation unsendable. Only `atm doctor` shows it, and the caller is not told to notify anyone.
- **B19:** any failed escalation assign is visible only through `atm doctor` ("Lead: inspect the journal...").
- **B18:** a cause that recurs after its escalation task was closed is never re-escalated.
- **B20:** stderr-only "caller must notify phase lead". It names no lead and no command.
- **B22:** the FAIL message tells the dev to redo its workflow. Its task is closed, its prompt says to wait for the lead's re-assignment, and the lead is never told.
- **B23/B29:** there is no round cap in the hook, so FAIL can loop without limit and no cap message is ever sent.
- **B25/B27 (and B26 QA assign):** the hook never assigns QA and tells no one that QA is ready.
- **C6:** rebase_required goes to the lead as "needs lead action". The dev never receives `rebase-required.xml.j2`, and the sanity bead is already closed PASS.
- **C8:** lock_timeout goes to the lead. Nobody retries the link (C9 is missing).
- **C10:** stack_failed gets only a vague "needs lead action" notice.
- **A2 (repo config, dev_sanity_member, phase lead), A5, A6, A26, A28 pr_target, A30, A30 no-assignment, D2, D3, D6:** lead-owned refusals reach only the dev or quality-mgr's stdout. The lead is never told.
- **A18 auth / A19 gh JSON:** these failures are masked as "offline" and come back as a misleading HEAD.NOT_PUSHED or VARS.EMPTY.
- **E4:** a not_reproducible fix (recorded `pr_number=""`) gets "finish its dev or fix close" although that close is done, so QA can never be built.
- **A11 (target, OPEN GAP-006):** no closer exists for the dev task after a not_reproducible close.
- **A1 / D1:** `--skip-checks` with qa is silently ignored. This one does not leave an agent stuck.

Assumption not verified: that ATM shows the hook's refused `message` to the caller of `atm task close`. If it does not, B5, B7 and B9 also become silent for dev-sanity.
