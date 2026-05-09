# `atm list`

CLI ownership for `atm list`:

- bounded metadata-search flag parsing
- shared queue-filter parsing aligned with `atm read`
- conversion into `atm-core` list/query requests
- human-readable metadata row rendering
- JSON metadata output

Workflow/state behavior remains owned by `atm-core`.

References:

- Product requirements: `docs/requirements.md` §7 and `read-behavior.md`
- `REQ-P-LIST-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/list.md`
