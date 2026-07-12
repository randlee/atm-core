# `atm peek`

CLI ownership for `atm peek`:

- single-message selection flag parsing
- shared queue-filter parsing aligned with `atm read`
- timeout flag parsing
- conversion into the daemon-backed non-mutating `atm-core` peek request
- human-readable full-message rendering
- JSON output for one selected message plus match metadata

Supported selectors/filters:

- target inbox / agent
- `--team`
- `--all`
- `--unread` and legacy alias `--unread-only`
- `--pending-ack` and legacy alias `--pending-ack-only`
- `--message-id`
- `--task`
- `--contains`
- `--since`
- `--from`
- `--since-last-seen` and `--no-since-last-seen`
- `--timeout`
- `--json`
- `--as`

Selection/rendering contract:

- `atm peek` returns one full message only
- `atm peek` never mutates read state, seen watermarks, or ack state
- when multiple matches remain, `atm peek` renders the newest selected message
  plus `match_count` / `additional_match_count` metadata

References:

- Product requirements: `docs/requirements.md` §7 and `read-behavior.md`
- `REQ-P-READ-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/read.md`
