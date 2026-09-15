//! Bounded, backend-owned read-only mailbox lane.
//!
//! The Tokio runtime awaits this lane; it never borrows the SQLite writer or
//! schedules `spawn_blocking` work for ordinary mailbox reads.

use atm_storage::{
    AsyncMailboxReader, AtmError, IsoTimestamp, MailboxListQuery, MailboxScope, Message,
    MessageKey, MessageQuery, ReadDeadline, ReadLaneError, SearchCount, SearchCountGroupBy,
    SearchFilters,
};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::reader_pool::ReaderPool;
use crate::search_store::{SqlFilterAliases, compile_sql_filters_for};
use crate::shared_db::{SharedDbTarget, deserialize_json, sqlite_error};

struct MailboxReader {
    pool: ReaderPool,
}

impl MailboxReader {
    fn new(pool: ReaderPool) -> Self {
        Self { pool }
    }

    async fn submit_list(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        self.submit_list_with_class(scope, query, deadline, false)
            .await
    }

    async fn submit_tool_list(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        self.submit_list_with_class(scope, query, deadline, true)
            .await
    }

    async fn submit_list_with_class(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
        tool_class: bool,
    ) -> Result<Vec<Message>, ReadLaneError> {
        if !scope.permits(&query) {
            return Err(ReadLaneError::UnauthorizedScope);
        }
        if tool_class {
            self.pool
                .submit_tool(deadline.remaining(), move |connection, target| {
                    list_messages(connection, target, &scope, &query)
                        .map_err(read_lane_storage_error)
                })
                .await
        } else {
            self.pool
                .submit(deadline.remaining(), move |connection, target| {
                    list_messages(connection, target, &scope, &query)
                        .map_err(read_lane_storage_error)
                })
                .await
        }
    }

    async fn submit_matching_list(
        &self,
        scope: MailboxScope,
        mut query: MailboxListQuery,
        deadline: ReadDeadline,
        tool_class: bool,
    ) -> Result<Vec<Message>, ReadLaneError> {
        if query
            .filters
            .team
            .as_ref()
            .is_some_and(|team| team != &scope.team)
            || query
                .filters
                .agent
                .as_ref()
                .is_some_and(|agent| agent != &scope.agent)
        {
            return Err(ReadLaneError::UnauthorizedScope);
        }
        query.filters.team = Some(scope.team.clone());
        query.filters.agent = Some(scope.agent.clone());
        let submit = move |connection: &Connection, target: &SharedDbTarget| {
            list_matching_messages(connection, target, &scope, &query)
                .map_err(read_lane_storage_error)
        };
        if tool_class {
            self.pool.submit_tool(deadline.remaining(), submit).await
        } else {
            self.pool.submit(deadline.remaining(), submit).await
        }
    }

    async fn submit_load(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: ReadDeadline,
    ) -> Result<Option<Message>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                load_message(connection, target, &scope, &key)
            })
            .await
    }

    async fn submit_member_exists(
        &self,
        scope: MailboxScope,
        deadline: ReadDeadline,
    ) -> Result<bool, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                mailbox_member_exists(connection, target, &scope).map_err(read_lane_storage_error)
            })
            .await
    }

    async fn submit_seen_watermark(
        &self,
        scope: MailboxScope,
        deadline: ReadDeadline,
    ) -> Result<Option<IsoTimestamp>, ReadLaneError> {
        self.pool
            .submit(deadline.remaining(), move |connection, target| {
                load_seen_watermark(connection, target, &scope).map_err(read_lane_storage_error)
            })
            .await
    }

    async fn submit_count(
        &self,
        scope: MailboxScope,
        filters: SearchFilters,
        group_by: Option<SearchCountGroupBy>,
        deadline: ReadDeadline,
    ) -> Result<Vec<SearchCount>, ReadLaneError> {
        if filters
            .team
            .as_ref()
            .is_some_and(|team| team != &scope.team)
            || filters
                .agent
                .as_ref()
                .is_some_and(|agent| agent != &scope.agent)
        {
            return Err(ReadLaneError::UnauthorizedScope);
        }
        self.pool
            .submit_tool(deadline.remaining(), move |connection, target| {
                count_messages(connection, target, &filters, group_by)
                    .map_err(read_lane_storage_error)
            })
            .await
    }
}

fn list_matching_messages(
    connection: &Connection,
    target: &SharedDbTarget,
    scope: &MailboxScope,
    query: &MailboxListQuery,
) -> Result<Vec<Message>, AtmError> {
    let compiled = compile_sql_filters_for(&query.filters, SqlFilterAliases::MAILBOX);
    let limit = query
        .limit
        .map(|value| i64::try_from(value).unwrap_or(i64::MAX))
        .unwrap_or(-1);
    let sql = format!(
        "SELECT m.message_key, m.envelope_json, ms.message_key, ms.read,
                ms.pending_ack_at, ms.acknowledged_at, ms.expires_at
         FROM mail_messages m
         LEFT JOIN mail_message_states ms
           ON (ms.team, ms.agent, ms.message_key) = (m.team, m.agent, m.message_key)
         LEFT JOIN message_templates t ON t.template_sha = m.template_sha
         WHERE 1 = 1 {}
         ORDER BY m.message_at DESC, m.message_key DESC
         LIMIT ?",
        compiled.clause
    );
    let transaction = open_reader_transaction(connection, target)?;
    let mut statement = transaction.prepare_cached(&sql).map_err(|error| {
        sqlite_error(
            target,
            "failed to prepare criteria mailbox reader query",
            error,
        )
    })?;
    let mut parameters = compiled.parameters;
    parameters.push(rusqlite::types::Value::Integer(limit));
    let rows = statement
        .query_map(params_from_iter(parameters), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to execute criteria mailbox reader query",
                error,
            )
        })?;
    let messages = rows
        .map(|row| {
            let (key, envelope_json, state_key, read, pending_ack_at, acknowledged_at, expires_at) =
                row.map_err(|error| {
                    sqlite_error(target, "failed to decode criteria mailbox row", error)
                })?;
            let state = state_key
                .map(|_| {
                    Ok::<StoredState, AtmError>(StoredState {
                        read: read.unwrap_or_default() != 0,
                        pending_ack_at: parse_timestamp(pending_ack_at, "pending_ack_at")?,
                        acknowledged_at: parse_timestamp(acknowledged_at, "acknowledged_at")?,
                        expires_at: parse_timestamp(expires_at, "expires_at")?,
                    })
                })
                .transpose()?;
            Ok(Message {
                team: scope.team.clone(),
                agent: scope.agent.clone(),
                message_key: MessageKey::new(key)?,
                envelope: apply_state(
                    deserialize_json(&envelope_json, "sqlite message envelope")?,
                    state.as_ref(),
                ),
            })
        })
        .collect::<Result<Vec<_>, AtmError>>()?;
    drop(statement);
    close_reader_transaction(transaction, target)?;
    Ok(messages)
}

pub(crate) fn start_mailbox_reader(
    pool: ReaderPool,
) -> std::sync::Arc<dyn AsyncMailboxReader + Send + Sync> {
    let reader = MailboxReader::new(pool);
    std::sync::Arc::new(reader)
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

    async fn list_messages_for_tool(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        self.submit_tool_list(scope, query, deadline).await
    }

    async fn list_matching_messages(
        &self,
        scope: MailboxScope,
        query: MailboxListQuery,
        deadline: ReadDeadline,
    ) -> Result<Vec<Message>, ReadLaneError> {
        self.submit_matching_list(scope, query, deadline, true)
            .await
    }

    async fn count_messages(
        &self,
        scope: MailboxScope,
        filters: SearchFilters,
        group_by: Option<SearchCountGroupBy>,
        deadline: ReadDeadline,
    ) -> Result<Vec<SearchCount>, ReadLaneError> {
        self.submit_count(scope, filters, group_by, deadline).await
    }

    async fn load_message(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: ReadDeadline,
    ) -> Result<Option<Message>, ReadLaneError> {
        self.submit_load(scope, key, deadline).await
    }

    async fn mailbox_member_exists(
        &self,
        scope: MailboxScope,
        deadline: ReadDeadline,
    ) -> Result<bool, ReadLaneError> {
        self.submit_member_exists(scope, deadline).await
    }

    async fn load_seen_watermark(
        &self,
        scope: MailboxScope,
        deadline: ReadDeadline,
    ) -> Result<Option<IsoTimestamp>, ReadLaneError> {
        self.submit_seen_watermark(scope, deadline).await
    }
}

impl atm_storage::contract::sealed::Sealed for MailboxReader {}

pub(crate) fn read_lane_storage_error(error: AtmError) -> ReadLaneError {
    ReadLaneError::Storage {
        code: error.code(),
        message: error.message().to_owned(),
        cause: error.cause().map(str::to_owned),
    }
}

fn storage_error(error: AtmError) -> ReadLaneError {
    read_lane_storage_error(error)
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
    let transaction = open_reader_transaction(connection, target)?;
    let mut statement = transaction
        .prepare(
            "SELECT mail_messages.message_key, mail_messages.envelope_json,
                 mail_message_states.message_key, mail_message_states.read,
                 mail_message_states.pending_ack_at, mail_message_states.acknowledged_at,
                 mail_message_states.expires_at
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
    let messages = decode_list_messages(target, query, limit, &mut statement)?;
    drop(statement);
    close_reader_transaction(transaction, target)?;
    Ok(messages)
}

fn open_reader_transaction<'connection>(
    connection: &'connection Connection,
    target: &SharedDbTarget,
) -> Result<rusqlite::Transaction<'connection>, AtmError> {
    connection.unchecked_transaction().map_err(|error| {
        sqlite_error(
            target,
            "failed to open bounded mailbox reader transaction",
            error,
        )
    })
}

fn decode_list_messages(
    target: &SharedDbTarget,
    query: &MessageQuery,
    limit: i64,
    statement: &mut rusqlite::Statement<'_>,
) -> Result<Vec<Message>, AtmError> {
    let rows = statement
        .query_map(
            params![
                query.team.as_str(),
                query.agent.as_str(),
                query.sender.as_ref().map(|value| value.as_str()),
                query.task_id.as_ref().map(|value| value.as_str()),
                limit,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .map_err(|error| {
            sqlite_error(target, "failed to execute mailbox reader list query", error)
        })?;
    rows.map(|row| {
        let (key, envelope_json, state_key, read, pending_ack_at, acknowledged_at, expires_at) =
            row.map_err(|error| {
                sqlite_error(target, "failed to decode mailbox reader row", error)
            })?;
        let key = MessageKey::new(key)?;
        let state = state_key
            .map(|_| {
                Ok::<StoredState, AtmError>(StoredState {
                    read: read.unwrap_or_default() != 0,
                    pending_ack_at: parse_timestamp(pending_ack_at, "pending_ack_at")?,
                    acknowledged_at: parse_timestamp(acknowledged_at, "acknowledged_at")?,
                    expires_at: parse_timestamp(expires_at, "expires_at")?,
                })
            })
            .transpose()?;
        Ok(Message {
            team: query.team.clone(),
            agent: query.agent.clone(),
            message_key: key,
            envelope: apply_state(
                deserialize_json(&envelope_json, "sqlite message envelope")?,
                state.as_ref(),
            ),
        })
    })
    .collect()
}

fn close_reader_transaction(
    transaction: rusqlite::Transaction<'_>,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    transaction
        .commit()
        .map_err(|error| sqlite_error(target, "failed to close mailbox reader transaction", error))
}

fn load_message(
    connection: &Connection,
    target: &SharedDbTarget,
    scope: &MailboxScope,
    key: &MessageKey,
) -> Result<Option<Message>, ReadLaneError> {
    let transaction = open_reader_transaction(connection, target).map_err(storage_error)?;
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
        .map_err(|error| {
            storage_error(sqlite_error(
                target,
                "failed to load mailbox reader record",
                error,
            ))
        })?;
    let message = row
        .map(|(team, agent, envelope_json)| {
            let team = team.parse().map_err(|error| {
                storage_error(AtmError::validation(format!(
                    "failed to parse sqlite team: {error}"
                )))
            })?;
            let agent = agent.parse().map_err(|error| {
                storage_error(AtmError::validation(format!(
                    "failed to parse sqlite agent: {error}"
                )))
            })?;
            if scope.team != team || scope.agent != agent {
                return Err(ReadLaneError::UnauthorizedScope);
            }
            let state =
                load_state(&transaction, target, &team, &agent, key).map_err(storage_error)?;
            let envelope = apply_state(
                deserialize_json(&envelope_json, "sqlite message envelope")
                    .map_err(storage_error)?,
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
    close_reader_transaction(transaction, target).map_err(storage_error)?;
    Ok(message)
}

fn mailbox_member_exists(
    connection: &Connection,
    target: &SharedDbTarget,
    scope: &MailboxScope,
) -> Result<bool, AtmError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM team_roster WHERE team_name = ?1 AND agent_name = ?2);",
            params![scope.team.as_str(), scope.agent.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value != 0)
        .map_err(|error| sqlite_error(target, "failed to validate mailbox roster member", error))
}

fn load_seen_watermark(
    connection: &Connection,
    target: &SharedDbTarget,
    scope: &MailboxScope,
) -> Result<Option<IsoTimestamp>, AtmError> {
    connection
        .query_row(
            "SELECT watermark FROM mail_seen_watermarks WHERE team = ?1 AND agent = ?2;",
            params![scope.team.as_str(), scope.agent.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| sqlite_error(target, "failed to load mailbox seen watermark", error))?
        .map(|raw| {
            raw.parse().map_err(|error| {
                AtmError::validation(format!("failed to parse sqlite seen watermark: {error}"))
            })
        })
        .transpose()
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

fn count_messages(
    connection: &Connection,
    target: &SharedDbTarget,
    filters: &SearchFilters,
    group_by: Option<SearchCountGroupBy>,
) -> Result<Vec<SearchCount>, AtmError> {
    let compiled = compile_sql_filters_for(filters, SqlFilterAliases::MAILBOX);
    let pending_ack_at =
        "COALESCE(ms.pending_ack_at, json_extract(m.envelope_json, '$.pendingAckAt'))";
    let acknowledged_at =
        "COALESCE(ms.acknowledged_at, json_extract(m.envelope_json, '$.acknowledgedAt'))";
    let bucket = format!(
        "CASE WHEN {pending_ack_at} IS NOT NULL AND {acknowledged_at} IS NULL THEN 1 WHEN COALESCE(ms.read, json_extract(m.envelope_json, '$.read'), 0) = 0 THEN 0 ELSE 2 END"
    );
    let (group_expression, tag_join) = match group_by {
        Some(SearchCountGroupBy::Bucket) => (bucket, ""),
        Some(SearchCountGroupBy::FromAgent) => ("m.from_agent".to_owned(), ""),
        Some(SearchCountGroupBy::Tag) => {
            let tag_column = if filters.effective_tags.is_empty() {
                "m.tags_json"
            } else {
                "m.effective_tags_json"
            };
            (
                "tag.value".to_owned(),
                if tag_column == "m.tags_json" {
                    " JOIN json_each(COALESCE(m.tags_json, '[]')) tag"
                } else {
                    " JOIN json_each(COALESCE(m.effective_tags_json, '[]')) tag"
                },
            )
        }
        None => ("'all'".to_owned(), ""),
    };
    let sql = format!(
        "SELECT CAST({group_expression} AS TEXT), COUNT(*)
      FROM mail_messages m
      LEFT JOIN mail_message_states ms
        ON (ms.team, ms.agent, ms.message_key) = (m.team, m.agent, m.message_key){tag_join}
      LEFT JOIN message_templates t ON t.template_sha = m.template_sha
      WHERE 1 = 1 {}
      GROUP BY {group_expression}",
        compiled.clause
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|error| sqlite_error(target, "failed to prepare mailbox count query", error))?;
    let rows = statement
        .query_map(params_from_iter(compiled.parameters), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| sqlite_error(target, "failed to execute mailbox count query", error))?;
    let mut counts = rows
        .map(|row| {
            let (key, count) = row.map_err(|error| {
                sqlite_error(target, "failed to decode mailbox count row", error)
            })?;
            let key = match group_by {
                Some(SearchCountGroupBy::Bucket) => match key.as_str() {
                    "0" => atm_storage::SearchCountKey::Bucket(atm_storage::MailboxBucket::Unread),
                    "1" => {
                        atm_storage::SearchCountKey::Bucket(atm_storage::MailboxBucket::PendingAck)
                    }
                    "2" => atm_storage::SearchCountKey::Bucket(atm_storage::MailboxBucket::History),
                    _ => return Err(AtmError::mailbox_read("invalid mailbox count bucket")),
                },
                Some(SearchCountGroupBy::FromAgent) => {
                    atm_storage::SearchCountKey::FromAgent(key.parse().map_err(|error| {
                        AtmError::validation(format!("invalid counted sender: {error}"))
                    })?)
                }
                Some(SearchCountGroupBy::Tag) => atm_storage::SearchCountKey::Tag(key),
                None => {
                    return Ok(SearchCount {
                        key: None,
                        count: usize::try_from(count).map_err(|_| {
                            AtmError::validation("mailbox count exceeds usize range")
                        })?,
                    });
                }
            };
            Ok(SearchCount {
                key: Some(key),
                count: usize::try_from(count)
                    .map_err(|_| AtmError::validation("mailbox count exceeds usize range"))?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if group_by.is_none() {
        let count = counts.iter().map(|count| count.count).sum();
        counts = vec![SearchCount { key: None, count }];
    }
    Ok(counts)
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
