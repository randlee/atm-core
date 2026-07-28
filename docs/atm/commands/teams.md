# `atm teams`

CLI ownership for `atm teams`:

- subcommand and flag parsing for the retained local team recovery surface
- conversion into `atm-core` team recovery requests
- human-readable output
- JSON output

Core team discovery, roster mutation, and backup/restore behavior remains
owned by `atm-core`.

CLI note:
- `teams add-member --pane-id` accepts tmux pane ids in `%<number>` form or a
  bare numeric pane id that ATM canonicalizes to `%<number>`
- `teams update-member` is the accepted repair path for existing member
  metadata such as durable `home_dir`, the graft `--workspace-root`, harness,
  model, agent type, and tmux `--pane-id`; manual `config.json` edits are no
  longer the supported pane, home-dir, or graft endpoint repair workflow

References:

- Product requirements: `docs/requirements.md` §12
- `REQ-P-TEAMS-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/team_admin.md`
