# `atm list`

CLI ownership for `atm list`:

- bounded metadata-search flag parsing
- shared queue-filter parsing aligned with `atm read`
- match-filter parsing for task/thread lookups without full-body rendering
- conversion into `atm-core` list/query requests
- human-readable metadata row rendering
- JSON metadata output

Supported filters/flags:

- target inbox / agent
- `--team`
- `--all`
- `--unread`
- `--pending-ack`
- `--limit`
- `--since`
- `--from`
- `--task`
- `--contains`
- `--json`
- `--as`

Output contract:

- `atm list` never renders multiple full message bodies
- each row is metadata only:
  - `message_id`
  - `summary`
  - `from`
  - `timestamp`
  - `read`
  - `pending_ack`
  - `task_id`
- human output may indicate hidden history when the bounded actionable view is
  selected
- JSON output preserves queue counts plus the bounded row set

Workflow/state behavior remains owned by `atm-core`.

References:

- Product requirements: `docs/requirements.md` §7 and `read-behavior.md`
- `REQ-P-LIST-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/list.md`
