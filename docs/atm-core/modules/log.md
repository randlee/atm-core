# `atm-core::log`

Owns ATM-owned log query and tail request models, filtering semantics, and the
mapping from CLI queries into the injected observability boundary.

It must not own:

- clap parsing
- human-readable line formatting
- JSON CLI envelopes
- direct dependency on concrete `sc-observability` types

References:

- Product requirements: `docs/requirements.md` §10 and §13
- `REQ-P-LOG-001`
- `REQ-P-OBS-001`
- `REQ-CORE-LOG-001`
- CLI surface: `docs/atm/commands/log.md`
- Supporting boundary: `docs/atm-core/modules/observability.md`
