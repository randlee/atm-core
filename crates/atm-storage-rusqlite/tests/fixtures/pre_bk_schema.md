# Frozen Pre-BK SQLite Schema

Source: `94bfc904477d043af29e0ca3cabac64f99ded2a4`, the phase-BK integration
baseline before the hook/cron schema landed. These fixtures must not track the
current migration implementation.

- `pre_bk_db_migrations.sql` is the byte-for-byte evaluated `DB_MIGRATIONS`
  string from `src/shared_db.rs`, including the expansions of
  `mail_messages_table_ddl!("CREATE TABLE IF NOT EXISTS mail_messages")` and
  `mail_messages_index_ddl!()` in `src/mail_messages_schema.rs`.
  SHA-256: `0e2f5ff7319aa0c28faa2a1666325e8ba727077c5a33b0cac22315871885253c`.
- `pre_bk_task_schema.sql` is the byte-for-byte concatenation of
  `TASK_TABLES_DDL`, `TASK_INDEX_DDL` and `ESCALATION_RECIPIENTS_DDL` from
  `src/task_store.rs` at that same commit. The pre-BK `ensure_schema` applied
  these separately from `DB_MIGRATIONS`; they include `prompt_handoffs`.
  SHA-256: `c461cebffe4edd326a8d5261c03491e1ba48319971085cbc88f04bbc4752c174`.

`hook_schema::pre_bk_ddl_set_reads_and_writes_after_hook_tables_created` covers
both an empty database and a database seeded with these frozen statements and
representative rows before opening the current backend. It verifies both new
hook tables, preservation of old rows, replay of the frozen old DDL, and old
column-set inserts/reads after migration for mail, message state, watermark,
roster, task, task-event and prompt-handoff tables. The new hook/cron rows also
survive the old statements and a subsequent backend reopen.
