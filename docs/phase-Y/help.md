# Phase Y Help Command Design Note

Purpose:

- capture the sprint-scoped design/ownership note for `atm help`
- bridge the product/sprint requirement in `Y.1` to the crate-local command
  doc in `docs/atm/commands/help.md`

Scope:

- `atm help` is the approved additive CLI feature for `Y.1`
- `Y.2` closes the intentionally deferred tier-2 topic examples without
  broadening the command surface
- `atm --help` remains clap-generated syntax help
- `atm help` is ATM-owned conceptual/product help
- `atm help <subcommand>` must begin with the authoritative clap `--help`
  output for that subcommand
- `atm help --list` and `atm help` overview output are part of the first
  delivery, not follow-up scope
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

Y.2 follow-up scope:

- replace the tier-2 placeholder notes with concrete operator examples
- keep the follow-up limited to help text and adjacent docs
- do not add JSON input, write-boundary refactors, or compatibility-format
  changes in this phase slice

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
- `docs/phase-Z/cli-json-io-audit.md`
