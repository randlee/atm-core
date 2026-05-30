# `atm-core::list`

Owns bounded metadata query behavior for queue inspection, including shared
filter semantics with `atm read`, successor-chain terminal-node collapse,
deduplication by `message_id`, compact row shaping, and metadata/count query
paths that do not require full-body response materialization.

Module rules:

- list selection operates on logical current terminal-node messages
- exact physical-message lookup is not owned here; that remains a `read`
  concern via `--message-id`
- shared filters include sender, timestamp, task, contains-text, and the
  actionable/unread/pending-ack/all selection buckets
- list result rows are metadata only and must not require body render paths

References:

- Product requirements: `docs/requirements.md` §7
- `REQ-P-LIST-001`
- `REQ-CORE-LIST-001`
- `REQ-CORE-WORKFLOW-001`
- Cross-cutting behavior: `docs/read-behavior.md`
- CLI surface: `docs/atm/commands/list.md`
