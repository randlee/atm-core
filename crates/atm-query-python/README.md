# atm-query

Local, read-only Python access to the ATM ([atm-core](https://github.com/randlee/atm-core/blob/develop/README.md)) message
store for analysis.

`atm_query` opens the current OS account's `~/.atm/db/mail.db` and runs one
parameterized, read-only SQL statement at a time against the versioned
`decomposed_messages` view. It is not a network client: nothing here reaches
the daemon, a peer, or another host.

```python
from atm_query import open_readonly

rows = open_readonly().query(
    "SELECT team, agent, template_type FROM decomposed_messages WHERE template_type = ?",
    ("qa-report",),
)
```

- `open_readonly(database_path=None)` returns a read-only database handle. With
  no path it opens `~/.atm/db/mail.db`; `ATM_HOME` never selects the database.
- `ReadonlyDatabase.query(sql, parameters=None)` returns a list of
  column-labelled dict rows.
- Errors raise `AtmQueryError` with a stable code.

The connection uses `query_only`, SQLite defensive settings, an authorizer that
allows only the `decomposed_messages` view, and fixed execution, row and
result-size budgets. Base tables are not a contract.

The query surface is documented in
[docs/atm-query-surface.md](https://github.com/randlee/atm-core/blob/develop/docs/atm-query-surface.md).

License: MIT OR Apache-2.0.
