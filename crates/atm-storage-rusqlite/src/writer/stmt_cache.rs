use rusqlite::{CachedStatement, Connection, Params, Result as SqlResult};

#[derive(Debug, Default)]
pub(crate) struct WriterStatementCache;

impl WriterStatementCache {
    pub(crate) fn insert_message_row<P: Params>(
        &mut self,
        connection: &Connection,
        params: P,
    ) -> SqlResult<usize> {
        let mut statement = cached(
            connection,
            "INSERT INTO mail_messages(team, agent, message_key, envelope_json, from_agent, source_chat_id, destination_chat_id, message_text, summary, message_at, message_id, parent_message_id, thread_mode, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(team, agent, message_key) DO NOTHING;",
        )?;
        statement.execute(params)
    }

    pub(crate) fn upsert_message_state<P: Params>(
        &mut self,
        connection: &Connection,
        params: P,
    ) -> SqlResult<usize> {
        let mut statement = cached(
            connection,
            "INSERT INTO mail_message_states(team, agent, message_key, read, pending_ack_at, acknowledged_at, expires_at, deleted_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(team, agent, message_key) DO UPDATE SET
               read = excluded.read,
               pending_ack_at = excluded.pending_ack_at,
               acknowledged_at = excluded.acknowledged_at,
               expires_at = excluded.expires_at,
               deleted_at = excluded.deleted_at,
               updated_at = excluded.updated_at;",
        )?;
        statement.execute(params)
    }
}

fn cached<'a>(connection: &'a Connection, sql: &str) -> SqlResult<CachedStatement<'a>> {
    connection.prepare_cached(sql)
}
