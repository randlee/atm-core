# `atm clear`

CLI ownership for `atm clear`:

- clear-mode flag parsing
- caller-team resolution plus owner-only caller-identity enforcement
- conversion into `atm-core` clear requests
- dry-run rendering
- human-readable output
- JSON output

Clear eligibility remains owned by `atm-core`.

References:

- Product requirements: `docs/requirements.md` §9
- `REQ-P-CLEAR-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/clear.md`
