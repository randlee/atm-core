# `atm send`

CLI ownership for `atm send`:

- positional and flag parsing
- caller-team resolution plus owner-only caller-identity enforcement
- conversion into `atm-core` send requests
- human-readable output
- JSON output

Core send behavior remains owned by `atm-core`.

References:

- Product requirements: `docs/requirements.md` §6
- `REQ-P-SEND-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- `REQ-CORE-CONFIG-002` for alias rewrite before canonical target resolution
- `REQ-CORE-SEND-002` for cross-team canonical `from` projection
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/send.md`
