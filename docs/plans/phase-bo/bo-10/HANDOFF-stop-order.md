# Message to fenix (paste as-is)

Stop planning. Ship bo-10.

1. Move the three bo-10 hook files out of sprint-bo-10-lite-hooks.md into design/lite-code/atm-bd-task-{assign,start}-hook.py and atm-bd-task-hook.py, replacing the old ones. Delete design/lite-code/atm-bd-lite-cron.py, shared/, and docs/state-transitions.md (superseded by hooks/*.md). Sprint docs link to the files; they do not embed code.
2. Open the bo-10 PR against atm-bd-orchestration: the three hooks, fakes-based tests (one per Transitions row, one per Errors row), and the gate run JSON for each hook. Dependency: the bo-9 stub as it stands.
3. Add these three anchors as proposed, exactly as written below. Rand locks them. Add nothing else to anchors.json.
4. bo-15 and ANC-075/076 work pauses until bo-14 has run one live lite phase. Plan it against a hook that has executed, not one that hasn't.
5. Report back with the PR link and nothing else.

## ANC-077 — Lite stays small
Statement: Total Python under skills/atm-bd-lite/ (hooks, cron policy, decision layer; excluding tests) must not exceed 600 lines.
Check: `find skills/atm-bd-lite -name '*.py' -not -path '*/tests/*' | xargs wc -l | tail -1` reports <= 600 (run by atm-bd-lite-gate.py).
Pass fixture: three hooks at 44/42/70 lines plus a 180-line cron policy: 336.
Fail fixture: a dogfooding fix adds a 300-line helper module under skills/atm-bd-lite/scripts/: 636.
Reason (Rand, 2026-11-01): atm-bd-orchestration is one agent more complicated than lite and is over 5k lines because every bug fix went into Python. The per-file ceiling (ANC-070) only moves growth into uncapped files.

## ANC-078 — Hooks import facts, not decisions
Statement: A lite hook must import only from atm_bd_types; every method the stub exposes to a hook returns data or a typed Refusal, never a verdict about what the hook should do.
Check: `ast` on each hook file: the only import module is atm_bd_types (run by atm-bd-lite-gate.py). The stub's public methods on Bead, HookRequest, Stack and PhaseFile are reviewed at bo-9 against this rule and frozen by ANC-034's regeneration diff.
Pass fixture: Stack.layer_for(branch) -> Layer | None; HookRequest.qa_pass() -> QaPass | Refusal.
Fail fixture: a hook imports from scripts/lite_helpers.py; or the stub adds Bead.should_close() -> bool.
Reason (Rand, 2026-11-01): the gate reads one file. A decision hidden in a helper is invisible to it; this closes that door.

## ANC-079 — Every bug fix is classified before it is written
Statement: A dogfooding fix task for lite must begin with a classification block — template bug / decision-row bug / spec bug / hook bug — with evidence; only "hook bug" may change a hook file, and only citing the hooks/*.md row it corrects.
Check: the lite fix task template (bo-11) requires the block; the lead's instructions refuse to assign a fix task without it; a hook change whose PR body lacks a cited row fails the gate (atm-bd-lite-gate.py reads the PR body for `row: T-...`).
Pass fixture: "Classification: template bug. Evidence: the stack block's rebase step omits --onto; no hook refused wrongly." Fix lands in the dev template.
Fail fixture: a fix PR that edits atm-bd-task-hook.py with no classification and no row cited.
Reason (Rand, 2026-11-01): "when bug fixes land the agents just start writing spaghetti code to fix the issues. That is what I want stopped." The question "where does this fix go" has to be asked before any Python is written.
