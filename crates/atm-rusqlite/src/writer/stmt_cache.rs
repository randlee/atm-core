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
            "INSERT INTO mail_messages(team, agent, message_key, envelope_json, from_agent, message_text, summary, message_at, legacy_message_id, parent_message_id, thread_mode, stale_at, imported_from, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(team, agent, message_key) DO NOTHING;",
        )?;
        statement.execute(params)
    }

    pub(crate) fn load_successor_owner<P: Params>(
        &mut self,
        connection: &Connection,
        params: P,
    ) -> SqlResult<String> {
        let mut statement = cached(
            connection,
            "SELECT message_key
             FROM mail_messages
             WHERE team = ?1 AND agent = ?2 AND parent_message_id = ?3;",
        )?;
        statement.query_row(params, |row| row.get(0))
    }

    pub(crate) fn load_legacy_identity_owner<P: Params>(
        &mut self,
        connection: &Connection,
        params: P,
    ) -> SqlResult<String> {
        let mut statement = cached(
            connection,
            "SELECT message_key
             FROM mail_messages
             WHERE team = ?1 AND agent = ?2 AND legacy_message_id = ?3;",
        )?;
        statement.query_row(params, |row| row.get(0))
    }

    pub(crate) fn upsert_ack_state<P: Params>(
        &mut self,
        connection: &Connection,
        params: P,
    ) -> SqlResult<usize> {
        let mut statement = cached(
            connection,
            "INSERT INTO ack_state(team, agent, message_key, pending_ack_at, acknowledged_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(team, agent, message_key) DO UPDATE SET
               pending_ack_at = excluded.pending_ack_at,
               acknowledged_at = excluded.acknowledged_at,
               updated_at = excluded.updated_at;",
        )?;
        statement.execute(params)
    }

    pub(crate) fn upsert_visibility_state<P: Params>(
        &mut self,
        connection: &Connection,
        params: P,
    ) -> SqlResult<usize> {
        let mut statement = cached(
            connection,
            "INSERT INTO mail_visibility_states(team, agent, message_key, state_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(team, agent, message_key) DO UPDATE SET
               state_json = excluded.state_json;",
        )?;
        statement.execute(params)
    }
}

fn cached<'a>(connection: &'a Connection, sql: &str) -> SqlResult<CachedStatement<'a>> {
    connection.prepare_cached(sql)
}
