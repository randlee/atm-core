# atm-core

Minimal ATM reset workspace.

Current target:
- `atm` CLI
- `send`
- `read`
- `sc-observability` logging
- no daemon
- no CI monitoring
- no agent-state integration in MVP

Docs:
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`
- `docs/migration-map.md`
- `docs/file-migration-plan.md`
- `docs/read-behavior.md`

Crates planned for MVP:
- `crates/atm-core`: library for config, addressing, mailbox I/O, command services, and observability integration
- `crates/atm`: CLI binary only

No third first-party crate is planned for MVP. Add one only if a second non-CLI consumer appears or `sc-observability` integration proves large enough to justify a separate boundary.
