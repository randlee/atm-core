//! Open-time normalization for pre-Phase-BB assignment marker state.

use atm_storage::AtmError;

use crate::shared_db::{SharedDbTarget, SqliteConnection, sqlite_error};

pub(crate) fn normalize_legacy_assignment_markers(
    connection: &SqliteConnection,
    target: &SharedDbTarget,
) -> Result<u64, AtmError> {
    let affected = connection
        .execute(
            "UPDATE mail_message_states
                SET pending_ack_at = NULL, nudge_pending_at = NULL, nudge_attempts = 0
              WHERE acknowledged_at IS NULL
                AND (team, agent, message_key) IN (
                     SELECT m.team, m.agent, m.message_key
                       FROM mail_messages m
                       JOIN tasks t ON t.team = m.team AND t.assignee = m.agent
                                   AND t.assignment_message_id = m.message_id
                      WHERE t.state IN ('assigned', 'active'))",
            [],
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to normalize legacy assignment markers",
                error,
            )
        })?;
    u64::try_from(affected)
        .map_err(|_| AtmError::mailbox_write("legacy assignment marker count overflowed u64"))
}

#[cfg(test)]
mod tests {
    use super::normalize_legacy_assignment_markers;
    use crate::shared_db::{SharedDbTarget, SqliteConnection};
    use crate::writer::task_ops::params;

    #[test]
    fn ba_fixture_with_pending_assignments_opens_with_zero_pending_ack_and_no_markers() {
        let connection = SqliteConnection::open_in_memory().expect("fixture database");
        connection
            .execute_batch(
                "CREATE TABLE mail_messages (
                    team TEXT NOT NULL, agent TEXT NOT NULL, message_key TEXT NOT NULL,
                    message_id TEXT NOT NULL
                 );
                 CREATE TABLE tasks (
                    team TEXT NOT NULL, assignee TEXT NOT NULL,
                    assignment_message_id TEXT NOT NULL, state TEXT NOT NULL
                 );
                 CREATE TABLE mail_message_states (
                    team TEXT NOT NULL, agent TEXT NOT NULL, message_key TEXT NOT NULL,
                    acknowledged_at TEXT, pending_ack_at TEXT,
                    nudge_pending_at TEXT, nudge_attempts INTEGER NOT NULL
                 );
                 INSERT INTO mail_messages VALUES ('bb','alice','m1','id1');
                 INSERT INTO tasks VALUES ('bb','alice','id1','assigned');
                 INSERT INTO mail_message_states
                    VALUES ('bb','alice','m1',NULL,'legacy','legacy',7);",
            )
            .expect("BA fixture");
        let target = SharedDbTarget::InMemory {
            uri: "bb5-assignment-migration".to_owned(),
        };

        assert_eq!(
            normalize_legacy_assignment_markers(&connection, &target).unwrap(),
            1
        );
        let state: (Option<String>, Option<String>, u32) = connection
            .query_row(
                "SELECT pending_ack_at, nudge_pending_at, nudge_attempts
                   FROM mail_message_states WHERE message_key = ?1",
                params!["m1"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(state, (None, None, 0));
        assert_eq!(
            normalize_legacy_assignment_markers(&connection, &target).unwrap(),
            1,
            "the statement is idempotent even though SQLite reports matched rows"
        );
    }
}
