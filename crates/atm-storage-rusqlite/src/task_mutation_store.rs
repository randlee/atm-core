//! Concrete v2 task mutation adapter tests.

#[cfg(test)]
mod tests {
    use crate::SqliteStorageBackend;
    use atm_storage::contract::{Message, MessageKey};
    use atm_storage::schema::{AtmMessageId, MessageEnvelope};
    use atm_storage::types::{AgentName, IsoTimestamp, MemberKey, TaskId, TeamName};
    use atm_storage::{
        PreparedAssignment, PreparedMessage, ReadDeadline, TaskLifecycleState, TaskMutationRequest,
        TaskOperation, TaskOperationId, TaskOutcome, TaskPriority,
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
    async fn active_unique_index_rejects_a_second_start_without_an_event() {
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
        store
            .apply(request(
                lead.clone(),
                first_task.clone(),
                TaskOperation::Start,
            ))
            .await
            .expect("first start");
        let error = store
            .apply(request(lead, second_task.clone(), TaskOperation::Start))
            .await
            .expect_err("second active task must fail");
        assert_eq!(error.code(), atm_storage::AtmErrorCode::TaskActiveConflict);
        backend
            .shared_db_for_test()
            .with_connection(|connection| {
                let events: u64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM task_events_v2 WHERE task_id = ?1",
                        [second_task.as_str()],
                        |row| row.get(0),
                    )
                    .expect("event count");
                assert_eq!(events, 1, "failed start has no lifecycle event");
                Ok(())
            })
            .expect("inspect second task");
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
            .list_logical_tasks(team(), Some(worker.agent().clone()), deadline())
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
}
