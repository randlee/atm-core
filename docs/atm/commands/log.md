# `atm log`

CLI ownership for `atm log`:

- filter flag parsing
- tail-mode parsing
- JSON output selection
- human-readable rendering of queried or tailed records

Generic log query/follow behavior remains owned by the observability-backed
`atm-core` log service.

Specific CLI-owned flags:

- `--tail`
- `--level`
- repeatable `--match key=value`
- `--since`
- `--limit`
- `--json`

`atm` must not implement ad hoc file parsing for `atm log`.

References:

- Product requirements: `docs/requirements.md` §10
- `REQ-P-LOG-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- `REQ-ATM-OBS-001`
- Product architecture: `docs/architecture.md`
- Core modules:
  - `docs/atm-core/modules/log.md`
  - `docs/atm-core/modules/observability.md`
