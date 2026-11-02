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

from atm_bd_types import AtmCli, CacheStore, CronCommand, CronReply, PhaseFilesCli, Refusal
from atm_bd_shared.qa_handoff import qa_handoff

PROFILE = "lite"  # policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)

if __name__ == "__main__":
    if isinstance(cmd := CronCommand.parse(sys.argv[1:]), Refusal):
        reply = CronReply.refused(cmd)
    elif isinstance(phase := PhaseFilesCli().load(cmd.phase_config_path(), PROFILE), Refusal):
        reply = CronReply.refused(phase)
    elif isinstance(beads := CacheStore.for_phase(cmd.phase), Refusal):
        reply = CronReply.refused(beads)
    elif isinstance(assigns := qa_handoff(cmd.phase, phase, beads), Refusal):
        reply = CronReply.refused(assigns)
    else:
        tasks = AtmCli(timeout_seconds=10)
        reply = CronReply.assigned([(assign, tasks.assign(assign)) for assign in assigns])
    print(reply.to_json())
