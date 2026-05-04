use super::*;

#[test]
fn inbox_ingress_counts_duplicate_atm_messages_across_source_files() {
    // Dedup advisory (Q.2-QA-12 / ATM-QA-012-003): duplicate suppression is
    // defined at the SQLite ingress boundary, so repeated source imports must
    // increment duplicate_messages without mutating durable message truth.
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_for_team_home(tempdir.path(), &team()).expect("open store");
    let primary_inbox = home::inbox_path_from_home(tempdir.path(), &team(), &agent(TEST_RECIPIENT))
        .expect("primary inbox path");
    let origin_inbox = primary_inbox
        .parent()
        .expect("inbox dir")
        .join(format!("{TEST_RECIPIENT}.origin-a.json"));
    fs::create_dir_all(primary_inbox.parent().expect("inbox dir")).expect("create inbox dir");

    let value =
        atm_core::schema::to_shared_inbox_value(&inbox_message("duplicate atm")).expect("value");
    fs::write(
        &primary_inbox,
        serde_json::to_vec(&vec![value.clone()]).expect("primary inbox json"),
    )
    .expect("write primary inbox");
    fs::write(
        &origin_inbox,
        serde_json::to_vec(&vec![value]).expect("origin inbox json"),
    )
    .expect("write origin inbox");

    let outcome = default_inbox_ingress()
        .ingest_mailbox_state(
            tempdir.path(),
            &team(),
            &agent(TEST_RECIPIENT),
            &store,
            &NullObservability,
        )
        .expect("ingest succeeds");

    assert_eq!(
        outcome,
        InboxIngestOutcome {
            imported_messages: 1,
            duplicate_messages: 1,
            degraded_records: 0,
        }
    );
}

#[test]
fn inbox_ingress_counts_duplicate_external_messages_on_reingest() {
    // Dedup advisory (Q.2-QA-12 / ATM-QA-012-003): re-ingesting the same
    // external record must report a duplicate rather than creating a second
    // durable row.
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_for_team_home(tempdir.path(), &team()).expect("open store");
    let inbox_path = home::inbox_path_from_home(tempdir.path(), &team(), &agent(TEST_RECIPIENT))
        .expect("inbox path");
    fs::create_dir_all(inbox_path.parent().expect("inbox dir")).expect("create inbox dir");

    let external = serde_json::json!({
        "from": "external-sender",
        "text": "duplicate external",
        "timestamp": "2026-05-02T21:00:00Z",
        "read": false
    });
    fs::write(
        &inbox_path,
        serde_json::to_vec(&vec![external]).expect("inbox json"),
    )
    .expect("write inbox");

    let ingester = default_inbox_ingress();
    let first = ingester
        .ingest_mailbox_state(
            tempdir.path(),
            &team(),
            &agent(TEST_RECIPIENT),
            &store,
            &NullObservability,
        )
        .expect("first ingest");
    assert_eq!(first.imported_messages, 1);
    assert_eq!(first.duplicate_messages, 0);

    let second = ingester
        .ingest_mailbox_state(
            tempdir.path(),
            &team(),
            &agent(TEST_RECIPIENT),
            &store,
            &NullObservability,
        )
        .expect("second ingest");
    assert_eq!(second.imported_messages, 0);
    assert_eq!(second.duplicate_messages, 1);
}

#[test]
fn inbox_ingress_is_idempotent_and_tracks_degraded_metadata() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_for_team_home(tempdir.path(), &team()).expect("open store");
    let inbox_path = home::inbox_path_from_home(tempdir.path(), &team(), &agent(TEST_RECIPIENT))
        .expect("inbox path");
    if let Some(parent) = inbox_path.parent() {
        fs::create_dir_all(parent).expect("inbox dir");
    }

    let valid = inbox_message("ingest me");
    let valid_value = atm_core::schema::to_shared_inbox_value(&valid).expect("shared inbox value");
    let malformed_metadata = serde_json::json!({
        "from": TEST_QA_AGENT,
        "text": "external malformed metadata",
        "timestamp": "2026-05-02T21:00:00Z",
        "read": false,
        "metadata": { "atm": { "messageId": "not-a-ulid" } }
    });
    fs::write(
        &inbox_path,
        serde_json::to_vec(&vec![valid_value, malformed_metadata]).expect("json array"),
    )
    .expect("write inbox");

    let observability = RecordingObservability::default();
    let ingester = default_inbox_ingress();
    let first = ingester
        .ingest_mailbox_state(
            tempdir.path(),
            &team(),
            &agent(TEST_RECIPIENT),
            &store,
            &observability,
        )
        .expect("first ingest");
    assert_eq!(
        first,
        InboxIngestOutcome {
            imported_messages: 2,
            duplicate_messages: 0,
            degraded_records: 1,
        }
    );
    let degraded_envelope = atm_core::read_messages(&inbox_path)
        .expect("read inbox messages")
        .into_iter()
        .find(|message| message.from.as_str() == TEST_QA_AGENT)
        .expect("degraded envelope");
    let degraded_message_key = MessageKey::from_source_fingerprint(&external_source_fingerprint(
        &inbox_path,
        degraded_envelope.from.as_str(),
        &degraded_envelope.timestamp.to_string(),
        degraded_envelope.summary.as_deref(),
        &degraded_envelope.text,
    ));
    let degraded_message = store
        .load_message(&degraded_message_key)
        .expect("load degraded message")
        .expect("stored degraded row");
    assert_eq!(degraded_message.sender_display, TEST_QA_AGENT);
    assert_eq!(degraded_message.body, "external malformed metadata");
    let events = observability.events.lock().expect("events lock");
    assert!(
        events.iter().any(|event| {
            event.command == "inbox_ingress"
                && event.outcome == "malformed_metadata"
                && event.error_code
                    == Some(atm_core::error::AtmErrorCode::WarningMalformedAtmFieldIgnored)
        }),
        "events: {events:?}"
    );
    drop(events);

    let second = ingester
        .ingest_mailbox_state(
            tempdir.path(),
            &team(),
            &agent(TEST_RECIPIENT),
            &store,
            &observability,
        )
        .expect("second ingest");
    assert_eq!(
        second,
        InboxIngestOutcome {
            imported_messages: 0,
            duplicate_messages: 2,
            degraded_records: 1,
        }
    );
}

#[test]
fn inbox_ingress_tolerates_bare_invalid_jsonl_line_without_panicking() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_for_team_home(tempdir.path(), &team()).expect("open store");
    let inbox_path = home::inbox_path_from_home(tempdir.path(), &team(), &agent(TEST_RECIPIENT))
        .expect("inbox path");
    if let Some(parent) = inbox_path.parent() {
        fs::create_dir_all(parent).expect("inbox dir");
    }

    let valid = inbox_message("ingest me");
    let valid_value = atm_core::schema::to_shared_inbox_value(&valid).expect("shared inbox value");
    let raw = format!(
        "{}\n{{not-json\n",
        serde_json::to_string(&valid_value).expect("json line")
    );
    fs::write(&inbox_path, raw).expect("write inbox");

    let observability = NullObservability;
    let outcome = default_inbox_ingress()
        .ingest_mailbox_state(
            tempdir.path(),
            &team(),
            &agent(TEST_RECIPIENT),
            &store,
            &observability,
        )
        .expect("ingest succeeds");

    assert_eq!(
        outcome,
        InboxIngestOutcome {
            imported_messages: 1,
            duplicate_messages: 0,
            degraded_records: 1,
        }
    );
}

#[test]
fn inbox_ingress_uses_envelope_defaults_when_sidecar_entry_is_absent_or_malformed() {
    let tempdir = TempDir::new().expect("tempdir");
    let store = RusqliteStore::open_for_team_home(tempdir.path(), &team()).expect("open store");
    let inbox_path = home::inbox_path_from_home(tempdir.path(), &team(), &agent(TEST_RECIPIENT))
        .expect("inbox path");
    if let Some(parent) = inbox_path.parent() {
        fs::create_dir_all(parent).expect("inbox dir");
    }
    let workflow_path =
        home::workflow_state_path_from_home(tempdir.path(), team().as_str(), TEST_RECIPIENT)
            .expect("workflow path");
    if let Some(parent) = workflow_path.parent() {
        fs::create_dir_all(parent).expect("workflow dir");
    }

    let mut with_sidecar = inbox_message("sidecar override");
    let sidecar_atm_id: AtmMessageId = "01JQYVB6W51Q2E7E6T3Y4Q9N2M"
        .parse()
        .expect("sidecar atm id");
    with_sidecar.extra["metadata"]["atm"]["messageId"] =
        serde_json::Value::String(sidecar_atm_id.to_string());
    let mut without_sidecar = inbox_message("default envelope state");
    let without_sidecar_atm_id: AtmMessageId = "01JQYVB6W51Q2E7E6T3Y4Q9N2N"
        .parse()
        .expect("default atm id");
    without_sidecar.extra["metadata"]["atm"]["messageId"] =
        serde_json::Value::String(without_sidecar_atm_id.to_string());
    let mut malformed_sidecar = inbox_message("malformed sidecar state");
    let malformed_sidecar_atm_id: AtmMessageId = "01JQYVB6W51Q2E7E6T3Y4Q9N2P"
        .parse()
        .expect("malformed atm id");
    malformed_sidecar.extra["metadata"]["atm"]["messageId"] =
        serde_json::Value::String(malformed_sidecar_atm_id.to_string());
    fs::write(
        &workflow_path,
        serde_json::to_vec(&serde_json::json!({
            "messages": {
                format!("atm:{sidecar_atm_id}"): {
                    "read": true,
                    "pendingAckAt": null,
                    "acknowledgedAt": null
                },
                format!("atm:{malformed_sidecar_atm_id}"): "not-an-object"
            }
        }))
        .expect("workflow json"),
    )
    .expect("write workflow state");
    fs::write(
        &inbox_path,
        serde_json::to_vec(&vec![
            atm_core::schema::to_shared_inbox_value(&with_sidecar).expect("sidecar message"),
            atm_core::schema::to_shared_inbox_value(&without_sidecar).expect("default message"),
            atm_core::schema::to_shared_inbox_value(&malformed_sidecar)
                .expect("malformed sidecar message"),
        ])
        .expect("json array"),
    )
    .expect("write inbox");

    let observability = NullObservability;
    default_inbox_ingress()
        .ingest_mailbox_state(
            tempdir.path(),
            &team(),
            &agent(TEST_RECIPIENT),
            &store,
            &observability,
        )
        .expect("ingest succeeds");

    let stored_with = store
        .load_message_by_atm_id(&sidecar_atm_id)
        .expect("load sidecar message")
        .expect("stored sidecar row");
    let stored_without = store
        .load_message_by_atm_id(&without_sidecar_atm_id)
        .expect("load default message")
        .expect("stored default row");
    let stored_malformed = store
        .load_message_by_atm_id(&malformed_sidecar_atm_id)
        .expect("load malformed-sidecar message")
        .expect("stored malformed-sidecar row");

    assert!(
        store
            .load_visibility(&stored_with.message_key)
            .expect("load sidecar visibility")
            .expect("sidecar visibility row")
            .read_at
            .is_some()
    );
    assert!(
        store
            .load_ack_state(&stored_with.message_key)
            .expect("load sidecar ack")
            .is_none()
    );

    let default_ack = store
        .load_ack_state(&stored_without.message_key)
        .expect("load default ack")
        .expect("default ack row");
    assert!(default_ack.pending_ack_at.is_some());
    assert!(
        store
            .load_visibility(&stored_without.message_key)
            .expect("load default visibility")
            .is_none()
    );

    let malformed_ack = store
        .load_ack_state(&stored_malformed.message_key)
        .expect("load malformed-sidecar ack")
        .expect("malformed-sidecar ack row");
    assert!(malformed_ack.pending_ack_at.is_some());
    assert!(
        store
            .load_visibility(&stored_malformed.message_key)
            .expect("load malformed-sidecar visibility")
            .is_none()
    );
}
