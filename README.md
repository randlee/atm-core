# atm-core

Daemon-free ATM workspace for the retained `1.0` release surface.

Current target:
- `atm` CLI
- `send`
- `read`
- `ack`
- `clear`
- `log`
- `doctor`
- `sc-observability` logging
- task-linked mail metadata with mandatory ack
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

Published crate/package identities:
- `agent-team-mail-core`: daemon-free core library published from
  `crates/atm-core`
- `agent-team-mail`: CLI package published from `crates/atm` and installing the
  `atm` binary

Workspace crates:
- `crates/atm-core`: library for config, addressing, mailbox I/O, command
  services, diagnostics, and the observability port boundary
- `crates/atm`: CLI binary plus the concrete `sc-observability` integration

No third first-party crate is planned for MVP. The first implementation dependency is an early `sc-observability` gap-analysis sprint to verify and close the shared query/follow/filter/health APIs needed by `atm log` and `atm doctor`.
