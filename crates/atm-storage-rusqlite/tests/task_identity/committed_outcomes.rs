use super::*;

#[test]
fn bd2_live_transition_rows_equal_persisted_rows_in_every_field() {
    for outcome in [
        TaskCloseOutcome::Completed,
        TaskCloseOutcome::Refused,
        TaskCloseOutcome::Cancelled,
    ] {
        let h = Harness::new();
        let (_, assigned) = h.assign_with_outcome("TASK", "alice", "lead", None);
        assert_eq!(assigned.task_events, h.events("TASK"));
        let started = h
            .backend
            .message_store()
            .admit_message_with_provenance(
                &h.start_message("TASK", "alice"),
                MessageWriteOrigin::Local,
            )
            .unwrap();
        assert!(
            started.task_events.is_empty(),
            "success rows have one owner"
        );
        assert_eq!(
            started.operation.unwrap().task_events,
            h.events("TASK")[1..]
        );
        let mut close = h.message("lead", "alice", "close");
        close.envelope.task_id = Some("TASK".parse().unwrap());
        close.envelope.task_op = Some(TaskOp::Close {
            outcome,
            reason: Some("reason".into()),
        });
        let admitted = h
            .backend
            .message_store()
            .admit_message_with_provenance(&close, MessageWriteOrigin::Local)
            .unwrap();
        assert!(admitted.task_events.is_empty());
        assert_eq!(
            admitted.operation.unwrap().task_events,
            h.events("TASK")[2..]
        );
    }
}

#[test]
fn bd2_direct_audit_rows_equal_persisted_rows_in_every_field() {
    let h = Harness::new();
    h.assign("TASK", "alice", "lead", None);
    let member = MemberKey::new(h.team.clone(), "alice".parse().unwrap());
    let task = "TASK".parse().unwrap();
    let store = h.backend.task_store();
    for outcome in [
        ReminderOutcome::Emitted,
        ReminderOutcome::Unrenderable,
        ReminderOutcome::Blocked,
    ] {
        let result = store
            .record_reminder(&member, &task, IsoTimestamp::now(), outcome)
            .unwrap();
        assert_eq!(Some(&result.event), h.events("TASK").last());
        assert_eq!(result.row, h.row("TASK"));
    }
    let lead = store
        .record_lead_notified(
            &member,
            &task,
            IsoTimestamp::now(),
            &"lead".parse().unwrap(),
            &AtmMessageId::new(),
        )
        .unwrap();
    assert_eq!(Some(&lead), h.events("TASK").last());
    let reset = store
        .reset_reminders(&member, &task, IsoTimestamp::now())
        .unwrap();
    assert_eq!(Some(&reset.event), h.events("TASK").last());
    assert_eq!(reset.row, h.row("TASK"));
    let moved = store
        .move_task(
            &h.team,
            &task,
            &"lead".parse().unwrap(),
            &MoveTarget::Head,
            IsoTimestamp::now(),
        )
        .unwrap();
    assert!(
        moved.task_events.is_empty(),
        "successful movement owns its event in TaskMoveRecord"
    );
    assert_eq!(
        Some(&moved.operation.unwrap().event),
        h.events("TASK").last()
    );
}

#[test]
fn bd2_plain_mail_and_acknowledgement_create_no_task_events() {
    struct Reply;
    impl atm_storage::AcknowledgementReplyBuilder for Reply {
        fn build_reply(&self, source: &Message) -> Result<Message, atm_storage::AtmError> {
            let mut reply = source.clone();
            let id = AtmMessageId::new();
            reply.message_key = id.into();
            reply.envelope.message_id = Some(id);
            reply.envelope.requires_ack = false;
            reply.envelope.pending_ack_at = None;
            reply.envelope.acknowledges_message_id = source.envelope.message_id;
            Ok(reply)
        }
    }
    let h = Harness::new();
    let mut plain = h.message("alice", "lead", "plain mail");
    plain.envelope.requires_ack = true;
    plain.envelope.pending_ack_at = Some(IsoTimestamp::now());
    let admission = h
        .backend
        .message_store()
        .admit_message_with_provenance(&plain, MessageWriteOrigin::Local)
        .unwrap();
    assert!(admission.task_events.is_empty());
    assert!(admission.operation.unwrap().task_events.is_empty());
    h.backend
        .message_store()
        .acknowledge_message_atomically(
            &atm_storage::AcknowledgementSource {
                team: h.team.clone(),
                agent: plain.agent,
                message_id: plain.envelope.message_id.unwrap(),
            },
            std::sync::Arc::new(Reply),
        )
        .unwrap();
    let connection = Connection::open(&h.path).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM task_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn bd2_historical_acked_and_migrated_rows_remain_decodable() {
    let h = Harness::new();
    let connection = Connection::open(&h.path).unwrap();
    for (seq, kind) in [(1, "acked"), (2, "migrated")] {
        connection.execute("INSERT INTO task_events(team,task_id,assignee,seq,at,event,from_state,to_state,actor)
            VALUES (?1,'HISTORY','alice',?2,'2026-09-01T00:00:00Z',?3,'assigned','active','alice')",
            params![h.team.as_str(), seq, kind]).unwrap();
    }
    let events = h.events("HISTORY");
    assert_eq!(
        events.iter().map(|row| row.event).collect::<Vec<_>>(),
        vec![TaskEventKind::Acked, TaskEventKind::Migrated]
    );
    assert_eq!(events[0].seq, 1);
    assert_eq!(events[1].seq, 2);
}
