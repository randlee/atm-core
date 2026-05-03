use std::path::Path;

use atm_core::mail_store::{
    AckStateRecord, IngestRecord, MailStore, MessageSourceKind, PendingExportRecord,
    VisibilityStateRecord,
};
use atm_core::roster_store::{
    PidUpdate, RosterMemberRecord, RosterRole, RosterStore, TransportKind,
};
use atm_core::schema::{AtmMessageId, LegacyMessageId};
use atm_core::store::{
    InsertOutcome, MessageKey, ProcessId, StoreBoundary, StoreDuplicateIdentity, StoreErrorKind,
};
use atm_core::task_store::{TaskRecord, TaskStatus, TaskStore};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use tempfile::TempDir;

use crate::{
    RusqliteStore, SCHEMA_VERSION, query_foreign_keys, query_journal_mode, query_user_version,
};

fn team() -> TeamName {
    "atm-dev".parse().expect("team")
}

fn agent(name: &str) -> AgentName {
    name.parse().expect("agent")
}

fn message_at(index: u8) -> atm_core::mail_store::StoredMessageRecord {
    let legacy_message_id: LegacyMessageId =
        format!("00000000-0000-4000-8000-0000000000{index:02}")
            .parse()
            .expect("legacy id");
    let atm_message_id: AtmMessageId = format!("01ARZ3NDEKTSV4RRFFQ69G5F{index:02}")
        .parse()
        .expect("atm id");
    atm_core::mail_store::StoredMessageRecord {
        message_key: MessageKey::from_atm_message_id(atm_message_id),
        team_name: team(),
        recipient_agent: agent("team-lead"),
        sender_display: "arch-ctm".to_string(),
        sender_canonical: Some(agent("arch-ctm")),
        sender_team: Some(team()),
        body: format!("body-{index}"),
        summary: Some(format!("summary-{index}")),
        created_at: "2026-05-02T20:00:00Z".parse().expect("timestamp"),
        source_kind: MessageSourceKind::Atm,
        legacy_message_id: Some(legacy_message_id),
        atm_message_id: Some(atm_message_id),
        raw_metadata_json: Some("{\"atm\":true}".to_string()),
    }
}

fn ingest_record(root: &Path, message_key: &MessageKey) -> IngestRecord {
    IngestRecord {
        team_name: team(),
        recipient_agent: agent("team-lead"),
        source_path: root.join("team-lead.json"),
        source_fingerprint: "sha256-team-lead-001".parse().expect("fingerprint"),
        message_key: message_key.clone(),
        imported_at: "2026-05-02T20:00:05Z".parse().expect("timestamp"),
    }
}

fn roster_member(name: &str, pane_id: Option<&str>, pid: Option<i64>) -> RosterMemberRecord {
    RosterMemberRecord {
        team_name: team(),
        agent_name: agent(name),
        role: "member".parse::<RosterRole>().expect("role"),
        transport_kind: "claude".parse::<TransportKind>().expect("transport"),
        host_name: "local-host".parse().expect("host"),
        recipient_pane_id: pane_id.map(|value| value.parse().expect("pane")),
        pid: pid.map(|value| ProcessId::new(value).expect("pid")),
        metadata_json: Some("{\"provider\":\"claude\"}".to_string()),
    }
}

fn pending_export(
    message_key: &MessageKey,
    attempt_count: u32,
    next: &str,
    expires: &str,
) -> PendingExportRecord {
    PendingExportRecord {
        message_key: message_key.clone(),
        export_target_team: team(),
        export_target_agent: agent("team-lead"),
        recipient_pane_id: Some("%1".parse().expect("pane")),
        attempt_count,
        next_attempt_at: next.parse().expect("next attempt"),
        expires_at: expires.parse().expect("expiry"),
    }
}

fn table_columns(store: &RusqliteStore, table_name: &str) -> Vec<(String, String, bool)> {
    let connection = store.lock_connection().expect("lock");
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .expect("pragma table_info");
    let mut rows = statement.query([]).expect("query table_info");
    let mut columns = Vec::new();
    while let Some(row) = rows.next().expect("next row") {
        let name: String = row.get(1).expect("name");
        let kind: String = row.get(2).expect("type");
        let not_null: i64 = row.get(3).expect("not null");
        columns.push((name, kind, not_null == 1));
    }
    columns
}

#[test]
fn opens_under_team_state_mail_db() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_for_team_home(tempdir.path(), &team()).expect("open store");
    assert_eq!(
        store.database_path,
        tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("atm-dev")
            .join(".atm-state")
            .join("mail.db")
    );
}

#[test]
fn bootstrap_is_idempotent_and_reports_wal_without_schema_drift() {
    let tempdir = TempDir::new().expect("tempdir");
    let db_path = tempdir.path().join("mail.db");

    let first = RusqliteStore::open_path(&db_path).expect("first bootstrap");
    let first_columns = table_columns(&first, "messages");
    let first_pending_export_columns = table_columns(&first, "pending_exports");
    let second = RusqliteStore::open_path(&db_path).expect("second bootstrap");

    let first_report = first.bootstrap_report().expect("first report");
    let second_report = second.bootstrap_report().expect("second report");
    let second_columns = table_columns(&second, "messages");
    let second_pending_export_columns = table_columns(&second, "pending_exports");

    assert_eq!(first_report.schema_version, SCHEMA_VERSION);
    assert_eq!(second_report.schema_version, SCHEMA_VERSION);
    assert!(first_report.wal_enabled);
    assert!(first_report.foreign_keys_enabled);
    assert_eq!(first_columns, second_columns);
    assert_eq!(first_pending_export_columns, second_pending_export_columns);
}

#[test]
fn insert_message_enforces_unique_identities() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");

    let first = message_at(1);
    let second_same_key_atm = AtmMessageId::new();
    let second_same_key = atm_core::mail_store::StoredMessageRecord {
        body: "updated".to_string(),
        legacy_message_id: Some(
            "00000000-0000-4000-8000-000000000099"
                .parse()
                .expect("legacy id"),
        ),
        atm_message_id: Some(second_same_key_atm),
        message_key: first.message_key.clone(),
        ..first.clone()
    };
    let second_atm_message_id = AtmMessageId::new();
    let second_same_legacy = atm_core::mail_store::StoredMessageRecord {
        message_key: MessageKey::from_atm_message_id(second_atm_message_id),
        atm_message_id: Some(second_atm_message_id),
        ..first.clone()
    };

    match store.insert_message(&first).expect("insert first") {
        InsertOutcome::Inserted(_) => {}
        InsertOutcome::Duplicate(_) => panic!("first insert unexpectedly duplicate"),
    }

    match store
        .insert_message(&second_same_key)
        .expect("duplicate key result")
    {
        InsertOutcome::Duplicate(StoreDuplicateIdentity::MessageKey(key)) => {
            assert_eq!(key, first.message_key)
        }
        other => panic!("unexpected duplicate outcome: {other:?}"),
    }

    match store
        .insert_message(&second_same_legacy)
        .expect("duplicate legacy result")
    {
        InsertOutcome::Duplicate(StoreDuplicateIdentity::LegacyMessageId(id)) => {
            assert_eq!(id, first.legacy_message_id.expect("legacy id"))
        }
        other => panic!("unexpected duplicate outcome: {other:?}"),
    }
}

#[test]
fn insert_message_enforces_unique_atm_message_id() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");
    let first = message_at(1);
    let second = atm_core::mail_store::StoredMessageRecord {
        message_key: MessageKey::from_atm_message_id(AtmMessageId::new()),
        legacy_message_id: Some(
            "00000000-0000-4000-8000-000000000055"
                .parse()
                .expect("legacy id"),
        ),
        ..first.clone()
    };

    store.insert_message(&first).expect("insert first");
    match store.insert_message(&second).expect("duplicate atm result") {
        InsertOutcome::Duplicate(StoreDuplicateIdentity::AtmMessageId(id)) => {
            assert_eq!(id, first.atm_message_id.expect("atm id"))
        }
        other => panic!("unexpected duplicate outcome: {other:?}"),
    }
}

#[test]
fn insert_message_batch_rolls_back_on_mid_operation_failure() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");

    let first = message_at(1);
    let second = atm_core::mail_store::StoredMessageRecord {
        message_key: MessageKey::from_atm_message_id(
            "01ARZ3NDEKTSV4RRFFQ69G5FBB".parse().expect("atm id"),
        ),
        atm_message_id: Some("01ARZ3NDEKTSV4RRFFQ69G5FBB".parse().expect("atm id")),
        ..first.clone()
    };

    let error = store
        .insert_message_batch(&[first.clone(), second])
        .expect_err("duplicate batch should fail");
    assert_eq!(error.kind, StoreErrorKind::Constraint);
    assert_eq!(
        error.code,
        atm_core::error_codes::AtmErrorCode::StoreConstraintViolation
    );
    assert!(
        store
            .load_message(&first.message_key)
            .expect("reload after failed batch")
            .is_none()
    );
}

#[test]
fn create_read_and_update_store_rows() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");
    let message = message_at(1);

    store.insert_message(&message).expect("insert message");

    let loaded = store
        .load_message(&message.message_key)
        .expect("load message")
        .expect("stored message");
    assert_eq!(loaded.message_key, message.message_key);
    assert_eq!(
        store
            .load_message_by_legacy_id(&message.legacy_message_id.expect("legacy id"))
            .expect("load by legacy")
            .expect("legacy row")
            .message_key,
        message.message_key
    );
    assert_eq!(
        store
            .load_message_by_atm_id(&message.atm_message_id.expect("atm id"))
            .expect("load by atm")
            .expect("atm row")
            .message_key,
        message.message_key
    );

    let ack_state = AckStateRecord {
        message_key: message.message_key.clone(),
        pending_ack_at: Some("2026-05-02T20:00:10Z".parse().expect("timestamp")),
        acknowledged_at: Some("2026-05-02T20:00:20Z".parse().expect("timestamp")),
        ack_reply_message_key: Some("ext:ack-reply-1".parse().expect("message key")),
        ack_reply_team: Some(team()),
        ack_reply_agent: Some(agent("team-lead")),
    };
    store.upsert_ack_state(&ack_state).expect("upsert ack");
    assert_eq!(
        store
            .load_ack_state(&message.message_key)
            .expect("load ack")
            .expect("ack row"),
        ack_state
    );

    let visibility = VisibilityStateRecord {
        message_key: message.message_key.clone(),
        read_at: Some("2026-05-02T20:00:30Z".parse().expect("timestamp")),
        cleared_at: None,
    };
    store
        .upsert_visibility(&visibility)
        .expect("upsert visibility");
    assert_eq!(
        store
            .load_visibility(&message.message_key)
            .expect("load visibility")
            .expect("visibility row"),
        visibility
    );

    let ingest = ingest_record(tempdir.path(), &message.message_key);
    match store.record_ingest(&ingest).expect("record ingest") {
        InsertOutcome::Inserted(_) => {}
        InsertOutcome::Duplicate(_) => panic!("first ingest unexpectedly duplicate"),
    }
    assert_eq!(
        store
            .load_ingest(
                &ingest.team_name,
                &ingest.recipient_agent,
                &ingest.source_fingerprint
            )
            .expect("load ingest")
            .expect("ingest row"),
        ingest
    );

    let export_due = pending_export(
        &message.message_key,
        1,
        "2026-05-02T20:00:15Z",
        "2026-05-02T20:05:00Z",
    );
    let export_future = pending_export(
        &"ext:retry-later"
            .parse::<MessageKey>()
            .expect("message key"),
        2,
        "2026-05-02T21:00:00Z",
        "2026-05-02T21:05:00Z",
    );
    store
        .insert_message(&atm_core::mail_store::StoredMessageRecord {
            message_key: export_future.message_key.clone(),
            team_name: team(),
            recipient_agent: agent("team-lead"),
            sender_display: "arch-ctm".to_string(),
            sender_canonical: Some(agent("arch-ctm")),
            sender_team: Some(team()),
            body: "future".to_string(),
            summary: None,
            created_at: "2026-05-02T20:01:00Z".parse().expect("timestamp"),
            source_kind: MessageSourceKind::External,
            legacy_message_id: None,
            atm_message_id: None,
            raw_metadata_json: None,
        })
        .expect("insert future export message");
    store
        .record_pending_export(&export_due)
        .expect("record export due");
    store
        .record_pending_export(&export_future)
        .expect("record export future");
    assert_eq!(
        store
            .load_due_pending_exports(&"2026-05-02T20:00:20Z".parse().expect("timestamp"), 10)
            .expect("load due exports"),
        vec![export_due.clone()]
    );
    assert_eq!(
        store
            .remove_expired_pending_exports(&"2026-05-02T20:04:59Z".parse().expect("timestamp"))
            .expect("remove not-yet-expired"),
        0
    );
    assert_eq!(
        store
            .remove_expired_pending_exports(&"2026-05-02T21:05:01Z".parse().expect("timestamp"))
            .expect("remove expired"),
        2
    );

    let task = TaskRecord {
        task_id: "TASK-1".parse().expect("task id"),
        message_key: message.message_key.clone(),
        status: TaskStatus::PendingAck,
        created_at: "2026-05-02T20:00:40Z".parse().expect("timestamp"),
        acknowledged_at: None,
        metadata_json: Some("{\"priority\":\"high\"}".to_string()),
    };
    store.upsert_task(&task).expect("upsert task");
    assert_eq!(
        store
            .load_task(&task.task_id)
            .expect("load task")
            .expect("task row"),
        task
    );
    assert_eq!(
        store
            .load_tasks_for_message(&message.message_key)
            .expect("load tasks for message"),
        vec![task.clone()]
    );
    let acknowledged_at: IsoTimestamp = "2026-05-02T20:00:45Z".parse().expect("timestamp");
    let acknowledged = store
        .acknowledge_task(&task.task_id, acknowledged_at)
        .expect("ack task")
        .expect("task row");
    assert_eq!(acknowledged.status, TaskStatus::Acknowledged);
    assert_eq!(acknowledged.acknowledged_at, Some(acknowledged_at));

    let mail_health = store.mail_health().expect("mail health");
    assert!(mail_health.messages_ready);
    assert!(mail_health.inbox_ingest_ready);
    assert!(mail_health.ack_state_ready);
    assert!(mail_health.message_visibility_ready);
    assert!(mail_health.pending_exports_ready);
}

#[test]
fn roster_replace_update_and_pid_round_trip() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");

    let arch = roster_member("arch-ctm", Some("%1"), Some(1001));
    let quality = roster_member("quality-mgr", None, None);
    store
        .replace_roster(&team(), &[arch.clone(), quality.clone()])
        .expect("replace roster");
    let loaded = store.load_roster(&team()).expect("load roster");
    assert_eq!(loaded, vec![arch.clone(), quality.clone()]);

    let updated_quality = RosterMemberRecord {
        recipient_pane_id: Some("%2".parse().expect("pane")),
        ..quality.clone()
    };
    store
        .upsert_roster_member(&updated_quality)
        .expect("upsert roster member");
    let pid_updated = store
        .update_member_pid(
            &team(),
            &updated_quality.agent_name,
            PidUpdate {
                pid: ProcessId::new(2024).expect("pid"),
            },
        )
        .expect("update pid")
        .expect("updated member");

    assert_eq!(pid_updated.agent_name, updated_quality.agent_name);
    assert_eq!(
        pid_updated.recipient_pane_id,
        updated_quality.recipient_pane_id
    );
    assert_eq!(pid_updated.pid, Some(ProcessId::new(2024).expect("pid")));

    let roster_health = store.roster_health().expect("roster health");
    assert!(roster_health.team_roster_ready);
}

#[test]
fn replace_roster_rolls_back_on_constraint_violation() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");
    let arch = roster_member("arch-ctm", Some("%1"), Some(1001));
    let quality = roster_member("quality-mgr", None, None);
    store
        .replace_roster(&team(), &[arch.clone(), quality.clone()])
        .expect("seed roster");

    let duplicate_arch = roster_member("arch-ctm", Some("%9"), Some(9009));
    let error = store
        .replace_roster(&team(), &[duplicate_arch.clone(), duplicate_arch])
        .expect_err("duplicate replacement should fail");
    assert_eq!(error.kind, StoreErrorKind::Constraint);
    assert_eq!(
        store.load_roster(&team()).expect("load roster"),
        vec![arch, quality]
    );
}

#[test]
fn team_roster_schema_keeps_recipient_pane_nullable() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");
    let connection = store.lock_connection().expect("lock");
    let mut statement = connection
        .prepare("PRAGMA table_info(team_roster)")
        .expect("pragma table_info");
    let mut rows = statement.query([]).expect("query table_info");

    let mut saw_recipient_pane = false;
    while let Some(row) = rows.next().expect("next row") {
        let name: String = row.get(1).expect("column name");
        if name == "recipient_pane_id" {
            let not_null: i64 = row.get(3).expect("not null flag");
            saw_recipient_pane = true;
            assert_eq!(not_null, 0);
        }
    }
    assert!(saw_recipient_pane);
}

#[test]
fn store_errors_stay_discriminated() {
    let tempdir = TempDir::new().expect("tempdir");
    let error = RusqliteStore::open_path(tempdir.path()).expect_err("directory path should fail");
    assert_eq!(error.kind, StoreErrorKind::Open);
    assert_eq!(
        error.code,
        atm_core::error_codes::AtmErrorCode::StoreOpenFailed
    );

    let busy = crate::classify_store_error(
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::DatabaseBusy,
                extended_code: rusqlite::ffi::SQLITE_BUSY,
            },
            Some("database busy".to_string()),
        ),
        "busy",
    );
    assert_eq!(busy.kind, StoreErrorKind::Busy);

    let constraint = crate::classify_store_error(
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY,
            },
            Some("constraint failed".to_string()),
        ),
        "constraint",
    );
    assert_eq!(constraint.kind, StoreErrorKind::Constraint);

    let query = crate::classify_store_error(rusqlite::Error::InvalidQuery, "query");
    assert_eq!(query.kind, StoreErrorKind::Query);
}

#[test]
fn bootstrap_report_matches_live_connection_settings() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");
    let connection = store.lock_connection().expect("lock");

    assert_eq!(
        query_user_version(&connection).expect("user version"),
        SCHEMA_VERSION
    );
    assert_eq!(
        query_journal_mode(&connection)
            .expect("journal mode")
            .to_ascii_lowercase(),
        "wal"
    );
    assert!(query_foreign_keys(&connection).expect("foreign keys"));
}
