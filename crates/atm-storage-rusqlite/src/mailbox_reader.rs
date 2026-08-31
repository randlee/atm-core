//! Bounded, backend-owned read-only mailbox lane.
//!
//! The Tokio runtime awaits this lane; it never borrows the SQLite writer or
//! schedules `spawn_blocking` work for ordinary mailbox reads.

use std::sync::Arc;

use atm_storage::{
    AsyncMailboxReader, AtmError, IsoTimestamp, MailboxScope, Message, MessageKey, MessageQuery,
    ReadDeadline, ReadLaneError,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::reader_pool::{ReaderPool, ReaderPoolConfig};
use crate::shared_db::{SharedDbTarget, deserialize_json, sqlite_error};

pub(crate) const DEFAULT_MAILBOX_READER_CONFIG: ReaderPoolConfig =
    ReaderPoolConfig::mailbox_defaults();

struct MailboxReader {
    pool: ReaderPool,
}

impl MailboxReader {
    fn start(target: Arc<SharedDbTarget>) -> Result<Self, AtmError> {
        Ok(Self {
            pool: ReaderPool::start("mailbox", target, DEFAULT_MAILBOX_READER_CONFIG)?,
        })
    }

    async fn submit_list(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        if !scope.permits(&query) {
            return Err(ReadLaneError::UnauthorizedScope);
        }
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                list_messages(connection, target, &scope, &query).map_err(storage_error)
            })
            .await
    }

    async fn submit_load(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: ReadDeadline,
    ) -> Result<Option<Message>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                load_message(connection, target, &scope, &key).map_err(storage_error)
            })
            .await
    }
}

pub(crate) fn start_mailbox_reader(
    target: Arc<SharedDbTarget>,
) -> Result<Arc<dyn AsyncMailboxReader + Send + Sync>, AtmError> {
    Ok(Arc::new(MailboxReader::start(target)?))
}

#[async_trait::async_trait]
impl AsyncMailboxReader for MailboxReader {
    async fn list_messages(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        self.submit_list(scope, query, deadline).await
    }

    async fn load_message(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: ReadDeadline,
    ) -> Result<Option<Message>, ReadLaneError> {
        self.submit_load(scope, key, deadline).await
    }
}

impl atm_storage::contract::sealed::Sealed for MailboxReader {}

fn storage_error(error: AtmError) -> ReadLaneError {
    ReadLaneError::Unavailable {
        message: error.message().to_owned(),
    }
}

fn list_messages(
    connection: &Connection,
    target: &SharedDbTarget,
    scope: &MailboxScope,
    query: &MessageQuery,
) -> Result<Vec<Message>, AtmError> {
    if !scope.permits(query) {
        return Err(AtmError::validation(
            "mailbox scope does not authorize this query",
        ));
    }
    let limit = query
        .limit
        .map(|value| i64::try_from(value).unwrap_or(i64::MAX))
        .unwrap_or(-1);
    let transaction = connection.unchecked_transaction().map_err(|error| {
        sqlite_error(
            target,
            "failed to open bounded mailbox reader transaction",
            error,
        )
    })?;
    let mut statement = transaction
        .prepare(
            "SELECT mail_messages.message_key, mail_messages.envelope_json
         FROM mail_messages
         LEFT JOIN mail_message_states
           ON mail_message_states.team = mail_messages.team
          AND mail_message_states.agent = mail_messages.agent
          AND mail_message_states.message_key = mail_messages.message_key
         WHERE mail_messages.team = ?1
           AND mail_messages.agent = ?2
           AND (?3 IS NULL OR mail_messages.from_agent = ?3)
           AND (?4 IS NULL OR json_extract(mail_messages.envelope_json, '$.taskId') = ?4)
           AND mail_message_states.deleted_at IS NULL
           AND (mail_message_states.expires_at IS NULL
                OR mail_message_states.expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ORDER BY mail_messages.message_at DESC, mail_messages.message_key DESC
         LIMIT ?5;",
        )
        .map_err(|error| {
            sqlite_error(target, "failed to prepare mailbox reader list query", error)
        })?;
    let rows = statement
        .query_map(
            params![
                query.team.as_str(),
                query.agent.as_str(),
                query.sender.as_ref().map(|value| value.as_str()),
                query.task_id.as_ref().map(|value| value.as_str()),
                limit,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|error| {
            sqlite_error(target, "failed to execute mailbox reader list query", error)
        })?;
    let mut messages = Vec::new();
    for row in rows {
        let (key, envelope_json) = row
            .map_err(|error| sqlite_error(target, "failed to decode mailbox reader row", error))?;
        let key = MessageKey::new(key)?;
        let state = load_state(&transaction, target, &query.team, &query.agent, &key)?;
        let envelope = apply_state(
            deserialize_json(&envelope_json, "sqlite message envelope")?,
            state.as_ref(),
        );
        messages.push(Message {
            team: query.team.clone(),
            agent: query.agent.clone(),
            message_key: key,
            envelope,
        });
    }
    drop(statement);
    transaction.commit().map_err(|error| {
        sqlite_error(target, "failed to close mailbox reader transaction", error)
    })?;
    Ok(messages)
}

fn load_message(
    connection: &Connection,
    target: &SharedDbTarget,
    scope: &MailboxScope,
    key: &MessageKey,
) -> Result<Option<Message>, AtmError> {
    let transaction = connection.unchecked_transaction().map_err(|error| {
        sqlite_error(
            target,
            "failed to open bounded mailbox reader transaction",
            error,
        )
    })?;
    let row = transaction
        .query_row(
            "SELECT team, agent, envelope_json FROM mail_messages WHERE message_key = ?1;",
            params![key.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load mailbox reader record", error))?;
    let message = row
        .map(|(team, agent, envelope_json)| {
            let team = team.parse().map_err(|error| {
                AtmError::validation(format!("failed to parse sqlite team: {error}"))
            })?;
            let agent = agent.parse().map_err(|error| {
                AtmError::validation(format!("failed to parse sqlite agent: {error}"))
            })?;
            if scope.team != team || scope.agent != agent {
                return Err(AtmError::validation(
                    "mailbox scope does not authorize this record",
                ));
            }
            let state = load_state(&transaction, target, &team, &agent, key)?;
            let envelope = apply_state(
                deserialize_json(&envelope_json, "sqlite message envelope")?,
                state.as_ref(),
            );
            Ok(Message {
                team,
                agent,
                message_key: key.clone(),
                envelope,
            })
        })
        .transpose()?;
    transaction.commit().map_err(|error| {
        sqlite_error(target, "failed to close mailbox reader transaction", error)
    })?;
    Ok(message)
}

#[derive(Debug, Clone)]
struct StoredState {
    read: bool,
    pending_ack_at: Option<IsoTimestamp>,
    acknowledged_at: Option<IsoTimestamp>,
    expires_at: Option<IsoTimestamp>,
}

fn load_state(
    connection: &Connection,
    target: &SharedDbTarget,
    team: &atm_storage::TeamName,
    agent: &atm_storage::AgentName,
    key: &MessageKey,
) -> Result<Option<StoredState>, AtmError> {
    connection
        .query_row(
            "SELECT read, pending_ack_at, acknowledged_at, expires_at FROM mail_message_states
         WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
            params![team.as_str(), agent.as_str(), key.as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load mailbox reader state", error))?
        .map(|(read, pending, acknowledged, expires)| {
            Ok(StoredState {
                read: read != 0,
                pending_ack_at: parse_timestamp(pending, "pending_ack_at")?,
                acknowledged_at: parse_timestamp(acknowledged, "acknowledged_at")?,
                expires_at: parse_timestamp(expires, "expires_at")?,
            })
        })
        .transpose()
}

fn parse_timestamp(raw: Option<String>, field: &str) -> Result<Option<IsoTimestamp>, AtmError> {
    raw.map(|value| value.parse()).transpose().map_err(|error| {
        AtmError::validation(format!(
            "failed to parse mail-store {field} timestamp: {error}"
        ))
    })
}

fn apply_state(
    mut envelope: atm_storage::MessageEnvelope,
    state: Option<&StoredState>,
) -> atm_storage::MessageEnvelope {
    if let Some(state) = state {
        envelope.read = state.read;
        envelope.pending_ack_at = state.pending_ack_at;
        envelope.acknowledged_at = state.acknowledged_at;
        envelope.expires_at = state.expires_at;
    }
    envelope
}
