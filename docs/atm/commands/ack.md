# `atm ack`

CLI ownership for `atm ack`:

- message-id and reply parsing
- caller-team resolution plus owner-only caller-identity enforcement
- conversion into `atm-core` ack requests
- human-readable output
- JSON output

Ack transition semantics remain owned by `atm-core`.

`atm ack` distinguishes normal reply emission from suppressed self-ack
completion when a historical pending-ack message was already addressed back to
the current actor.

References:

- Product requirements: `docs/requirements.md` §8
- `REQ-P-ACK-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/ack.md`
