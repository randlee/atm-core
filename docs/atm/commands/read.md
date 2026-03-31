# `atm read`

CLI ownership for `atm read`:

- selection flag parsing
- timeout flag parsing
- conversion into `atm-core` read requests
- human-readable queue rendering
- JSON output

Workflow/state behavior remains owned by `atm-core`.

References:

- Product requirements: `docs/requirements.md` §7 and `read-behavior.md`
- `REQ-P-READ-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/read.md`
