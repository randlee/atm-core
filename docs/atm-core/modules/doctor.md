# `atm-core::doctor`

Owns local diagnostics, config/path/inbox checks, observability readiness
checks, and the finding model used by the CLI renderer.

It must not own:

- clap parsing
- terminal grouping/formatting
- process exit mapping

References:

- Product requirements: `docs/requirements.md` §11 and §13
- `REQ-P-DOCTOR-001`
- `REQ-P-OBS-001`
- `REQ-CORE-DOCTOR-001`
- CLI surface: `docs/atm/commands/doctor.md`
- Supporting boundary: `docs/atm-core/modules/observability.md`
