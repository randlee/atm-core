# `atm-core::doctor`

Owns local diagnostics, config/path/inbox checks, observability readiness
checks, and the finding model used by the CLI renderer.

For the Tokio/Axum daemon, `DoctorExecutionContext.peer_wire_security` is a
typed bootstrap-injected diagnostic status that serializes as the public launch
value (`mutual-tls` or `plaintext-test`). It is absent for client-only doctor
execution and must never expose certificate, pin, key, or peer-record data.

It must not own:

- clap parsing
- terminal grouping/formatting
- process exit mapping

References:

- Product requirements: `docs/requirements.md` §11 and §15
- `REQ-P-DOCTOR-001`
- `REQ-P-OBS-001`
- `REQ-CORE-CONFIG-001` for obsolete `[atm].identity` configuration drift
  detection and `[atm].team_members` baseline-roster checks
- `REQ-CORE-DOCTOR-001`
- CLI surface: `docs/atm/commands/doctor.md`
- Supporting boundary: `docs/atm-core/modules/observability.md`
- Integration design:
  `docs/atm-core/design/sc-observability-integration.md`
