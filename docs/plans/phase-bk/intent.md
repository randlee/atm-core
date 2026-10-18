# Intent - phase-bk: atm task hooks and atm cron

## What this plan is for

atm-core work only (Rand, 2026-10-18: "phase-bj atm-bd-orch only, phase-bk
atm-core only"). An orchestration skill can replace the behavior of every
`atm task` subcommand with a hook it registers for one team and one phase,
and can schedule the same kind of hook on a cron string that survives a
daemon restart. Rulings are recorded verbatim on atm-core issue #1873; the
design is `design.md` in this directory.

## What success looks like

- Every `atm task` subcommand (`assign`, `start`, `close`, `move`, `list`,
  `events`) consults its registered hook, if any, with one JSON request
  schema that includes the caller, and acts on the hook's JSON reply:
  refused with a message, pass-through, or a list of un-hooked atm commands
  to run instead. Hooks are never invoked recursively. A missing hook means
  today's behavior.
- `atm hook register|disable|list` records hooks per team and phase;
  disabling at phase end restores normal behavior.
- `atm cron start|end|list` stores cron strings in the atm database, the
  daemon runs each due schedule's hook with the same contract, and schedules
  persist across a daemon restart without replaying missed ticks.
- `atm doctor` reports every registered hook module: its header sha
  (sc-sha, as templates) against the released sha, warning on drift, plus the
  module's own `doctor` output.
- Every hook call, cron tick and hook-returned action lands in the one
  structured log (ADR-067).

## What is deliberately out of scope

- The python hook module itself, `atm-task-close.py`, the stack bead and the
  console tabs: phase bj in atm-bd-orchestration.
- Hooks on commands other than `atm task` and `atm cron`.
- Cross-host hooks: a hook runs on the host whose daemon owns the team.
