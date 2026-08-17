# `atm members`

CLI ownership for `atm members`:

- flag parsing
- conversion into `atm-core` member-list requests
- human-readable output
- JSON output

The member listing exposes the persisted `harness` for every member in both
human-readable and JSON output. This is the authoritative read-only surface
for checking whether a recipient uses a graft-compatible harness (`hermes` or
`python-graft`) before exercising graft delivery.

Core roster loading and deterministic member projection remain owned by
`atm-core`.

References:

- Product requirements: `docs/requirements.md` §13
- `REQ-P-MEMBERS-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/team_admin.md`
