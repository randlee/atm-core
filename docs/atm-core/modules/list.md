# `atm-core::list`

Owns bounded metadata query behavior for queue inspection, including shared
filter semantics with `atm read`, deduplication by `message_id`, and compact
row shaping for the CLI layer to render.

References:

- Product requirements: `docs/requirements.md` §7
- `REQ-P-LIST-001`
- `REQ-CORE-LIST-001`
- `REQ-CORE-WORKFLOW-001`
- Cross-cutting behavior: `docs/read-behavior.md`
- CLI surface: `docs/atm/commands/list.md`
