# `atm help`

CLI ownership for `atm help`:

- conceptual-help topic parsing
- overview and `--list` rendering
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
- long-form operator guidance lives in the installed user-doc corpus under
  `<install-root>/share/doc/atm/`; help topics should point there rather than
  trying to inline the full manuals
- installed-doc lookup is derived from the resolved installed `atm` binary
  location as `../share/doc/atm/`; it must not use `ATM_HOME`
- the conceptual `identity` topic must reinforce the accepted Phase AD rule:
  `atm peek` / `atm list` are inspection-only surfaces, while mutating
  commands resolve only the actual caller and do not expose impersonation
  flags

First-delivery topic scope:

- tier 1:
  - `config`
  - `errors`
- tier 2:
  - `hooks`
  - `identity`
  - `skills`

Y.2 follow-up scope:

- replace the tier-2 placeholder text with concrete operator examples and
  troubleshooting guidance
- keep the command surface unchanged: no new flags, no JSON-input mode, and no
  compatibility-writer behavior changes

Output contract:

- human output distinguishes command-help vs concept-topic results clearly
- human topic output points to installed long-form docs when available
- JSON output identifies the target and result kind and includes the rendered
  help body
- JSON topic output carries the same installed-doc pointer information

JSON contract notes:

- `atm help <topic> --json` extends the existing CLI JSON-output pattern to
  the new help command
- retained commands already expose JSON output; `atm help` extends that
  established surface rather than introducing CLI JSON output for the first time
- general structured JSON input remains out of scope for `Phase Y` and
  `Phase Z`

References:

- Product requirements: `docs/requirements.md` §14
- `REQ-P-HELP-001`
- `REQ-P-USER-DOCS-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
