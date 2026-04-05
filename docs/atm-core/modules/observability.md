# `atm-core::observability`

Owns the injected observability boundary used by `atm-core` services and the
ATM-owned event/query models that sit above shared observability crates.

It must not own:

- direct `sc-observability` initialization
- CLI flag parsing
- CLI output rendering

References:

- Product requirements: `docs/requirements.md` §3.5, §10, §11, and §13
- `REQ-P-LOG-001`
- `REQ-P-DOCTOR-001`
- `REQ-P-OBS-001`
- `REQ-CORE-OBS-001`
- Product architecture: `docs/architecture.md` §2.3
- Integration design:
  `docs/atm-core/design/sc-observability-integration.md`
