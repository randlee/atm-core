use std::collections::BTreeSet;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

use atm_core::GraftConfig;
use atm_core::boundary::PostSendHookEvent;
use atm_core::error::AtmError;
use atm_core::list::ListQuery;
use atm_core::read::ReadQuery;
use atm_core::schema::AtmMessageId;
use atm_core::types::{AckActivationMode, ReadSelection};

use crate::{
    DEFAULT_LIST_LIMIT, GraftObservability, GraftSessionClient, GraftSessionOptions,
    GraftSessionState, HostNudgeInjector, RECEIVE_LOOP_JOIN_DEADLINE, SessionSnapshot,
};

pub(crate) fn load_graft_config(workspace_root: &Path) -> Result<Option<GraftConfig>, AtmError> {
    let config = atm_core::load_atm_config(workspace_root)?;
    Ok(config.map(|config| config.graft))
}

pub(crate) fn read_snapshot(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
) -> Result<SessionSnapshot, AtmError> {
    snapshot
        .read()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| {
            AtmError::daemon_unavailable("graft session snapshot lock poisoned").with_recovery(
                "Restart the embedding host before retrying graft session lifecycle operations.",
            )
        })
}

fn write_snapshot(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    state: GraftSessionState,
) -> Result<(), AtmError> {
    let mut snapshot = snapshot.write().map_err(|_| {
        AtmError::daemon_unavailable("graft session snapshot lock poisoned").with_recovery(
            "Restart the embedding host before retrying graft session lifecycle operations.",
        )
    })?;
    snapshot.state = state;
    Ok(())
}

pub(crate) fn set_session_state(
    snapshot: &Arc<RwLock<SessionSnapshot>>,
    state: GraftSessionState,
    observability: &dyn GraftObservability,
) -> Result<(), AtmError> {
    write_snapshot(snapshot, state)?;
    observability.session_state_changed(&read_snapshot(snapshot)?);
    Ok(())
}

pub(crate) fn join_receive_loop_with_deadline(
    join_handle: JoinHandle<Result<(), AtmError>>,
) -> Result<(), AtmError> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let join_helper = thread::Builder::new()
        .name("atm-graft-receive-loop-join".to_string())
        .spawn(move || {
            let result = match join_handle.join() {
                Ok(result) => result,
                Err(_) => Err(AtmError::daemon_unavailable("graft receive loop panicked")
                    .with_recovery(
                        "Restart the embedding host and atm-daemon before retrying graft mode.",
                    )),
            };
            let _ = result_tx.send(result);
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn graft receive-loop join helper")
                .with_source(source)
                .with_recovery(
                    "Retry graft shutdown after the embedding host can spawn one bounded join helper thread.",
                )
        })?;
    let join_helper_thread_id = join_helper.thread().id();
    match result_rx.recv_timeout(RECEIVE_LOOP_JOIN_DEADLINE) {
        Ok(result) => {
            join_helper.join().map_err(|_| {
                AtmError::daemon_unavailable("graft receive-loop join helper panicked")
                    .with_recovery(
                        "Restart the embedding host and atm-daemon before retrying graft mode.",
                    )
            })?;
            result
        }
        Err(RecvTimeoutError::Timeout) => {
            tracing::debug!(
                timeout_ms = RECEIVE_LOOP_JOIN_DEADLINE.as_millis(),
                thread_id = ?join_helper_thread_id,
                "graft receive-loop join timed out; helper left detached after deadline"
            );
            Err(AtmError::daemon_unavailable(format!(
                "graft receive loop shutdown exceeded the {:?} join deadline",
                RECEIVE_LOOP_JOIN_DEADLINE
            ))
            .with_recovery(
                "Restart the embedding host if the graft receive loop does not shut down within the bounded join deadline.",
            ))
        }
        Err(RecvTimeoutError::Disconnected) => join_helper.join().map_or_else(
            |_| {
                Err(
                    AtmError::daemon_unavailable("graft receive-loop join helper panicked")
                        .with_recovery(
                            "Restart the embedding host and atm-daemon before retrying graft mode.",
                        ),
                )
            },
            |_| {
                Err(AtmError::daemon_unavailable(
                    "graft receive-loop join helper disconnected unexpectedly",
                )
                .with_recovery(
                    "Restart the embedding host and atm-daemon before retrying graft mode.",
                ))
            },
        ),
    }
}

pub(crate) struct ReceiveLoopContext {
    pub(crate) client: Arc<dyn GraftSessionClient>,
    pub(crate) options: GraftSessionOptions,
    pub(crate) home_dir: std::path::PathBuf,
    pub(crate) snapshot: Arc<RwLock<SessionSnapshot>>,
    pub(crate) injector: Arc<dyn HostNudgeInjector>,
    pub(crate) observability: Arc<dyn GraftObservability>,
    pub(crate) stop_rx: Receiver<()>,
}

pub(crate) fn run_receive_loop(ctx: ReceiveLoopContext) -> Result<(), AtmError> {
    let mut delivered_message_ids = BTreeSet::new();
    loop {
        match ctx.stop_rx.recv_timeout(ctx.options.poll_interval()) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => {}
        }

        match poll_once(&ctx, &mut delivered_message_ids) {
            Ok(()) => {
                let _ = set_session_state(
                    &ctx.snapshot,
                    GraftSessionState::Polling,
                    ctx.observability.as_ref(),
                );
            }
            Err(error) => {
                let _ = set_session_state(
                    &ctx.snapshot,
                    GraftSessionState::Degraded,
                    ctx.observability.as_ref(),
                );
                if let Ok(snapshot) = read_snapshot(&ctx.snapshot) {
                    ctx.observability
                        .session_error(&snapshot, "poll_unread_messages", &error);
                }
            }
        }
    }
}

fn poll_once(
    ctx: &ReceiveLoopContext,
    delivered_message_ids: &mut BTreeSet<AtmMessageId>,
) -> Result<(), AtmError> {
    let mut rows = ctx
        .client
        .list_messages(build_unread_list_query(ctx)?)?
        .rows;
    rows.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));

    let mut current_unread_message_ids = BTreeSet::new();
    for row in rows {
        let Some(message_id) = row.message_id else {
            continue;
        };
        current_unread_message_ids.insert(message_id);
        if delivered_message_ids.contains(&message_id) {
            continue;
        }

        let event = read_post_send_event(ctx, message_id)?;
        ctx.injector.inject_nudge(event.clone())?;
        let snapshot = read_snapshot(&ctx.snapshot)?;
        ctx.observability.nudge_delivered(&snapshot, &event);
        delivered_message_ids.insert(message_id);
    }

    delivered_message_ids.retain(|message_id| current_unread_message_ids.contains(message_id));
    Ok(())
}

fn build_unread_list_query(ctx: &ReceiveLoopContext) -> Result<ListQuery, AtmError> {
    ListQuery::new(
        ctx.home_dir.clone(),
        ctx.options.workspace_root().to_path_buf(),
        ctx.options.agent().clone(),
        None,
        ctx.options.team().clone(),
        ReadSelection::Unread,
        false,
        Some(DEFAULT_LIST_LIMIT),
        None,
        None,
        None,
        None,
    )
}

fn build_exact_read_query(
    ctx: &ReceiveLoopContext,
    message_id: AtmMessageId,
) -> Result<ReadQuery, AtmError> {
    let message_id = message_id.to_string();
    ReadQuery::new(
        ctx.home_dir.clone(),
        ctx.options.workspace_root().to_path_buf(),
        ctx.options.agent().clone(),
        None,
        ctx.options.team().clone(),
        ReadSelection::All,
        false,
        false,
        AckActivationMode::ReadOnly,
        Some(message_id.as_str()),
        None,
        None,
        None,
        None,
        None,
    )
}

fn read_post_send_event(
    ctx: &ReceiveLoopContext,
    message_id: AtmMessageId,
) -> Result<PostSendHookEvent, AtmError> {
    let outcome = ctx
        .client
        .read_message(build_exact_read_query(ctx, message_id)?)?;
    let message = outcome.message.ok_or_else(|| {
        AtmError::daemon_unavailable(format!(
            "graft read for message {message_id} returned no selected message"
        ))
        .with_recovery(
            "Retry the graft receive loop after atm-daemon and atm-graft use the same ATM read contract.",
        )
    })?;
    let envelope = message.envelope;
    let durable_message_id = envelope.message_id.ok_or_else(|| {
        AtmError::daemon_unavailable(format!(
            "graft read for message {message_id} returned a durable record without a message_id"
        ))
        .with_recovery(
            "Repair the retained mailbox state so every ATM-authored message keeps its ULID before retrying graft delivery.",
        )
    })?;
    let recipient_team = ctx.options.team().clone();
    Ok(PostSendHookEvent {
        sender: envelope.from,
        sender_team: envelope
            .source_team
            .unwrap_or_else(|| recipient_team.clone()),
        recipient: ctx.options.agent().clone(),
        recipient_team,
        message_id: durable_message_id,
        message: envelope.text,
        requires_ack: envelope.pending_ack_at.is_some() && envelope.acknowledged_at.is_none(),
        is_ack: envelope.acknowledges_message_id.is_some(),
        task_id: envelope.task_id,
        recipient_pane_id: None,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, RwLock, mpsc};
    use std::time::{Duration, Instant};

    use atm_core::boundary::PostSendHookEvent;
    use atm_core::error::AtmError;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::list::{ListOutcome, ListQuery, ListRow};
    use atm_core::protocol::ProtocolErrorEnvelope;
    use atm_core::read::{BucketCounts, ReadOutcome};
    use atm_core::schema::{AtmMessageId, InboxMessage};
    use atm_core::test_support::{TEST_LEAD, TEST_QA, TEST_TEAM};
    use atm_core::types::{AgentName, CommandAction, IsoTimestamp, TeamName};
    use serde_json::json;

    use crate::{
        GraftObservability, GraftSessionClient, GraftSessionOptions, GraftSessionState,
        HostNudgeInjector, SessionSnapshot,
    };

    use super::{ReceiveLoopContext, read_post_send_event, read_snapshot, run_receive_loop};

    #[derive(Debug, Default)]
    struct RecordingClient {
        rows: Mutex<Vec<ListRow>>,
        messages: Mutex<std::collections::HashMap<AtmMessageId, InboxMessage>>,
    }

    impl atm_core::graft::AtmGraftClient for RecordingClient {
        fn send_message(
            &self,
            _request: atm_core::send::SendRequest,
        ) -> Result<atm_core::send::SendOutcome, AtmError> {
            panic!("send_message not used in runtime tests")
        }

        fn read_message(&self, query: atm_core::read::ReadQuery) -> Result<ReadOutcome, AtmError> {
            let message_id = query
                .message_id_filter()
                .copied()
                .expect("runtime tests use exact message id");
            let envelope = self
                .messages
                .lock()
                .expect("messages lock")
                .get(&message_id)
                .cloned()
                .expect("message");
            Ok(serde_json::from_value(json!({
                "action": "read",
                "team": "test-team",
                "agent": "qa-a",
                "selection_mode": "all",
                "mutation_applied": false,
                "count": 1,
                "message": {
                    "bucket": "unread",
                    "class": "unread",
                    "from": envelope.from,
                    "text": envelope.text,
                    "timestamp": envelope.timestamp,
                    "read": envelope.read,
                    "source_team": envelope.source_team,
                    "summary": envelope.summary,
                    "message_id": envelope.message_id,
                    "pendingAckAt": envelope.pending_ack_at,
                    "acknowledgedAt": envelope.acknowledged_at,
                    "acknowledgesMessageId": envelope.acknowledges_message_id,
                    "parentMessageId": envelope.parent_message_id,
                    "threadMode": envelope.thread_mode,
                    "expiresAt": envelope.expires_at,
                    "taskId": envelope.task_id
                },
                "selected_message_id": message_id,
                "match_count": 1,
                "additional_match_count": 0,
                "bucket_counts": {
                    "unread": 1,
                    "pending_ack": 0,
                    "history": 0
                }
            }))
            .expect("read outcome"))
        }

        fn acknowledge_message(
            &self,
            _request: atm_core::ack::AckRequest,
        ) -> Result<atm_core::ack::AckOutcome, AtmError> {
            panic!("acknowledge_message not used in runtime tests")
        }
    }

    impl GraftSessionClient for RecordingClient {
        fn list_messages(&self, _query: ListQuery) -> Result<ListOutcome, AtmError> {
            let rows = self.rows.lock().expect("rows lock").clone();
            let unread = rows.len();
            Ok(ListOutcome {
                action: CommandAction::List,
                team: TeamName::from_validated("test-team"),
                agent: AgentName::from_validated("qa-a"),
                selection_mode: atm_core::types::ReadSelection::Unread,
                history_collapsed: false,
                count: unread,
                rows,
                bucket_counts: BucketCounts {
                    unread,
                    pending_ack: 0,
                    history: 0,
                },
            })
        }
    }

    #[derive(Debug, Default)]
    struct RecordingInjector {
        nudges: Mutex<Vec<PostSendHookEvent>>,
    }

    impl HostNudgeInjector for RecordingInjector {
        fn inject_nudge(&self, nudge: PostSendHookEvent) -> Result<(), AtmError> {
            self.nudges.lock().expect("nudges lock").push(nudge);
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct NoopObservability;

    impl GraftObservability for NoopObservability {}

    fn test_home_dir() -> PathBuf {
        std::env::temp_dir().join("atm-graft-runtime-test-home")
    }

    fn test_workspace_dir() -> PathBuf {
        std::env::temp_dir().join("atm-graft-runtime-test-workspace")
    }

    fn session_options() -> GraftSessionOptions {
        GraftSessionOptions::for_current_process(
            test_workspace_dir(),
            TeamName::from_validated(TEST_TEAM),
            AgentName::from_validated(TEST_QA),
        )
        .with_poll_interval(Duration::from_millis(1))
    }

    fn wait_until(description: &str, predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if predicate() {
                return;
            }
            std::thread::yield_now();
        }
        panic!("timed out waiting for {description}");
    }

    fn unread_row(message_id: AtmMessageId) -> ListRow {
        ListRow {
            message_id: Some(message_id),
            summary: "review failing smoke lane".to_string(),
            from: AgentName::from_validated(TEST_LEAD),
            timestamp: IsoTimestamp::now(),
            read: false,
            pending_ack: false,
            task_id: None,
        }
    }

    fn unread_message(message_id: AtmMessageId) -> InboxMessage {
        InboxMessage {
            from: AgentName::from_validated(TEST_LEAD),
            text: "review failing smoke lane".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TeamName::from_validated(TEST_TEAM)),
            summary: Some("review failing smoke lane".to_string()),
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn read_post_send_event_projects_read_outcome_into_shared_event() {
        let message_id = AtmMessageId::new();
        let client = Arc::new(RecordingClient::default());
        client
            .messages
            .lock()
            .expect("messages lock")
            .insert(message_id, unread_message(message_id));
        let ctx = ReceiveLoopContext {
            client,
            options: session_options(),
            home_dir: test_home_dir(),
            snapshot: Arc::new(RwLock::new(SessionSnapshot {
                team: TeamName::from_validated(TEST_TEAM),
                agent: AgentName::from_validated(TEST_QA),
                state: GraftSessionState::Polling,
            })),
            injector: Arc::new(RecordingInjector::default()),
            observability: Arc::new(NoopObservability),
            stop_rx: mpsc::channel().1,
        };

        let event = read_post_send_event(&ctx, message_id).expect("event");

        assert_eq!(event.message_id, message_id);
        assert_eq!(event.sender, AgentName::from_validated(TEST_LEAD));
        assert_eq!(event.sender_team, TeamName::from_validated(TEST_TEAM));
        assert_eq!(event.recipient, AgentName::from_validated(TEST_QA));
        assert_eq!(event.recipient_team, TeamName::from_validated(TEST_TEAM));
        assert_eq!(event.message, "review failing smoke lane");
    }

    #[test]
    fn receive_loop_polls_unread_messages_and_injects_each_message_once() {
        let message_id = AtmMessageId::new();
        let client = Arc::new(RecordingClient::default());
        client
            .rows
            .lock()
            .expect("rows lock")
            .push(unread_row(message_id));
        client
            .messages
            .lock()
            .expect("messages lock")
            .insert(message_id, unread_message(message_id));
        let injector = Arc::new(RecordingInjector::default());
        let (stop_tx, stop_rx) = mpsc::channel();
        let snapshot = Arc::new(RwLock::new(SessionSnapshot {
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_QA),
            state: GraftSessionState::Polling,
        }));
        let ctx = ReceiveLoopContext {
            client,
            options: session_options(),
            home_dir: test_home_dir(),
            snapshot: Arc::clone(&snapshot),
            injector: Arc::clone(&injector) as Arc<dyn HostNudgeInjector>,
            observability: Arc::new(NoopObservability),
            stop_rx,
        };

        let join = std::thread::spawn(move || run_receive_loop(ctx));
        wait_until("graft receive-loop delivery", || {
            injector.nudges.lock().expect("nudges lock").len() == 1
        });
        stop_tx.send(()).expect("stop");
        join.join().expect("join").expect("receive loop");

        let nudges = injector.nudges.lock().expect("nudges lock");
        assert_eq!(nudges.len(), 1);
        assert_eq!(nudges[0].message_id, message_id);
        assert_eq!(
            read_snapshot(&snapshot).expect("snapshot").state,
            GraftSessionState::Polling
        );
    }

    #[test]
    fn receive_loop_marks_session_degraded_when_poll_fails() {
        #[derive(Debug, Default)]
        struct FailingClient;

        impl atm_core::graft::AtmGraftClient for FailingClient {
            fn send_message(
                &self,
                _request: atm_core::send::SendRequest,
            ) -> Result<atm_core::send::SendOutcome, AtmError> {
                panic!("send_message not used in runtime tests")
            }

            fn read_message(
                &self,
                _query: atm_core::read::ReadQuery,
            ) -> Result<ReadOutcome, AtmError> {
                panic!("read_message not used in runtime tests")
            }

            fn acknowledge_message(
                &self,
                _request: atm_core::ack::AckRequest,
            ) -> Result<atm_core::ack::AckOutcome, AtmError> {
                panic!("acknowledge_message not used in runtime tests")
            }
        }

        impl GraftSessionClient for FailingClient {
            fn list_messages(&self, _query: ListQuery) -> Result<ListOutcome, AtmError> {
                Err(ProtocolErrorEnvelope {
                    code: AtmErrorCode::DaemonUnavailable,
                    message: "simulated list failure".to_string(),
                    recovery: vec!["Retry after the daemon recovers.".to_string()],
                }
                .into_atm_error())
            }
        }

        let injector = Arc::new(RecordingInjector::default());
        let (stop_tx, stop_rx) = mpsc::channel();
        let snapshot = Arc::new(RwLock::new(SessionSnapshot {
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_QA),
            state: GraftSessionState::Polling,
        }));
        let ctx = ReceiveLoopContext {
            client: Arc::new(FailingClient),
            options: session_options(),
            home_dir: test_home_dir(),
            snapshot: Arc::clone(&snapshot),
            injector: Arc::clone(&injector) as Arc<dyn HostNudgeInjector>,
            observability: Arc::new(NoopObservability),
            stop_rx,
        };

        let join = std::thread::spawn(move || run_receive_loop(ctx));
        wait_until("graft receive-loop degraded state", || {
            read_snapshot(&snapshot)
                .map(|state| state.state == GraftSessionState::Degraded)
                .unwrap_or(false)
        });
        stop_tx.send(()).expect("stop");
        join.join().expect("join").expect("receive loop");

        assert_eq!(
            read_snapshot(&snapshot).expect("snapshot").state,
            GraftSessionState::Degraded
        );
    }
}
