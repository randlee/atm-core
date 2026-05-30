# ATM SQLite Query Diagrams

The diagrams here describe the simplified target query model.

Query rules:
- every ATM-owned mailbox read uses SQLite mailbox rows plus unified message
  state
- one explicit `mail_message_states` table owns mutable mailbox state
- deleted rows remain hidden from normal queries
- expired rows remain hidden from normal queries
- full message content is fetched only for selected keys that must be rendered
- no ATM-owned mailbox query reads inbox JSON or summary files
- weak provenance round-trip fields are not part of the message load/save
  contract; store-owned ingest timing may still exist internally for health
  reporting

Diagrams:
- save message:
  [sql_save-message.mmd](./sql_save-message.mmd)
- load message:
  [sql_load-message.mmd](./sql_load-message.mmd)
- save message state:
  [sql_save-message-state.mmd](./sql_save-message-state.mmd)
- load message state:
  [sql_load-message-state.mmd](./sql_load-message-state.mmd)
- query mailbox metadata rows:
  [sql_query-mailbox-metadata-rows.mmd](./sql_query-mailbox-metadata-rows.mmd)
- query mailbox metadata counts:
  [sql_query-mailbox-metadata-counts.mmd](./sql_query-mailbox-metadata-counts.mmd)

Static HTML viewer:
- [query-diagrams.html](../reports/query-diagrams.html)

Optimization target:
- `atm list` currently uses `query_mailbox_metadata_rows` and computes
  logical-current collapse plus bucket counts in ATM code
- `query_mailbox_metadata_counts` exists as a dedicated SQL aggregate method,
  but `atm list` does not call it yet
- `atm read` currently uses one metadata query, one content fetch for the
  selected row, and one optional message-state write when display state changes
