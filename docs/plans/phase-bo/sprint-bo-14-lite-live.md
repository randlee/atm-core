# Sprint bo-14: lite templates, install, release, one live lite phase

This is the old bo-11 sprint, renumbered and unchanged. It depends on bo-10, bo-11, bo-12 and bo-13.

## Cron
The lite cron runs the shared `qa_handoff` for each lite phase and assigns what it returns. It is the safety net for a close whose returned actions never ran.

```python
#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0", "atm-bd-shared"]
# [tool.uv.sources]
# atm-bd-shared = { path = "../../shared" }  # .atm-bd/atm-bd-lite/scripts -> .atm-bd/shared
# ///
"""atm-bd-lite cron: hand ready QA beads to the QA member (safety net for a close whose actions never ran).

  atm-bd-lite-cron.py --phase P
"""
import sys

from atm_bd_types import AtmCli, BdCli, CronCommand, CronReply, PhaseFilesCli, Refusal
from atm_bd_shared.qa_handoff import qa_handoff

PROFILE = "lite"  # policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)

if __name__ == "__main__":
    if isinstance(cmd := CronCommand.parse(sys.argv[1:]), Refusal):
        reply = CronReply.refused(cmd)
    elif isinstance(phase := PhaseFilesCli().load(cmd.phase_config_path(), PROFILE), Refusal):
        reply = CronReply.refused(phase)
    elif isinstance(assigns := qa_handoff(cmd.phase, phase, BdCli(timeout_seconds=10)), Refusal):
        reply = CronReply.refused(assigns)
    else:
        tasks = AtmCli(timeout_seconds=10)
        reply = CronReply.assigned([(assign, tasks.assign(assign)) for assign in assigns])
    print(reply.to_json())
```
- [ ] A ready, unassigned QA bead is assigned by cron when no close did it. A QA bead the close hook already assigned is skipped.

## Acceptance
- [ ] A lite phase runs end to end from release assets with one agent (ANC-015). Steps: dev started, dev acquire/record/close, QA assigned by the close hook, QA FAIL and fix, fix acquire/record/close, QA PASS, and the stack landed.
- [ ] An orchestration phase file with no `profile` key runs unchanged, and orchestration's tests pass unedited (ANC-039).
- [ ] Every hook result of the run appears in the bo-13 audit log.

## Validation
- `just lint`, `just test`.
- The live run's `atm task events` and `bd show` output for every bead.
