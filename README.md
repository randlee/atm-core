# atm-core

Minimal ATM reset workspace.

Current target:
- `atm` CLI
- `send`
- `read`
- `log`
- `doctor`
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
- `crates/atm-core`: library for config, addressing, mailbox I/O, command services, diagnostics, and observability integration
- `crates/atm`: CLI binary only

No third first-party crate is planned for MVP. The first implementation dependency is an early `sc-observability` gap-analysis sprint to verify and close the shared query/follow/filter/health APIs needed by `atm log` and `atm doctor`.
