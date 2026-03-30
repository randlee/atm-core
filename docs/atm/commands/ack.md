# `atm ack`

CLI ownership for `atm ack`:

- message-id and reply parsing
- actor override parsing
- conversion into `atm-core` ack requests
- human-readable output
- JSON output

Ack transition semantics remain owned by `atm-core`.

References:

- Product requirements: `docs/requirements.md` §8
- `REQ-P-ACK-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/ack.md`
