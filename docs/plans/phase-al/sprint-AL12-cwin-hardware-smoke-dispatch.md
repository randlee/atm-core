# AL.12 — cwin (Windows) Hardware Smoke-Dispatch Instructions

**recommended_agent:** team-lead (doc-only, no ATM identity on receiving end).
**must_follow:** AL.9 (PR #779) — dispatch instructions reference its build/branch.
**unblocks:** cwin hardware team executing AL.9 smoke validation.
**parallel_safe:** yes, independent of AL.11 (M5).

## Deliverables

1. Write `docs/plans/phase-al/AL9-hardware-smoke-dispatch-cwin.md`: instructions
   for the cwin Windows hardware team (no ATM identity, cannot receive task
   assignments) to run, on its designated Windows host `fastpc4.rz.local`,
   against the AL.9 build/branch (PR #779):

   1. The standard local smoke ladder established for cwin by AI.52 — build
      the branch's release CLI/daemon pair under an isolated ATM home and
      disposable test database, then run `just smoke`, `just smoke localhost`,
      `just smoke local-ip`, `just test`, and `atm doctor --json`. A local
      smoke failure must be fixed and revalidated before step 2.
   2. Confirm the daemon stays up and reachable as the peer target for M5's
      (AL.11) `docs/peer-pair-smoke.md` progressive ladder — `just smoke
      peer-preflight m5 fastpc4`, `just smoke crosshost-send m5 fastpc4`,
      `just smoke crosshost-ack m5 fastpc4` are run FROM the M5 side and
      require `fastpc4.rz.local`'s daemon to be healthy and SSH-reachable for
      the duration; cwin does not run those commands itself.

   The dispatch doc must instruct cwin to post its local-ladder JSON output
   and generated XHTML panes produced by the repository's
   `.claude/skills/smoke-test/SKILL.md` skill, plus `atm doctor --json` output
   and host/OS version, to PR #779. It must also confirm in the PR comment
   that its daemon window stayed up for M5's cross-host commands (AL.11) to
   complete against it.
