# `atm send`

CLI ownership for `atm send`:

- positional and flag parsing
- caller-team resolution plus owner-only caller-identity enforcement
- conversion into `atm-core` send requests
- human-readable output
- JSON output

Core send behavior remains owned by `atm-core`.

`atm send` accepts an inline destination as
`agent[:chat-id]@team[.host]`. The first `.` after the team begins the host;
the remaining hostname or literal IP is preserved in the canonical request.
The self-send guard rejects only the caller's exact `agent@team` destination
when it has no host. Any syntactically valid host-qualified destination,
including `localhost`, `127.0.0.1`, or an advertised IP, proceeds to the
ordinary host-routing contract. The shared `atm-core` path owns this rule; the
CLI adds no host flag or alternate route.

Acknowledgement ownership notes:

- `--requires-ack` creates durable sender-owned acknowledgement state at send
  time
- task-linked sends imply `requires_ack = true`
- plain informational sends remain `requires_ack = false`

References:

- Product requirements: `docs/requirements.md` §6
- `REQ-P-SEND-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- `REQ-CORE-CONFIG-002` for alias rewrite before canonical target resolution
- `REQ-CORE-SEND-002` for cross-team canonical `from` projection
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/send.md`
