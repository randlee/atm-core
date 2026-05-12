# ATM SQLite Query Diagrams

The diagrams here describe the simplified target query model.

Query rules:
- every ATM-owned mailbox read starts from mutable message-status state
- deleted rows remain hidden from normal queries
- logical-current queries exclude superseded rows unless an admin-only
  diagnostic surface explicitly asks for them
- full message content is fetched only for selected keys that must be rendered
- no ATM-owned mailbox query reads inbox JSON or summary files

Diagrams:
- save message:
  [sql_save-message.mmd](./sql_save-message.mmd)
- load message:
  [sql_load-message.mmd](./sql_load-message.mmd)
- save visibility state:
  [sql_save-visibility-state.mmd](./sql_save-visibility-state.mmd)
- load visibility state:
  [sql_load-visibility-state.mmd](./sql_load-visibility-state.mmd)
- query mailbox metadata rows:
  [sql_query-mailbox-metadata-rows.mmd](./sql_query-mailbox-metadata-rows.mmd)
- query mailbox metadata counts:
  [sql_query-mailbox-metadata-counts.mmd](./sql_query-mailbox-metadata-counts.mmd)

Static HTML viewer:
- [query-diagrams.html](../reports/query-diagrams.html)

Optimization target:
- `atm list` should use one status-rooted candidate/count query and one
  limited header projection query
- `atm read` should use one status-rooted selection query and one content fetch
  for the chosen row, plus one optional status mutation query when read/ack
  state changes
