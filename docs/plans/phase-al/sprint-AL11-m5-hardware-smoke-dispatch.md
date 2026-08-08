# AL.11 — M5 (macOS) Hardware Smoke-Dispatch Instructions

**recommended_agent:** team-lead (doc-only, no ATM identity on receiving end).
**must_follow:** AL.9 (PR #779) — dispatch instructions reference its build/branch.
**unblocks:** M5 hardware team executing AL.9 smoke validation.
**parallel_safe:** yes, independent of AL.12 (cwin).

## Deliverables

1. Write `docs/plans/phase-al/AL9-hardware-smoke-dispatch-m5.md`: instructions
   for the M5 macOS hardware team (no ATM identity, cannot receive task
   assignments) to run the exact peer-pair procedure AL.9 deliverable 1 already
   specifies for proving M5 direct cross-host write, per
   `docs/peer-pair-smoke.md`'s progressive live-daemon commands, against the
   AL.9 build/branch (PR #779) and its designated peer host `fastpc4.rz.local`
   (operated by the cwin team, AL.12):

   ```bash
   just smoke localhost
   just smoke local-ip
   just smoke peer-preflight m5 fastpc4
   just smoke crosshost-send m5 fastpc4
   just smoke crosshost-ack m5 fastpc4
   ```

   Each command must be run only after the prior one passes, in order — this
   is the same progressive ladder `docs/peer-pair-smoke.md` defines, not an
   independent smoke choice. The dispatch doc must instruct M5 to also run
   `atm doctor --json` and post the JSON plus generated XHTML evidence panes
   produced by the repository's `.claude/skills/smoke-test/SKILL.md` skill
   for every command, along with host/OS version, to PR #779.
   `peer-preflight` must be confirmed healthy before
   `crosshost-send`/`crosshost-ack` are attempted — a failure at any step
   blocks the later steps and must be posted as-is, not retried silently.
