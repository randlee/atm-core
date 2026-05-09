# `atm read`

CLI ownership for `atm read`:

- single-message selection flag parsing
- shared queue-filter parsing aligned with `atm list`
- deprecated legacy read-flag alias handling and warning presentation
- timeout flag parsing
- conversion into `atm-core` read requests
- human-readable full-message rendering
- JSON output for one selected message plus match metadata
- exact-message retrieval help text for ATM-authored JSONL retrieval stubs

Workflow/state behavior remains owned by `atm-core`.

References:

- Product requirements: `docs/requirements.md` §7 and `read-behavior.md`
- `REQ-P-READ-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/read.md`
