# `atm help`

CLI ownership for `atm help`:

- conceptual-help topic parsing
- `--list` parsing
- mapping subcommand targets into clap-generated `--help` rendering
- typed topic-registry dispatch for ATM-owned concept topics
- human-readable help rendering
- JSON help rendering

Concept/help policy:

- `atm --help` remains clap-generated syntax help
- `atm help` is a separate ATM-owned conceptual-help command
- `atm help <subcommand>` must start with the authoritative clap `--help`
  output for that subcommand
- ATM-owned prose may be appended after the clap output, but it must not
  duplicate or drift from flag/argument documentation

First-delivery topic scope:

- tier 1:
  - `config`
  - `errors`
- tier 2:
  - `hooks`
  - `identity`
  - `skills`

Output contract:

- human output distinguishes command-help vs concept-topic results clearly
- JSON output identifies the target and result kind and includes the rendered
  help body

JSON contract notes:

- `atm help <topic> --json` extends the existing CLI JSON-output pattern to
  the new help command
- general structured JSON input remains out of scope for `Phase Y` and
  `Phase Z`

References:

- Product requirements: `docs/requirements.md` §13.5
- `REQ-P-HELP-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
