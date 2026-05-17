# Phase Z CLI JSON I/O Audit

Status:
- planned

Purpose:
- audit the current ATM CLI JSON surface before executable smoke and dogfood
- distinguish stable agent-facing JSON contracts from internal JSON shapes
- identify the minimum safe JSON-input expansion path, if any

## Questions This Audit Must Answer

1. Which retained ATM commands already support `--json` output?
2. For each such command, what exact DTO/outcome shape is currently emitted?
3. Which retained commands still lack JSON output entirely?
4. Which commands currently accept only positional text, `--file`, or
   `--stdin` input?
5. Which commands, if any, should gain JSON input first?
6. Is `atm send` the correct first JSON-input command, or does the code and
   requirements analysis show a better first candidate?
7. Which internal JSON structures must remain internal and not be treated as
   public CLI contracts?
8. Which docs are stale about current JSON support?

## Required Inputs

- `crates/atm/src/commands/`
- `crates/atm/src/output.rs`
- `docs/requirements.md`
- `docs/atm/commands/`
- `docs/plan-phase-Y.md`
- `docs/plan-phase-Z.md`

## Required Outputs

- command-by-command table:
  - command
  - JSON output supported: yes/no
  - JSON input supported: yes/no
  - current input modes
  - current output DTO
  - public-contract confidence
  - recommended action
- stale-doc findings list
- recommended sequencing for any future JSON-input work
- explicit recommendation on whether the work belongs:
  - before `Z.1`
  - after `Z.1`
  - after `Phase Z`

## Constraints

- do not equate internal serde support with approved public CLI JSON contract
- do not propose broad JSON-input rollout without explicit DTO and validation
  design
- prefer agent-safe structured I/O, but only where the public contract can be
  made explicit and testable

## Expected Deliverable

One audit report that `team-lead`, `quality-mgr`, and the implementation owner
can use to decide whether JSON I/O changes should happen before smoke testing
or be deferred.
