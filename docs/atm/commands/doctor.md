# `atm doctor`

CLI ownership for `atm doctor`:

- doctor-mode and output flag parsing
- conversion into `atm-core` doctor requests
- human-readable finding rendering
- JSON output

Diagnostic logic remains owned by `atm-core`.

References:

- Product requirements: `docs/requirements.md` §11
- `REQ-P-DOCTOR-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- `REQ-ATM-OBS-001`
- Product architecture: `docs/architecture.md`
- Core modules:
  - `docs/atm-core/modules/doctor.md`
  - `docs/atm-core/modules/observability.md`
