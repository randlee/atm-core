# Fixtures proposed to Rand: ANC-055..059

Rand ruling C (2026-11-01, fixture report rulings): one pass and one fail fixture each, pass fixtures citing a
command and its output. Not yet in anchors.json; each pair landed on plan and dev gates in 2 of 2 Jev runs
against a copy of anchors.json. Delete this file once Rand has locked or rejected them.

## ANC-055
- Pass: grep -rn 'close-vars\|close_vars' skills/atm-bd-lite prints nothing, and grep of the lite hooks and templates for open(, write_text and read_text finds no per-task state file.
- Fail: The lite dev template has the dev copy close-vars.json and fill in its commit and PR, and the lite close hook reads that file.

## ANC-056
- Pass: uv run pytest skills/atm-bd-lite/tests/test_hook_facts.py passes: each hook is given only the task id in its ATM request plus fakes for beads-cache, git, the provider and the phase file, and looks up every other fact from them.
- Fail: The lite close hook reads the PR number and branch head from a JSON file the dispatch template wrote at assignment.

## ANC-057
- Pass: grep -c 'atm task close' prints 1 for each lite dev and fix template, that command sends the report with --stdin, and grep finds no 'atm send' or 'atm ack' in them.
- Fail: The lite dev closes its task with atm task close and then sends its report to the lead in a second atm send.

## ANC-058
- Pass: uv run pytest skills/atm-bd-lite/tests/test_errors.py passes: for every Errors row, a failed lookup makes the hook return HookReply.refused with that row's fix command, and no test supplies stored state.
- Fail: When the PR lookup fails, the lite close hook uses the PR number saved at dispatch time instead.

## ANC-059
- Pass: find skills/atm-bd-lite -name '*repair*' -o -name '*backfill*' -o -name '*legacy*' prints nothing.
- Fail: skills/atm-bd-lite/scripts/backfill-close-vars.py rebuilds close-vars files for tasks dispatched before lite existed.
