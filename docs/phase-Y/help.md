# Phase Y Help Command Design Note

Purpose:

- capture the sprint-scoped design/ownership note for `atm help`
- bridge the product/sprint requirement in `Y.1` to the crate-local command
  doc in `docs/atm/commands/help.md`

Scope:

- `atm help` is the approved additive CLI feature for `Y.1`
- `atm --help` remains clap-generated syntax help
- `atm help` is ATM-owned conceptual/product help
- `atm help <subcommand>` must begin with the authoritative clap `--help`
  output for that subcommand
- `atm help <topic> --json` is allowed because it extends the existing public
  JSON-output pattern to the new help command
- structured JSON input is not part of `Y.1`, `Phase Y`, or `Phase Z`

Required first-delivery topics:

- tier 1:
  - `config`
  - `errors`
- tier 2:
  - `hooks`
  - `identity`
  - `skills`

Ownership split:

- sprint/product design and scope:
  - `docs/phase-Y/sprint-Y1.md`
  - `docs/requirements.md`
- crate-local command ownership:
  - `docs/atm/commands/help.md`
- crate-local CLI architecture/requirements:
  - `docs/atm/requirements.md`
  - `docs/atm/architecture.md`

References:

- `GH #83`
- `docs/phase-Y/sprint-Y1.md`
- `docs/atm/commands/help.md`
