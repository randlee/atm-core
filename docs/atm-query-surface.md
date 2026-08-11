# ATM local query surface

## `decomposed_messages` v1

`decomposed_messages` is the versioned, read-only SQLite view for local
consumers that need durable decomposed-template metadata. It is the supported
SQL surface; the underlying `mail_messages`, `message_templates`, and
`mail_message_states` tables remain implementation details.

Version 1 exposes exactly these columns, in this order:

1. `team`
2. `agent`
3. `from_agent`
4. `message_at`
5. `message_id`
6. `template_sha`
7. `template_type`
8. `vars_json`
9. `category`
10. `tags_json`
11. `summary`
12. `read`
13. `acknowledged_at`
14. `pending_ack_at`

The view contains only rows whose `template_sha` resolves to a durable,
immutable catalog entry. It is created additively for both fresh and migrated
SQLite databases. Future changes require a separately versioned view; callers
must not depend on base-table column layout.
