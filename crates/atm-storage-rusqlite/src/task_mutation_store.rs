//! Concrete v2 task mutation adapter tests.

#[cfg(test)]
mod tests {
    use crate::SqliteStorageBackend;
    use atm_storage::contract::{Message, MessageKey};
    use atm_storage::schema::{AtmMessageId, MessageEnvelope};
    use atm_storage::types::{AgentName, IsoTimestamp, MemberKey, TaskId, TeamName};
    use atm_storage::{
        MessageWriteOrigin, PreparedAssignment, PreparedMessage, ReadDeadline, TaskLedgerScope,
        TaskLifecycleState, TaskMutationRequest, TaskOperation, TaskOperationId, TaskOutcome,
        TaskPriority, TaskRevision,
    };
    use serde_json::Map;
    use std::time::Duration;

    fn team() -> TeamName {
        "task-v2-test".parse().expect("team")
    }

    fn member(name: &str) -> MemberKey {
        MemberKey::new(team(), name.parse::<AgentName>().expect("agent"))
    }

    fn task(value: &str) -> TaskId {
        value.parse().expect("task")
    }

    fn message_id() -> AtmMessageId {
        AtmMessageId::new()
    }

    fn assignment(
        task_id: &TaskId,
        assignee: &MemberKey,
        assigner: &MemberKey,
    ) -> PreparedAssignment {
        let id = message_id();
        PreparedAssignment {
            assignee: assignee.clone(),
            assigner: assigner.clone(),
            priority: TaskPriority::Normal,
            delivery_origin: MessageWriteOrigin::Local,
            message: PreparedMessage {
                message: Message {
                    team: team(),
                    agent: assignee.agent().clone(),
                    message_key: MessageKey::from(id),
                    envelope: MessageEnvelope {
                        from: assigner.agent().clone(),
                        source_chat_id: None,
                        text: "assignment body must remain in mail_messages only".to_owned(),
                        timestamp: IsoTimestamp::now(),
                        read: false,
                        source_team: Some(team()),
                        destination_chat_id: None,
                        summary: Some("assignment".to_owned()),
                        message_id: Some(id),
                        requires_ack: false,
                        pending_ack_at: None,
                        acknowledged_at: None,
                        acknowledges_message_id: None,
                        parent_message_id: None,
                        thread_mode: None,
                        expires_at: None,
                        task_id: Some(task_id.clone()),
                        task_complete: None,
                        extra: Map::new(),
                    },
                },
            },
            template_sha: None,
        }
    }

    fn handoff(task_id: &TaskId, recipient: &MemberKey, sender: &MemberKey) -> PreparedMessage {
        let id = message_id();
        PreparedMessage {
            message: Message {
                team: team(),
                agent: recipient.agent().clone(),
                message_key: MessageKey::from(id),
                envelope: MessageEnvelope {
                    from: sender.agent().clone(),
                    source_chat_id: None,
                    text: "completion handoff".to_owned(),
                    timestamp: IsoTimestamp::now(),
                    read: false,
                    source_team: Some(team()),
                    destination_chat_id: None,
                    summary: Some("handoff".to_owned()),
                    message_id: Some(id),
                    requires_ack: false,
                    pending_ack_at: None,
                    acknowledged_at: None,
                    acknowledges_message_id: None,
                    parent_message_id: None,
                    thread_mode: None,
                    expires_at: None,
                    task_id: Some(task_id.clone()),
                    task_complete: Some(task_id.clone()),
                    extra: Map::new(),
                },
            },
        }
    }

    fn request(actor: MemberKey, task_id: TaskId, operation: TaskOperation) -> TaskMutationRequest {
        TaskMutationRequest {
            operation_id: TaskOperationId::new(),
            actor,
            task_id,
            expected_revision: None,
            operation,
        }
    }

    #[tokio::test]
    async fn mutation_replay_is_idempotent_and_assignment_body_is_not_duplicated() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let lead = member("lead");
        let worker = member("worker");
        let task_id = task("v2-idempotent-task");
        let request = request(
            lead.clone(),
            task_id.clone(),
            TaskOperation::Assign(assignment(&task_id, &worker, &lead)),
        );

        let first = store
            .apply(request.clone())
            .await
            .expect("initial assignment");
        let replay = store.apply(request).await.expect("idempotent replay");

        assert_eq!(first.state, TaskLifecycleState::Assigned);
        assert!(!first.replayed);
        assert_eq!(replay.state, TaskLifecycleState::Assigned);
        assert!(replay.replayed);
        assert!(first.message_id.is_some());
        assert_eq!(replay.message_id, first.message_id);
        assert_eq!(replay.current_assignee, first.current_assignee);
        assert_eq!(replay.current_attempt, first.current_attempt);
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let attempts: u64 = connection
                    .query_row("SELECT COUNT(*) FROM task_assignment_attempts", [], |row| row.get(0))
                    .expect("attempt count");
                let events: u64 = connection
                    .query_row("SELECT COUNT(*) FROM task_events_v2", [], |row| row.get(0))
                    .expect("event count");
                let body_columns: u64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM pragma_table_info('tasks_v2') WHERE name IN ('description', 'body', 'message_text')",
                        [],
                        |row| row.get(0),
                    )
                    .expect("body-column count");
                assert_eq!(attempts, 1);
                assert_eq!(events, 1);
                assert_eq!(body_columns, 0, "canonical v2 task rows do not duplicate bodies");
                Ok(())
            })
            .expect("inspect v2 state");
    }

    #[tokio::test]
    async fn canonical_mutations_refresh_the_retained_v1_projection_without_retriggering_v2() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let lead = member("lead");
        let worker = member("worker");
        let task_id = task("v2-v1-coexistence-task");

        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Assign(assignment(&task_id, &worker, &lead)),
            ))
            .await
            .expect("assign");
        assert_v1_projection_state(&backend, "v2-v1-coexistence-task", "worker", "assigned");

        store
            .apply(request(
                worker.clone(),
                task_id.clone(),
                TaskOperation::Start,
            ))
            .await
            .expect("start");
        assert_v1_projection_state(&backend, "v2-v1-coexistence-task", "worker", "active");

        store
            .apply(request(
                worker.clone(),
                task_id.clone(),
                TaskOperation::Close {
                    outcome: TaskOutcome::Succeeded,
                    handoff: handoff(&task_id, &lead, &worker),
                },
            ))
            .await
            .expect("close");
        assert_v1_projection_state(&backend, "v2-v1-coexistence-task", "worker", "complete");
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let events: u64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM task_events_v2 WHERE task_id = 'v2-v1-coexistence-task'",
                        [],
                        |row| row.get(0),
                    )
                    .expect("canonical event count");
                assert_eq!(events, 3, "compatibility writes must not retrigger v1 bridge events");
                Ok(())
            })
            .expect("inspect canonical events");
    }

    fn assert_v1_projection_state(
        backend: &SqliteStorageBackend,
        task_id: &str,
        assignee: &str,
        expected_state: &str,
    ) {
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let state: String = connection
                    .query_row(
                        "SELECT state FROM tasks
                         WHERE team = 'task-v2-test' AND task_id = ?1 AND assignee = ?2",
                        [task_id, assignee],
                        |row| row.get(0),
                    )
                    .expect("retained v1 state");
                assert_eq!(state, expected_state);
                Ok(())
            })
            .expect("inspect retained v1 projection");
    }

    #[tokio::test]
    async fn close_clears_every_task_linked_pending_marker() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let lead = member("lead");
        let worker = member("worker");
        let task_id = task("v2-cleanup-task");
        let assignment = assignment(&task_id, &worker, &lead);
        let assignment_id = assignment
            .message
            .message
            .envelope
            .message_id
            .expect("message id");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Assign(assignment),
            ))
            .await
            .expect("assignment");
        assert!(
            backend
                .pending_nudge_store()
                .mark_pending(&worker, &assignment_id, IsoTimestamp::now())
                .expect("mark pending")
        );
        store
            .apply(request(
                worker.clone(),
                task_id.clone(),
                TaskOperation::Start,
            ))
            .await
            .expect("start");

        let result = store
            .apply(request(
                worker.clone(),
                task_id.clone(),
                TaskOperation::Close {
                    outcome: TaskOutcome::Succeeded,
                    handoff: handoff(&task_id, &lead, &worker),
                },
            ))
            .await
            .expect("close");

        assert_eq!(
            result.state,
            TaskLifecycleState::Closed(TaskOutcome::Succeeded)
        );
        assert!(
            backend
                .pending_nudge_store()
                .claim_next_pending(&worker)
                .expect("claim")
                .is_none()
        );
    }

    #[tokio::test]
    async fn cancelled_close_is_read_back_as_cancelled_not_superseded() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let reader = backend.async_task_ledger_reader();
        let lead = member("lead");
        let worker = member("worker");
        let task_id = task("v2-cancelled-close");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Assign(assignment(&task_id, &worker, &lead)),
            ))
            .await
            .expect("assign");
        let outcome = TaskOutcome::Aborted(atm_storage::TaskAbortReason::Cancelled);
        store
            .apply(request(
                worker.clone(),
                task_id.clone(),
                TaskOperation::Close {
                    outcome: outcome.clone(),
                    handoff: handoff(&task_id, &lead, &worker),
                },
            ))
            .await
            .expect("cancel");
        let deadline = ReadDeadline::new(Duration::from_secs(1)).expect("deadline");
        let events = reader
            .list_task_lifecycle_events(team(), task_id, None, deadline)
            .await
            .expect("events");
        assert_eq!(
            events.last().and_then(|event| event.outcome.clone()),
            Some(outcome)
        );
    }

    #[tokio::test]
    async fn peer_origin_assignment_rejects_before_any_durable_task_mutation() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let lead = member("lead");
        let worker = member("worker");
        let task_id = task("v2-cross-host-rejected");
        let mut prepared = assignment(&task_id, &worker, &lead);
        prepared.delivery_origin = MessageWriteOrigin::Peer;

        let error = store
            .apply(request(
                lead,
                task_id.clone(),
                TaskOperation::Assign(prepared),
            ))
            .await
            .expect_err("peer-origin task handoff must be rejected");
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::TaskHandoffCrossHostUnsupported
        );
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let mutations: u64 = connection
                    .query_row(
                        "SELECT (SELECT COUNT(*) FROM tasks_v2) +
                                (SELECT COUNT(*) FROM task_assignment_attempts) +
                                (SELECT COUNT(*) FROM task_events_v2) +
                                (SELECT COUNT(*) FROM task_operations) +
                                (SELECT COUNT(*) FROM mail_messages)",
                        [],
                        |row| row.get(0),
                    )
                    .expect("mutation count");
                assert_eq!(mutations, 0, "cross-host rejection is pre-mutation");
                Ok(())
            })
            .expect("inspect failed handoff");
    }

    #[tokio::test]
    async fn concurrent_starts_commit_exactly_one_active_task_without_a_loser_event() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let lead = member("lead");
        let worker = member("worker");
        let first_task = task("v2-active-first");
        let second_task = task("v2-active-second");
        for task_id in [&first_task, &second_task] {
            store
                .apply(request(
                    lead.clone(),
                    task_id.clone(),
                    TaskOperation::Assign(assignment(task_id, &worker, &lead)),
                ))
                .await
                .expect("assignment");
        }
        let (first, second) = tokio::join!(
            store.apply(request(
                lead.clone(),
                first_task.clone(),
                TaskOperation::Start
            )),
            store.apply(request(lead, second_task.clone(), TaskOperation::Start)),
        );
        assert_eq!(
            usize::from(first.is_ok()) + usize::from(second.is_ok()),
            1,
            "exactly one writer transaction may make this member active"
        );
        let error = first
            .err()
            .or_else(|| second.err())
            .expect("one start must lose");
        assert_eq!(error.code(), atm_storage::AtmErrorCode::TaskActiveConflict);
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let start_events: u64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM task_events_v2 WHERE event = 'started'",
                        [],
                        |row| row.get(0),
                    )
                    .expect("start event count");
                let active: u64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM tasks_v2 WHERE state = 'active'",
                        [],
                        |row| row.get(0),
                    )
                    .expect("active task count");
                assert_eq!(start_events, 1, "failed start has no lifecycle event");
                assert_eq!(
                    active, 1,
                    "the partial unique index remains the final authority"
                );
                Ok(())
            })
            .expect("inspect concurrent starts");
    }

    #[tokio::test]
    async fn concurrent_reassign_compare_and_swap_commits_one_attempt_without_partial_mail() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let lead = member("lead");
        let worker = member("worker");
        let alpha = member("alpha");
        let beta = member("beta");
        let task_id = task("v2-concurrent-reassign");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Assign(assignment(&task_id, &worker, &lead)),
            ))
            .await
            .expect("assign");
        let reassign = |assignee: &MemberKey| TaskMutationRequest {
            operation_id: TaskOperationId::new(),
            actor: lead.clone(),
            task_id: task_id.clone(),
            expected_revision: Some(TaskRevision::from_raw(1)),
            operation: TaskOperation::Reassign(assignment(&task_id, assignee, &lead)),
        };
        let (left, right) =
            tokio::join!(store.apply(reassign(&alpha)), store.apply(reassign(&beta)));
        assert_eq!(
            usize::from(left.is_ok()) + usize::from(right.is_ok()),
            1,
            "exactly one compare-and-swap reassignment commits"
        );
        let loser = left.err().or_else(|| right.err()).expect("one stale loser");
        assert_eq!(loser.code(), atm_storage::AtmErrorCode::TaskRevisionStale);
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let attempts: u64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM task_assignment_attempts WHERE task_id = 'v2-concurrent-reassign'",
                        [],
                        |row| row.get(0),
                    )
                    .expect("attempt count");
                let messages: u64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM mail_messages
                         WHERE team = 'task-v2-test' AND agent IN ('alpha', 'beta')",
                        [],
                        |row| row.get(0),
                    )
                    .expect("reassignment mail count");
                assert_eq!(attempts, 2);
                assert_eq!(messages, 1, "stale reassignment writes no assignment mail");
                Ok(())
            })
            .expect("inspect concurrent reassignment");
    }

    #[tokio::test]
    async fn failed_handoff_write_rolls_back_terminal_state_event_and_operation() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let lead = member("lead");
        let worker = member("worker");
        let task_id = task("v2-handoff-rollback");
        let prepared = assignment(&task_id, &worker, &lead);
        let assignment_id = prepared
            .message
            .message
            .envelope
            .message_id
            .expect("assignment id");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Assign(prepared),
            ))
            .await
            .expect("assign");
        store
            .apply(request(
                worker.clone(),
                task_id.clone(),
                TaskOperation::Start,
            ))
            .await
            .expect("start");
        let mut conflicting_handoff = handoff(&task_id, &worker, &worker);
        conflicting_handoff.message.envelope.message_id = Some(assignment_id);
        let error = store
            .apply(request(
                worker,
                task_id.clone(),
                TaskOperation::Close {
                    outcome: TaskOutcome::Succeeded,
                    handoff: conflicting_handoff,
                },
            ))
            .await
            .expect_err("handoff message id collision must fail");
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::MessageValidationFailed
        );
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let state: String = connection
                    .query_row(
                        "SELECT state FROM tasks_v2 WHERE task_id = 'v2-handoff-rollback'",
                        [],
                        |row| row.get(0),
                    )
                    .expect("task state");
                let events: u64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM task_events_v2 WHERE task_id = 'v2-handoff-rollback'",
                        [],
                        |row| row.get(0),
                    )
                    .expect("event count");
                let operations: u64 = connection
                    .query_row("SELECT COUNT(*) FROM task_operations", [], |row| row.get(0))
                    .expect("operation count");
                assert_eq!(state, "active");
                assert_eq!(events, 2);
                assert_eq!(operations, 2, "failed close writes no idempotency result");
                Ok(())
            })
            .expect("inspect rollback state");
    }

    #[tokio::test]
    async fn bounded_logical_reader_owns_priority_and_top_runnable_order() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let reader = backend.async_task_ledger_reader();
        let lead = member("lead");
        let worker = member("worker");
        let high_task = task("v2-order-high");
        let normal_task = task("v2-order-normal");
        let mut high_assignment = assignment(&high_task, &worker, &lead);
        high_assignment.priority = TaskPriority::High;
        for (task_id, prepared) in [
            (&normal_task, assignment(&normal_task, &worker, &lead)),
            (&high_task, high_assignment),
        ] {
            store
                .apply(request(
                    lead.clone(),
                    task_id.clone(),
                    TaskOperation::Assign(prepared),
                ))
                .await
                .expect("assignment");
        }
        let deadline = || ReadDeadline::new(Duration::from_secs(1)).expect("deadline");
        let list = reader
            .list_logical_tasks(
                team(),
                Some(worker.agent().clone()),
                TaskLedgerScope::All,
                None,
                deadline(),
            )
            .await
            .expect("logical list");
        assert_eq!(
            list.iter()
                .map(|row| row.task_id.clone())
                .collect::<Vec<_>>(),
            vec![high_task.clone(), normal_task.clone()]
        );
        assert_eq!(
            reader
                .top_runnable_task(team(), worker.agent().clone(), deadline())
                .await
                .expect("top runnable")
                .expect("assigned task")
                .task_id,
            high_task
        );
        store
            .apply(request(lead, normal_task.clone(), TaskOperation::Start))
            .await
            .expect("start normal task");
        assert_eq!(
            reader
                .top_runnable_task(team(), worker.agent().clone(), deadline())
                .await
                .expect("top runnable")
                .expect("active task")
                .task_id,
            normal_task
        );
    }

    #[tokio::test]
    async fn logical_reader_projects_reminder_cadence_for_current_attempt_only() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let reader = backend.async_task_ledger_reader();
        let lead = member("lead");
        let worker = member("worker");
        let alternate = member("alternate");
        let task_id = task("v2-reminder-attempt-projection");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Assign(assignment(&task_id, &worker, &lead)),
            ))
            .await
            .expect("assign first attempt");
        let deadline = || ReadDeadline::new(Duration::from_secs(1)).expect("deadline");
        let first_attempt = reader
            .load_logical_task(team(), task_id.clone(), deadline())
            .await
            .expect("load first attempt")
            .expect("first task row");
        let reminded_at: IsoTimestamp = "2026-09-10T00:00:00Z".parse().expect("timestamp");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::RecordReminder {
                    attempt: first_attempt.current_attempt,
                    at: reminded_at,
                },
            ))
            .await
            .expect("record first-attempt reminder");
        let reminded = reader
            .load_logical_task(team(), task_id.clone(), deadline())
            .await
            .expect("load reminded task")
            .expect("reminded task row");
        assert_eq!(reminded.last_reminded_at, Some(reminded_at));

        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Reassign(assignment(&task_id, &alternate, &lead)),
            ))
            .await
            .expect("reassign to second attempt");
        let reassigned = reader
            .top_runnable_task(team(), alternate.agent().clone(), deadline())
            .await
            .expect("read second attempt")
            .expect("second task row");
        assert_ne!(reassigned.current_attempt, first_attempt.current_attempt);
        assert_eq!(reassigned.last_reminded_at, None);
    }

    #[tokio::test]
    async fn reassign_reopen_and_supersede_preserve_history_and_identity() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let reader = backend.async_task_ledger_reader();
        let lead = member("lead");
        let worker = member("worker");
        let alternate = member("alternate");
        let task_id = task("v2-history-task");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Assign(assignment(&task_id, &worker, &lead)),
            ))
            .await
            .expect("assign");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Reassign(assignment(&task_id, &alternate, &lead)),
            ))
            .await
            .expect("reassign");
        let deadline = || ReadDeadline::new(Duration::from_secs(1)).expect("deadline");
        assert_eq!(
            reader
                .list_task_assignment_attempts(team(), task_id.clone(), deadline())
                .await
                .expect("attempts")
                .len(),
            2
        );
        store
            .apply(request(
                alternate.clone(),
                task_id.clone(),
                TaskOperation::Start,
            ))
            .await
            .expect("start");
        store
            .apply(request(
                alternate.clone(),
                task_id.clone(),
                TaskOperation::Close {
                    outcome: TaskOutcome::Succeeded,
                    handoff: handoff(&task_id, &lead, &alternate),
                },
            ))
            .await
            .expect("close");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Reopen(assignment(&task_id, &worker, &lead)),
            ))
            .await
            .expect("reopen");
        let successor_id = task("v2-history-successor");
        let result = store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Supersede {
                    handoff: handoff(&task_id, &lead, &lead),
                    successor_task_id: successor_id.clone(),
                    successor: Box::new(assignment(&successor_id, &alternate, &lead)),
                },
            ))
            .await
            .expect("supersede");
        assert_eq!(
            result.state,
            TaskLifecycleState::Closed(TaskOutcome::Aborted(
                atm_storage::TaskAbortReason::Superseded {
                    successor_task_id: successor_id.clone(),
                }
            ))
        );
        let rows = reader
            .list_logical_tasks(team(), None, TaskLedgerScope::All, None, deadline())
            .await
            .expect("logical rows");
        assert!(rows.iter().any(|row| {
            row.task_id == successor_id
                && row.current_assignee == *alternate.agent()
                && row.state == TaskLifecycleState::Assigned
        }));
        assert_eq!(
            reader
                .list_task_assignment_attempts(team(), task_id, deadline())
                .await
                .expect("history attempts")
                .len(),
            3,
            "reassign and reopen retain the task id and append attempts"
        );
    }

    #[tokio::test]
    async fn stale_revisions_and_conflicting_operation_ids_fail_without_partial_state() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let store = backend.async_task_mutation_store();
        let lead = member("lead");
        let worker = member("worker");
        let task_id = task("v2-cas-task");
        store
            .apply(request(
                lead.clone(),
                task_id.clone(),
                TaskOperation::Assign(assignment(&task_id, &worker, &lead)),
            ))
            .await
            .expect("assign");
        let operation_id = TaskOperationId::new();
        let start = TaskMutationRequest {
            operation_id,
            actor: worker.clone(),
            task_id: task_id.clone(),
            expected_revision: Some(TaskRevision::from_raw(1)),
            operation: TaskOperation::Start,
        };
        store.apply(start.clone()).await.expect("start");
        let stale = TaskMutationRequest {
            operation_id: TaskOperationId::new(),
            actor: worker.clone(),
            task_id: task_id.clone(),
            expected_revision: Some(TaskRevision::from_raw(1)),
            operation: TaskOperation::Block {
                reason: "stale request".to_owned(),
            },
        };
        assert_eq!(
            store.apply(stale).await.expect_err("stale revision").code(),
            atm_storage::AtmErrorCode::TaskRevisionStale
        );
        let conflict = TaskMutationRequest {
            operation_id,
            actor: worker,
            task_id,
            expected_revision: None,
            operation: TaskOperation::Block {
                reason: "different bytes under same id".to_owned(),
            },
        };
        assert_eq!(
            store
                .apply(conflict)
                .await
                .expect_err("operation id conflict")
                .code(),
            atm_storage::AtmErrorCode::TaskOperationConflict
        );
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let state: String = connection
                    .query_row(
                        "SELECT state FROM tasks_v2 WHERE team = 'task-v2-test' AND task_id = 'v2-cas-task'",
                        [],
                        |row| row.get(0),
                    )
                    .expect("state");
                let events: u64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM task_events_v2 WHERE team = 'task-v2-test' AND task_id = 'v2-cas-task'",
                        [],
                        |row| row.get(0),
                    )
                    .expect("events");
                assert_eq!(state, "active");
                assert_eq!(events, 2, "failed mutations append no event");
                Ok(())
            })
            .expect("inspect cas state");
    }
}
