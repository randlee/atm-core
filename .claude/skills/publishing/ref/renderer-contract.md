# Renderer Contract

- Direct agent template rendering uses `atm compose` with `--template <path>`
  and `--vars <json>`. Do not invoke `sc-compose render` from an agent prompt
  or an inline Python command.
- Package Python code (`install.py`, integration examples, and tests) uses only
  the interpreter printed by `bootstrap_sc_compose.py`. That bootstrapper
  installs or replaces the wheel until it exactly matches
  `SC_COMPOSE_VERSION`.
- Homebrew and Scoop channel workflows bootstrap that same pinned renderer on
  the runner via `.github/actions/setup-renderer`. They must not extract or
  execute the consumer product binary as a template renderer.
