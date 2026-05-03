use atm_core::mail_store::{
    AckStateRecord, ImportedMessageState, IngestRecord, MailStore, MailStoreHealth,
    MessageSourceKind, PendingExportRecord, StoredMessageRecord, VisibilityStateRecord,
};
use atm_core::schema::{AtmMessageId, LegacyMessageId};
use atm_core::store::{
    InsertOutcome, MessageKey, SourceFingerprint, StoreDuplicateIdentity, StoreError,
};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use rusqlite::{Connection, OptionalExtension};

use crate::task::upsert_task_row;
use crate::{
    RusqliteStore, classify_store_error, invalid_store_data, parse_optional, parse_required,
    sqlite_error_detail, table_exists,
};

const MESSAGE_SELECT_COLUMNS: &str = "SELECT message_key, team_name, recipient_agent, sender_display, sender_canonical, sender_team, body, summary, created_at, source_kind, legacy_message_id, atm_message_id, raw_metadata_json FROM messages";

#[derive(Debug)]
struct RawMessageRow {
    message_key: String,
    team_name: String,
    recipient_agent: String,
    sender_display: String,
    sender_canonical: Option<String>,
    sender_team: Option<String>,
    body: String,
    summary: Option<String>,
    created_at: String,
    source_kind: String,
    legacy_message_id: Option<String>,
    atm_message_id: Option<String>,
    raw_metadata_json: Option<String>,
}

#[derive(Debug)]
struct RawAckStateRow {
    message_key: String,
    pending_ack_at: Option<String>,
    acknowledged_at: Option<String>,
    ack_reply_message_key: Option<String>,
    ack_reply_team: Option<String>,
    ack_reply_agent: Option<String>,
}

#[derive(Debug)]
struct RawVisibilityRow {
    message_key: String,
    read_at: Option<String>,
    cleared_at: Option<String>,
}

#[derive(Debug)]
struct RawIngestRow {
    team_name: String,
    recipient_agent: String,
    source_path: String,
    source_fingerprint: String,
    message_key: String,
    imported_at: String,
}

#[derive(Debug)]
struct RawPendingExportRow {
    message_key: String,
    export_target_team: String,
    export_target_agent: String,
    recipient_pane_id: Option<String>,
    attempt_count: i64,
    next_attempt_at: String,
    expires_at: String,
}

impl MailStore for RusqliteStore {
    fn insert_message(
        &self,
        message: &StoredMessageRecord,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError> {
        let connection = self.lock_connection()?;
        match insert_message_row(&connection, message) {
            Ok(()) => Ok(InsertOutcome::Inserted(message.clone())),
            Err(error) => match classify_message_duplicate(&error, message) {
                Some(identity) => Ok(InsertOutcome::Duplicate(identity)),
                None => Err(classify_store_error(error, "failed to insert message row")),
            },
        }
    }

    fn insert_message_batch(&self, messages: &[StoredMessageRecord]) -> Result<(), StoreError> {
        self.with_transaction(|transaction| {
            for message in messages {
                insert_message_row(transaction, message).map_err(|error| {
                    classify_store_error(error, "failed to insert message batch row")
                })?;
            }
            Ok(())
        })
    }

    fn load_message(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<StoredMessageRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                &format!("{MESSAGE_SELECT_COLUMNS} WHERE message_key = ?1"),
                [message_key.as_str()],
                |row| {
                    Ok(RawMessageRow {
                        message_key: row.get(0)?,
                        team_name: row.get(1)?,
                        recipient_agent: row.get(2)?,
                        sender_display: row.get(3)?,
                        sender_canonical: row.get(4)?,
                        sender_team: row.get(5)?,
                        body: row.get(6)?,
                        summary: row.get(7)?,
                        created_at: row.get(8)?,
                        source_kind: row.get(9)?,
                        legacy_message_id: row.get(10)?,
                        atm_message_id: row.get(11)?,
                        raw_metadata_json: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load message by key"))?;
        raw.map(convert_message_row).transpose()
    }

    fn load_message_by_legacy_id(
        &self,
        legacy_message_id: &LegacyMessageId,
    ) -> Result<Option<StoredMessageRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                &format!("{MESSAGE_SELECT_COLUMNS} WHERE legacy_message_id = ?1"),
                [legacy_message_id.to_string()],
                |row| {
                    Ok(RawMessageRow {
                        message_key: row.get(0)?,
                        team_name: row.get(1)?,
                        recipient_agent: row.get(2)?,
                        sender_display: row.get(3)?,
                        sender_canonical: row.get(4)?,
                        sender_team: row.get(5)?,
                        body: row.get(6)?,
                        summary: row.get(7)?,
                        created_at: row.get(8)?,
                        source_kind: row.get(9)?,
                        legacy_message_id: row.get(10)?,
                        atm_message_id: row.get(11)?,
                        raw_metadata_json: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load message by legacy id"))?;
        raw.map(convert_message_row).transpose()
    }

    fn load_message_by_atm_id(
        &self,
        atm_message_id: &AtmMessageId,
    ) -> Result<Option<StoredMessageRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                &format!("{MESSAGE_SELECT_COLUMNS} WHERE atm_message_id = ?1"),
                [atm_message_id.to_string()],
                |row| {
                    Ok(RawMessageRow {
                        message_key: row.get(0)?,
                        team_name: row.get(1)?,
                        recipient_agent: row.get(2)?,
                        sender_display: row.get(3)?,
                        sender_canonical: row.get(4)?,
                        sender_team: row.get(5)?,
                        body: row.get(6)?,
                        summary: row.get(7)?,
                        created_at: row.get(8)?,
                        source_kind: row.get(9)?,
                        legacy_message_id: row.get(10)?,
                        atm_message_id: row.get(11)?,
                        raw_metadata_json: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load message by atm id"))?;
        raw.map(convert_message_row).transpose()
    }

    fn upsert_ack_state(&self, ack_state: &AckStateRecord) -> Result<AckStateRecord, StoreError> {
        let connection = self.lock_connection()?;
        upsert_ack_state_row(&connection, ack_state)
            .map_err(|error| classify_store_error(error, "failed to upsert ack state"))?;
        Ok(ack_state.clone())
    }

    fn load_ack_state(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<AckStateRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                "SELECT message_key, pending_ack_at, acknowledged_at, ack_reply_message_key, ack_reply_team, ack_reply_agent FROM ack_state WHERE message_key = ?1",
                [message_key.as_str()],
                |row| {
                    Ok(RawAckStateRow {
                        message_key: row.get(0)?,
                        pending_ack_at: row.get(1)?,
                        acknowledged_at: row.get(2)?,
                        ack_reply_message_key: row.get(3)?,
                        ack_reply_team: row.get(4)?,
                        ack_reply_agent: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load ack state"))?;
        raw.map(convert_ack_state_row).transpose()
    }

    fn upsert_visibility(
        &self,
        visibility: &VisibilityStateRecord,
    ) -> Result<VisibilityStateRecord, StoreError> {
        let connection = self.lock_connection()?;
        upsert_visibility_row(&connection, visibility)
            .map_err(|error| classify_store_error(error, "failed to upsert visibility state"))?;
        Ok(visibility.clone())
    }

    fn load_visibility(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<VisibilityStateRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                "SELECT message_key, read_at, cleared_at FROM message_visibility WHERE message_key = ?1",
                [message_key.as_str()],
                |row| {
                    Ok(RawVisibilityRow {
                        message_key: row.get(0)?,
                        read_at: row.get(1)?,
                        cleared_at: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load visibility state"))?;
        raw.map(convert_visibility_row).transpose()
    }

    fn record_ingest(
        &self,
        ingest_record: &IngestRecord,
    ) -> Result<InsertOutcome<IngestRecord>, StoreError> {
        let connection = self.lock_connection()?;
        match insert_ingest_row(&connection, ingest_record) {
            Ok(_) => Ok(InsertOutcome::Inserted(ingest_record.clone())),
            Err(error) => match classify_ingest_duplicate(&error, ingest_record) {
                Some(identity) => Ok(InsertOutcome::Duplicate(identity)),
                None => Err(classify_store_error(error, "failed to record ingest row")),
            },
        }
    }

    fn insert_message_with_ingest(
        &self,
        message: &StoredMessageRecord,
        ingest_record: &IngestRecord,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError> {
        self.insert_message_with_ingest_state(
            message,
            ingest_record,
            &ImportedMessageState::default(),
        )
    }

    fn insert_message_with_ingest_state(
        &self,
        message: &StoredMessageRecord,
        ingest_record: &IngestRecord,
        state: &ImportedMessageState,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError> {
        self.with_transaction(|transaction| {
            match insert_message_row(transaction, message) {
                Ok(()) => {}
                Err(error) => {
                    if let Some(identity) = classify_message_duplicate(&error, message) {
                        return Ok(InsertOutcome::Duplicate(identity));
                    }
                    return Err(classify_store_error(
                        error,
                        "failed to insert imported mailbox row",
                    ));
                }
            }
            match insert_ingest_row(transaction, ingest_record) {
                Ok(()) => {}
                Err(error) => {
                    return match classify_ingest_duplicate(&error, ingest_record) {
                        Some(identity) => Err(StoreError::constraint(format!(
                            "ingest fingerprint duplicated during imported mailbox batch write: {identity:?}"
                        ))),
                        None => Err(classify_store_error(
                            error,
                            "failed to record inbox ingest fingerprint",
                        )),
                    };
                }
            }
            if let Some(ack_state) = state.ack_state.as_ref() {
                upsert_ack_state_row(transaction, ack_state).map_err(|error| {
                    classify_store_error(error, "failed to upsert imported ack state")
                })?;
            }
            if let Some(visibility) = state.visibility.as_ref() {
                upsert_visibility_row(transaction, visibility).map_err(|error| {
                    classify_store_error(error, "failed to upsert imported visibility state")
                })?;
            }
            if let Some(task) = state.task.as_ref() {
                upsert_task_row(transaction, task).map_err(|error| {
                    classify_store_error(error, "failed to upsert imported task row")
                })?;
            }
            Ok(InsertOutcome::Inserted(message.clone()))
        })
    }

    fn load_ingest(
        &self,
        team_name: &TeamName,
        recipient_agent: &AgentName,
        source_fingerprint: &SourceFingerprint,
    ) -> Result<Option<IngestRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                "SELECT team_name, recipient_agent, source_path, source_fingerprint, message_key, imported_at FROM inbox_ingest WHERE team_name = ?1 AND recipient_agent = ?2 AND source_fingerprint = ?3",
                (team_name.as_str(), recipient_agent.as_str(), source_fingerprint.as_str()),
                |row| {
                    Ok(RawIngestRow {
                        team_name: row.get(0)?,
                        recipient_agent: row.get(1)?,
                        source_path: row.get(2)?,
                        source_fingerprint: row.get(3)?,
                        message_key: row.get(4)?,
                        imported_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load ingest row"))?;
        raw.map(convert_ingest_row).transpose()
    }

    fn record_pending_export(&self, export: &PendingExportRecord) -> Result<(), StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                r#"
                INSERT INTO pending_exports (
                    message_key,
                    export_target_team,
                    export_target_agent,
                    recipient_pane_id,
                    attempt_count,
                    next_attempt_at,
                    expires_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(message_key) DO UPDATE SET
                    export_target_team = excluded.export_target_team,
                    export_target_agent = excluded.export_target_agent,
                    recipient_pane_id = excluded.recipient_pane_id,
                    attempt_count = excluded.attempt_count,
                    next_attempt_at = excluded.next_attempt_at,
                    expires_at = excluded.expires_at
                "#,
                (
                    export.message_key.as_str(),
                    export.export_target_team.as_str(),
                    export.export_target_agent.as_str(),
                    export.recipient_pane_id.as_ref().map(ToString::to_string),
                    i64::from(export.attempt_count),
                    export.next_attempt_at.to_string(),
                    export.expires_at.to_string(),
                ),
            )
            .map_err(|error| classify_store_error(error, "failed to record pending export"))?;
        Ok(())
    }

    fn remove_pending_export(&self, message_key: &MessageKey) -> Result<(), StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                "DELETE FROM pending_exports WHERE message_key = ?1",
                [message_key.as_str()],
            )
            .map_err(|error| classify_store_error(error, "failed to remove pending export"))?;
        Ok(())
    }

    fn load_due_pending_exports(
        &self,
        now: &IsoTimestamp,
        limit: usize,
    ) -> Result<Vec<PendingExportRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT message_key, export_target_team, export_target_agent, recipient_pane_id, attempt_count, next_attempt_at, expires_at FROM pending_exports WHERE next_attempt_at <= ?1 ORDER BY next_attempt_at, message_key LIMIT ?2",
            )
            .map_err(|error| classify_store_error(error, "failed to prepare pending export query"))?;
        let rows = statement
            .query_map((now.to_string(), limit as i64), |row| {
                Ok(RawPendingExportRow {
                    message_key: row.get(0)?,
                    export_target_team: row.get(1)?,
                    export_target_agent: row.get(2)?,
                    recipient_pane_id: row.get(3)?,
                    attempt_count: row.get(4)?,
                    next_attempt_at: row.get(5)?,
                    expires_at: row.get(6)?,
                })
            })
            .map_err(|error| classify_store_error(error, "failed to query pending export rows"))?;

        let mut records = Vec::new();
        for row in rows {
            let raw = row.map_err(|error| {
                classify_store_error(error, "failed to read pending export row")
            })?;
            records.push(convert_pending_export_row(raw)?);
        }
        Ok(records)
    }

    fn remove_expired_pending_exports(&self, now: &IsoTimestamp) -> Result<u64, StoreError> {
        let connection = self.lock_connection()?;
        let removed = connection
            .execute(
                "DELETE FROM pending_exports WHERE expires_at <= ?1",
                [now.to_string()],
            )
            .map_err(|error| {
                classify_store_error(error, "failed to remove expired pending exports")
            })?;
        Ok(removed as u64)
    }

    fn mail_health(&self) -> Result<MailStoreHealth, StoreError> {
        let connection = self.lock_connection()?;
        Ok(MailStoreHealth {
            messages_ready: table_exists(&connection, "messages")?,
            inbox_ingest_ready: table_exists(&connection, "inbox_ingest")?,
            ack_state_ready: table_exists(&connection, "ack_state")?,
            message_visibility_ready: table_exists(&connection, "message_visibility")?,
            pending_exports_ready: table_exists(&connection, "pending_exports")?,
        })
    }
}

pub(crate) fn insert_message_row(
    connection: &Connection,
    message: &StoredMessageRecord,
) -> rusqlite::Result<()> {
    connection.execute(
        r#"
        INSERT INTO messages (
            message_key,
            team_name,
            recipient_agent,
            sender_display,
            sender_canonical,
            sender_team,
            body,
            summary,
            created_at,
            source_kind,
            legacy_message_id,
            atm_message_id,
            raw_metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        "#,
        (
            message.message_key.as_str(),
            message.team_name.as_str(),
            message.recipient_agent.as_str(),
            message.sender_display.as_str(),
            message.sender_canonical.as_ref().map(ToString::to_string),
            message.sender_team.as_ref().map(ToString::to_string),
            message.body.as_str(),
            message.summary.as_deref(),
            message.created_at.to_string(),
            message.source_kind.as_str(),
            message.legacy_message_id.as_ref().map(ToString::to_string),
            message.atm_message_id.as_ref().map(ToString::to_string),
            message.raw_metadata_json.as_deref(),
        ),
    )?;
    Ok(())
}

pub(crate) fn upsert_ack_state_row(
    connection: &Connection,
    ack_state: &AckStateRecord,
) -> rusqlite::Result<()> {
    connection.execute(
        r#"
        INSERT INTO ack_state (
            message_key,
            pending_ack_at,
            acknowledged_at,
            ack_reply_message_key,
            ack_reply_team,
            ack_reply_agent
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(message_key) DO UPDATE SET
            pending_ack_at = excluded.pending_ack_at,
            acknowledged_at = excluded.acknowledged_at,
            ack_reply_message_key = excluded.ack_reply_message_key,
            ack_reply_team = excluded.ack_reply_team,
            ack_reply_agent = excluded.ack_reply_agent
        "#,
        (
            ack_state.message_key.as_str(),
            ack_state.pending_ack_at.as_ref().map(ToString::to_string),
            ack_state.acknowledged_at.as_ref().map(ToString::to_string),
            ack_state
                .ack_reply_message_key
                .as_ref()
                .map(|value| value.to_string()),
            ack_state.ack_reply_team.as_ref().map(ToString::to_string),
            ack_state.ack_reply_agent.as_ref().map(ToString::to_string),
        ),
    )?;
    Ok(())
}

pub(crate) fn upsert_visibility_row(
    connection: &Connection,
    visibility: &VisibilityStateRecord,
) -> rusqlite::Result<()> {
    connection.execute(
        r#"
        INSERT INTO message_visibility (message_key, read_at, cleared_at)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(message_key) DO UPDATE SET
            read_at = excluded.read_at,
            cleared_at = excluded.cleared_at
        "#,
        (
            visibility.message_key.as_str(),
            visibility.read_at.as_ref().map(ToString::to_string),
            visibility.cleared_at.as_ref().map(ToString::to_string),
        ),
    )?;
    Ok(())
}

pub(crate) fn classify_message_duplicate(
    error: &rusqlite::Error,
    message: &StoredMessageRecord,
) -> Option<StoreDuplicateIdentity> {
    let detail = sqlite_error_detail(error)?;
    if detail.contains("messages.message_key") {
        return Some(StoreDuplicateIdentity::MessageKey(
            message.message_key.clone(),
        ));
    }
    if detail.contains("messages.legacy_message_id") {
        return message
            .legacy_message_id
            .map(StoreDuplicateIdentity::LegacyMessageId);
    }
    if detail.contains("messages.atm_message_id") {
        return message
            .atm_message_id
            .map(StoreDuplicateIdentity::AtmMessageId);
    }
    None
}

fn classify_ingest_duplicate(
    error: &rusqlite::Error,
    ingest: &IngestRecord,
) -> Option<StoreDuplicateIdentity> {
    let detail = sqlite_error_detail(error)?;
    if detail.contains("inbox_ingest.team_name")
        || detail.contains("inbox_ingest.recipient_agent")
        || detail.contains("inbox_ingest.source_fingerprint")
    {
        return Some(StoreDuplicateIdentity::IngestFingerprint {
            team_name: ingest.team_name.clone(),
            recipient_agent: ingest.recipient_agent.clone(),
            source_fingerprint: ingest.source_fingerprint.clone(),
        });
    }
    None
}

fn insert_ingest_row(
    connection: &Connection,
    ingest_record: &IngestRecord,
) -> rusqlite::Result<()> {
    connection.execute(
        r#"
        INSERT INTO inbox_ingest (
            team_name,
            recipient_agent,
            source_path,
            source_fingerprint,
            message_key,
            imported_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        (
            ingest_record.team_name.as_str(),
            ingest_record.recipient_agent.as_str(),
            ingest_record.source_path.display().to_string(),
            ingest_record.source_fingerprint.as_str(),
            ingest_record.message_key.as_str(),
            ingest_record.imported_at.to_string(),
        ),
    )?;
    Ok(())
}

fn convert_message_row(raw: RawMessageRow) -> Result<StoredMessageRecord, StoreError> {
    Ok(StoredMessageRecord {
        message_key: parse_required(raw.message_key, "message_key")?,
        team_name: parse_required(raw.team_name, "team_name")?,
        recipient_agent: parse_required(raw.recipient_agent, "recipient_agent")?,
        sender_display: raw.sender_display,
        sender_canonical: parse_optional(raw.sender_canonical, "sender_canonical")?,
        sender_team: parse_optional(raw.sender_team, "sender_team")?,
        body: raw.body,
        summary: raw.summary,
        created_at: parse_required(raw.created_at, "created_at")?,
        source_kind: parse_message_source_kind(raw.source_kind)?,
        legacy_message_id: parse_optional(raw.legacy_message_id, "legacy_message_id")?,
        atm_message_id: parse_optional(raw.atm_message_id, "atm_message_id")?,
        raw_metadata_json: raw.raw_metadata_json,
    })
}

fn convert_ack_state_row(raw: RawAckStateRow) -> Result<AckStateRecord, StoreError> {
    Ok(AckStateRecord {
        message_key: parse_required(raw.message_key, "message_key")?,
        pending_ack_at: parse_optional(raw.pending_ack_at, "pending_ack_at")?,
        acknowledged_at: parse_optional(raw.acknowledged_at, "acknowledged_at")?,
        ack_reply_message_key: parse_optional(raw.ack_reply_message_key, "ack_reply_message_key")?,
        ack_reply_team: parse_optional(raw.ack_reply_team, "ack_reply_team")?,
        ack_reply_agent: parse_optional(raw.ack_reply_agent, "ack_reply_agent")?,
    })
}

fn convert_visibility_row(raw: RawVisibilityRow) -> Result<VisibilityStateRecord, StoreError> {
    Ok(VisibilityStateRecord {
        message_key: parse_required(raw.message_key, "message_key")?,
        read_at: parse_optional(raw.read_at, "read_at")?,
        cleared_at: parse_optional(raw.cleared_at, "cleared_at")?,
    })
}

fn convert_ingest_row(raw: RawIngestRow) -> Result<IngestRecord, StoreError> {
    Ok(IngestRecord {
        team_name: parse_required(raw.team_name, "team_name")?,
        recipient_agent: parse_required(raw.recipient_agent, "recipient_agent")?,
        source_path: std::path::PathBuf::from(raw.source_path),
        source_fingerprint: parse_required(raw.source_fingerprint, "source_fingerprint")?,
        message_key: parse_required(raw.message_key, "message_key")?,
        imported_at: parse_required(raw.imported_at, "imported_at")?,
    })
}

fn convert_pending_export_row(raw: RawPendingExportRow) -> Result<PendingExportRecord, StoreError> {
    Ok(PendingExportRecord {
        message_key: parse_required(raw.message_key, "message_key")?,
        export_target_team: parse_required(raw.export_target_team, "export_target_team")?,
        export_target_agent: parse_required(raw.export_target_agent, "export_target_agent")?,
        recipient_pane_id: parse_optional(raw.recipient_pane_id, "recipient_pane_id")?,
        attempt_count: u32::try_from(raw.attempt_count)
            .map_err(|error| invalid_store_data("attempt_count", error))?,
        next_attempt_at: parse_required(raw.next_attempt_at, "next_attempt_at")?,
        expires_at: parse_required(raw.expires_at, "expires_at")?,
    })
}

fn parse_message_source_kind(value: String) -> Result<MessageSourceKind, StoreError> {
    match value.as_str() {
        "atm" => Ok(MessageSourceKind::Atm),
        "legacy" => Ok(MessageSourceKind::Legacy),
        "external" => Ok(MessageSourceKind::External),
        _ => Err(invalid_store_data("source_kind", "unsupported source kind")),
    }
}
