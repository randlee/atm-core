# Phase Z CLI JSON I/O Audit

Status:
- complete
- still current after `Phase Yd` authorization and `Phase Ye` closure because
  neither line changed the retained public CLI JSON output surface

Purpose:
- audit the current ATM CLI JSON surface before executable smoke and dogfood
- distinguish stable agent-facing JSON contracts from internal JSON shapes
- identify the minimum safe JSON-input expansion path, if any

## Findings

1. Retained ATM commands that already support `--json` output:
   - `send`
   - `list`
   - `read`
   - `ack`
   - `clear`
   - `log`
   - `doctor`
   - `teams`
   - `members`
2. No retained command currently lacks JSON output.
3. Normal command input is still text/flag oriented:
   - `atm send` accepts positional message text, `--file`, and `--stdin`
   - other retained commands accept flags/arguments only, not structured JSON
4. Structured JSON input is not implemented today.
5. Existing `--json` outputs are explicit public command DTOs; they should be
   treated as the stable agent-facing contract for the current line.
6. Internal daemon/protocol/storage serde shapes must not be treated as public
   CLI contracts automatically.
7. Stale documentation claiming missing JSON output is a documentation defect,
   not a product gap.

## Recommendations

- no Phase `Y` or `Phase Z` sprint should be spent retrofitting JSON output on
  the retained commands, because that work is already done
- `Y.1` remains scoped to `atm help` and adjacent UX/help wording
- `atm help <topic> --json` is acceptable because it extends the existing
  output pattern to a new command
- structured JSON input should be deferred until after `Phase Z`
- the first future JSON-input candidate is still most likely `atm send`, but
  only after:
  - explicit public DTO design
  - command-level validation rules
  - clear separation from internal message/envelope/store shapes

## Audit Inputs Used

- `crates/atm/src/commands/`
- `crates/atm/src/output.rs`
- `docs/requirements.md`
- `docs/atm/commands/`
- `docs/plans/phase-Y/plan-phase-Y.md`
- `docs/plans/phase-Z/plan-phase-Z.md`

## Contract Boundary Notes

- do not equate internal serde support with approved public CLI JSON contract
- do not propose broad JSON-input rollout without explicit DTO and validation
  design
- prefer agent-safe structured I/O, but only where the public contract can be
  made explicit and testable
