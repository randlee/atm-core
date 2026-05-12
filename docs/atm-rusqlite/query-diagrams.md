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
- bounded metadata list:
  [atm-list.mmd](../atm/diagrams/atm-list.mmd)
- single-message read, including exact-id and superseded/current behavior:
  [atm-read.mmd](../atm/diagrams/atm-read.mmd)
- clear/delete status mutation:
  [atm-clear.mmd](../atm/diagrams/atm-clear.mmd)
- send/ack write-side commit and post-commit effects:
  [atm-send-compose.mmd](../atm/diagrams/atm-send-compose.mmd)
  [atm-send-ack.mmd](../atm/diagrams/atm-send-ack.mmd)

Static HTML viewer:
- [query-diagrams.html](./query-diagrams.html)

Optimization target:
- `atm list` should use one status-rooted candidate/count query and one
  limited header projection query
- `atm read` should use one status-rooted selection query and one content fetch
  for the chosen row, plus one optional status mutation query when read/ack
  state changes
