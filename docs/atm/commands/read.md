# `atm read`

CLI ownership for `atm read`:

- single-message selection flag parsing
- shared queue-filter parsing aligned with `atm list`
- deprecated legacy read-flag alias handling and warning presentation
- timeout flag parsing
- conversion into `atm-core` read requests
- human-readable full-message rendering
- JSON output for one selected message plus match metadata
- exact-message retrieval help text for ATM-authored JSONL retrieval stubs

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

Selection/rendering contract:

- `atm read` returns one full message only
- `atm read` is owner-only and may mutate read / seen state
- `atm read` never creates new pending-ack state on display
- use `atm peek` for non-mutating mailbox inspection, including inspection of
  another member with `--as`
- exact `--message-id` selection bypasses logical terminal-node collapse so the
  addressed physical message is returned directly
- task/from/contains/queue filters otherwise operate on logical current
  terminal-node messages
- when multiple matches remain, `atm read` renders the newest selected message
  plus `match_count` / `additional_match_count` metadata
- CLI-owned deprecation warnings for legacy queue flags are emitted after the
  result render so they do not obscure the selected message

Workflow/state behavior remains owned by `atm-core`.

References:

- Product requirements: `docs/requirements.md` §7 and `read-behavior.md`
- `REQ-P-READ-001`
- `REQ-ATM-CMD-001`
- `REQ-ATM-OUT-001`
- Product architecture: `docs/architecture.md`
- Core module: `docs/atm-core/modules/read.md`
