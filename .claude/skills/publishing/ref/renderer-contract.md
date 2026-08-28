# Renderer Contract

- Direct agent template rendering uses the `sc-compose` CLI. Do not import
  `sc_compose` from an agent prompt or an inline Python command.
- Before a direct render, read the exact `SC_COMPOSE_VERSION` in
  `.github/scripts/bootstrap_sc_compose.py`; `sc-compose --version` must report
  that same version. Stop and report a sanitized version mismatch otherwise.
- Package Python code (`install.py`, integration examples, and tests) uses only
  the interpreter printed by `bootstrap_sc_compose.py`. That bootstrapper
  installs or replaces the wheel until it exactly matches
  `SC_COMPOSE_VERSION`.
- Homebrew and Scoop channel workflows bootstrap that same pinned renderer on
  the runner via `.github/actions/setup-renderer`. They must not extract or
  execute the consumer product binary as a template renderer.
