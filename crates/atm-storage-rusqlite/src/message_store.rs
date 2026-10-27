use super::{SharedDb, deserialize_json};
use atm_storage::AtmError;
use atm_storage::contract::{
    AcknowledgementCommit, AcknowledgementReplyBuilder, AcknowledgementSource, AsyncMessageStore,
    MailboxBucketCounts, MailboxScope, Message, MessageKey, MessageQuery, MessageStore,
};
use atm_storage::schema::MessageEnvelope;
use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct SqliteMessageStore {
    pub(crate) db: Arc<SharedDb>,
}

#[derive(Debug)]
pub(crate) struct SqliteNudgeTemplateOverrideStore {
    pub(crate) db: Arc<SharedDb>,
}

#[derive(Debug)]
pub(crate) struct SqlitePendingNudgeStore {
    pub(crate) db: Arc<SharedDb>,
}

#[derive(Debug)]
pub(crate) struct SqliteTaskStore {
    pub(crate) db: Arc<SharedDb>,
}

#[derive(Debug)]
pub(crate) struct SqlitePeerConfigStore {
    pub(crate) db: Arc<SharedDb>,
}

#[derive(Debug, Clone)]
struct StoredMailMessageState {
    read: bool,
    pending_ack_at: Option<IsoTimestamp>,
    acknowledged_at: Option<IsoTimestamp>,
    expires_at: Option<IsoTimestamp>,
}

impl SqliteMessageStore {
    pub(crate) fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }

    fn parse_optional_timestamp(
        raw: Option<String>,
        field_name: &str,
    ) -> Result<Option<IsoTimestamp>, AtmError> {
        raw.map(|value| value.parse::<IsoTimestamp>())
            .transpose()
            .map_err(|error| {
                AtmError::validation(format!(
                    "failed to parse mail-store {field_name} timestamp: {error}"
                ))
            })
    }

    fn load_message_state_row(
        db: &SharedDb,
        connection: &Connection,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<Option<StoredMailMessageState>, AtmError> {
        connection
            .query_row(
                "SELECT read, pending_ack_at, acknowledged_at, expires_at
                 FROM mail_message_states
                 WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                params![team.as_str(), agent.as_str(), message_key.as_ref()],
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
            .map_err(|error| db.error("failed to load sqlite message-state row", error))?
            .map(|(read, pending_ack_at, acknowledged_at, expires_at)| {
                Ok(StoredMailMessageState {
                    read: read != 0,
                    pending_ack_at: Self::parse_optional_timestamp(
                        pending_ack_at,
                        "pending_ack_at",
                    )?,
                    acknowledged_at: Self::parse_optional_timestamp(
                        acknowledged_at,
                        "acknowledged_at",
                    )?,
                    expires_at: Self::parse_optional_timestamp(expires_at, "expires_at")?,
                })
            })
            .transpose()
    }

    fn apply_loaded_state(
        mut envelope: MessageEnvelope,
        state: Option<&StoredMailMessageState>,
    ) -> MessageEnvelope {
        if let Some(state) = state {
            envelope.read = state.read;
            envelope.pending_ack_at = state.pending_ack_at;
            envelope.acknowledged_at = state.acknowledged_at;
            envelope.expires_at = state.expires_at;
        }
        envelope
    }
}

impl atm_storage::contract::sealed::Sealed for SqliteMessageStore {}

impl MessageStore for SqliteMessageStore {
    fn save_message(&self, message: &Message) -> Result<(), AtmError> {
        self.db.submit_upsert_message(message.clone()).map(|_| ())
    }

    fn save_message_if_absent(&self, message: &Message) -> Result<Option<Message>, AtmError> {
        if self.db.submit_upsert_message(message.clone())? {
            return Ok(None);
        }
        self.load_message(&message.message_key)?.ok_or_else(|| {
            AtmError::daemon_unavailable(
                "sqlite writer reported an existing message key but the retained record could not be loaded",
            )
        }).map(Some)
    }

    fn admit_message_with_provenance(
        &self,
        message: &Message,
        provenance: atm_storage::MessageWriteOrigin,
    ) -> Result<atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>, AtmError>
    {
        self.db
            .submit_message_admission(message.clone(), provenance)
    }

    fn save_messages_atomically(&self, messages: &[Message]) -> Result<(), AtmError> {
        self.db.submit_upsert_messages_atomically(messages.to_vec())
    }

    fn acknowledge_message_atomically(
        &self,
        source: &AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        self.db.submit_acknowledgement(source.clone(), builder)
    }

    fn load_message(&self, key: &MessageKey) -> Result<Option<Message>, AtmError> {
        let key = key.clone();
        let query_key = key.clone();
        let db = Arc::clone(&self.db);
        let record = self.db.read(move |connection| {
            let loaded = connection
                .query_row(
                    "SELECT team, agent, envelope_json
                     FROM mail_messages
                     WHERE message_key = ?1;",
                    params![query_key.as_ref()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| db.error("failed to load sqlite message", error))?;

            if let Some((team, agent, envelope_json)) = loaded {
                let team: TeamName = team.parse().map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse sqlite team for message {query_key}: {error}"
                    ))
                })?;
                let agent: AgentName = agent.parse().map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse sqlite agent for message {query_key}: {error}"
                    ))
                })?;
                let state =
                    Self::load_message_state_row(&db, connection, &team, &agent, &query_key)?;
                Ok(Some((team, agent, envelope_json, state)))
            } else {
                Ok(None)
            }
        })?;

        if let Some((team, agent, envelope_json, state)) = record {
            let envelope = deserialize_json(&envelope_json, "sqlite message envelope")?;
            let envelope = Self::apply_loaded_state(envelope, state.as_ref());
            Ok(Some(Message {
                team,
                agent,
                message_key: key.clone(),
                envelope,
            }))
        } else {
            Ok(None)
        }
    }

    fn list_messages(&self, query: &MessageQuery) -> Result<Vec<Message>, AtmError> {
        let query = query.clone();
        let db = Arc::clone(&self.db);
        self.db.read(move |connection| {
            let limit = query
                .limit
                // The shared contract accepts usize, but SQLite LIMIT is i64.
                // Saturating oversize fuzz/test inputs at i64::MAX keeps the
                // query bounded without inventing a second caller-visible cap.
                .map(|value| i64::try_from(value).unwrap_or(i64::MAX))
                .unwrap_or(-1);
            let mut statement = connection
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
                       AND (
                            mail_message_states.expires_at IS NULL
                            OR mail_message_states.expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                       )
                     ORDER BY mail_messages.message_at DESC, mail_messages.message_key DESC
                     LIMIT ?5;",
                )
                .map_err(|error| db.error("failed to prepare sqlite message list query", error))?;
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
                .map_err(|error| db.error("failed to execute sqlite message list query", error))?;

            let mut messages = Vec::new();
            for row in rows {
                let (message_key, envelope_json) =
                    row.map_err(|error| db.error("failed to decode sqlite message row", error))?;
                let message_key = MessageKey::new(message_key).map_err(|error| {
                    AtmError::validation(format!(
                        "failed to parse sqlite message key during list: {error}"
                    ))

                })?;
                let state =
                    Self::load_message_state_row(
                        &db,
                        connection,
                        &query.team,
                        &query.agent,
                        &message_key,
                    )?;
                let envelope: MessageEnvelope =
                    deserialize_json(&envelope_json, "sqlite message envelope")?;
                let envelope = Self::apply_loaded_state(envelope, state.as_ref());
                messages.push(Message {
                    team: query.team.clone(),
                    agent: query.agent.clone(),
                    message_key,
                    envelope,
                });
            }
            Ok(messages)
        })
    }

    fn mailbox_bucket_counts(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<Option<MailboxBucketCounts>, AtmError> {
        let team = team.clone();
        let agent = agent.clone();
        let db = Arc::clone(&self.db);
        self.db.read(move |connection| {
            let sql = "WITH visible AS (
                    SELECT
                        mail_messages.message_id,
                        mail_messages.parent_message_id,
                        COALESCE(
                            mail_message_states.read,
                            json_extract(mail_messages.envelope_json, '$.read'),
                            0
                        ) AS is_read,
                        mail_message_states.pending_ack_at,
                        mail_message_states.acknowledged_at,
                        mail_message_states.expires_at
                    FROM mail_messages
                    LEFT JOIN mail_message_states
                      ON mail_message_states.team = mail_messages.team
                     AND mail_message_states.agent = mail_messages.agent
                     AND mail_message_states.message_key = mail_messages.message_key
                    WHERE mail_messages.team = ?1
                      AND mail_messages.agent = ?2
                      AND mail_message_states.deleted_at IS NULL
                      AND (
                           mail_message_states.expires_at IS NULL
                           OR mail_message_states.expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                      )
                 ), terminal AS (
                    SELECT visible.*
                    FROM visible
                    WHERE visible.message_id IS NULL
                       OR NOT EXISTS (
                            SELECT 1
                            FROM visible AS successor
                            WHERE successor.parent_message_id = visible.message_id
                       )
                 ), displayable AS (
                    SELECT *
                    FROM terminal
                    WHERE expires_at IS NULL OR is_read = 0
                 )
                 SELECT
                    COALESCE(SUM(CASE
                        WHEN pending_ack_at IS NOT NULL AND acknowledged_at IS NULL THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE
                        WHEN NOT (pending_ack_at IS NOT NULL AND acknowledged_at IS NULL)
                         AND is_read = 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE
                        WHEN NOT (pending_ack_at IS NOT NULL AND acknowledged_at IS NULL)
                         AND is_read != 0 THEN 1 ELSE 0 END), 0)
                 FROM displayable";
            let (pending_ack, unread, history): (i64, i64, i64) = connection
                .query_row(sql, params![team.as_str(), agent.as_str()], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .map_err(|error| {
                    db.error("failed to aggregate sqlite mailbox bucket counts", error)
                })?;
            Ok(Some(MailboxBucketCounts {
                unread: usize::try_from(unread).map_err(|_| {
                    AtmError::validation("sqlite unread mailbox count exceeds usize range")
                })?,
                pending_ack: usize::try_from(pending_ack).map_err(|_| {
                    AtmError::validation("sqlite pending-ack mailbox count exceeds usize range")
                })?,
                history: usize::try_from(history).map_err(|_| {
                    AtmError::validation("sqlite history mailbox count exceeds usize range")
                })?,
            }))
        })
    }

    fn delete_message(&self, key: &MessageKey) -> Result<(), AtmError> {
        self.delete_message_with_origin(key, atm_storage::MessageWriteOrigin::Local)
    }

    fn delete_message_with_origin(
        &self,
        key: &MessageKey,
        origin: atm_storage::MessageWriteOrigin,
    ) -> Result<(), AtmError> {
        if matches!(origin, atm_storage::MessageWriteOrigin::Peer) {
            return Err(AtmError::validation(
                "peer provenance cannot delete local messages",
            ));
        }
        self.db.submit_delete_message(key.clone(), origin)
    }
}

#[async_trait::async_trait]
impl AsyncMessageStore for SqliteMessageStore {
    async fn apply_read_display_state_async(
        &self,
        scope: MailboxScope,
        message_ids: Vec<MessageKey>,
        seen_watermark: Option<IsoTimestamp>,
    ) -> Result<(), AtmError> {
        self.apply_read_display_state_with_origin_async(
            scope,
            message_ids,
            seen_watermark,
            atm_storage::MessageWriteOrigin::Local,
        )
        .await
    }

    async fn apply_read_display_state_with_origin_async(
        &self,
        scope: MailboxScope,
        message_ids: Vec<MessageKey>,
        seen_watermark: Option<IsoTimestamp>,
        origin: atm_storage::MessageWriteOrigin,
    ) -> Result<(), AtmError> {
        if matches!(origin, atm_storage::MessageWriteOrigin::Peer) {
            return Err(AtmError::validation(
                "peer provenance cannot update local mailbox display state",
            ));
        }
        self.db
            .submit_read_display_state_async(scope, message_ids, seen_watermark, origin)
            .await
    }

    async fn save_message_if_absent_async(
        &self,
        message: Message,
    ) -> Result<Option<Message>, AtmError> {
        self.db.submit_upsert_message_async(message).await
    }

    async fn admit_message_with_provenance_async(
        &self,
        message: Message,
        provenance: atm_storage::MessageWriteOrigin,
    ) -> Result<atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>, AtmError>
    {
        self.db
            .submit_message_admission_async(message, provenance)
            .await
    }

    async fn admit_template_message_async(
        &self,
        admission: atm_storage::TemplateMessageAdmission,
    ) -> Result<atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>, AtmError>
    {
        self.db
            .submit_template_message_admission_async(admission)
            .await
    }

    async fn acknowledge_message_atomically_async(
        &self,
        source: AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<atm_storage::CommittedTaskWrite<AcknowledgementCommit>, AtmError> {
        self.db
            .submit_acknowledgement_async(source, builder, atm_storage::MessageWriteOrigin::Local)
            .await
    }

    async fn acknowledge_message_atomically_with_origin_async(
        &self,
        source: AcknowledgementSource,
        builder: Arc<dyn AcknowledgementReplyBuilder>,
        origin: atm_storage::MessageWriteOrigin,
    ) -> Result<atm_storage::CommittedTaskWrite<AcknowledgementCommit>, AtmError> {
        if matches!(origin, atm_storage::MessageWriteOrigin::Peer) {
            return Err(AtmError::validation(
                "peer provenance cannot acknowledge local messages",
            ));
        }
        self.db
            .submit_acknowledgement_async(source, builder, origin)
            .await
    }
}
