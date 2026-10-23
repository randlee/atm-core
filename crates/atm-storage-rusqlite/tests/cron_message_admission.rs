//! Cron message admission rechecks the opaque run's generation fence in the
//! same ordered SQLite transaction as insertion and duplicate classification.

use std::path::PathBuf;

use atm_storage::schema::{AtmMessageId, MessageEnvelope};
use atm_storage::{
    AgentName, CronGeneration, CronScheduleId, IsoTimestamp, Message, MessageKey,
    MessageWriteOrigin, MoveTarget, NewCronSchedule, PhaseId, StopCondition, TaskId, TeamName,
};
use atm_storage_rusqlite::SqliteStorageBackend;
use chrono::{TimeZone, Utc};
use tempfile::TempDir;

fn stamp(day: u32) -> IsoTimestamp {
    IsoTimestamp::from_datetime(
        Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0)
            .single()
            .expect("fixed timestamp"),
    )
}

struct Harness {
    _directory: TempDir,
    backend: SqliteStorageBackend,
    team: TeamName,
    phase: PhaseId,
}

impl Harness {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("tempdir");
        let backend =
            SqliteStorageBackend::new(directory.path().join("mail.db")).expect("sqlite backend");
        Self {
            _directory: directory,
            backend,
            team: "test-team".parse().expect("team"),
            phase: PhaseId::parse("bk").expect("phase"),
        }
    }

    fn start_schedule(&self) -> atm_storage::CronSchedule {
        self.backend
            .cron_schedule_store()
            .start(NewCronSchedule {
                team: self.team.clone(),
                phase: self.phase.clone(),
                name: "nightly".to_owned(),
                cron_expr: "*/5 * * * *".to_owned(),
                module_path: PathBuf::from("/hooks/nightly.py"),
                cwd: PathBuf::from("/repo"),
                args: Vec::new(),
                timeout_ms: 30_000,
                jitter_s: 0,
                custom: serde_json::Map::new(),
                stop: StopCondition::Hook,
                created_by: "lead".parse().expect("creator"),
                created_at: stamp(1),
            })
            .expect("active schedule")
    }

    fn message(&self, task: &str) -> Message {
        let id = AtmMessageId::new();
        Message {
            team: self.team.clone(),
            agent: "alice".parse::<AgentName>().expect("recipient"),
            message_key: MessageKey::from(id),
            envelope: MessageEnvelope {
                from: "lead".parse().expect("sender"),
                source_chat_id: None,
                text: format!("assign {task}"),
                timestamp: stamp(2),
                read: false,
                source_team: Some(self.team.clone()),
                destination_chat_id: None,
                summary: None,
                message_id: Some(id),
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: Some(task.parse::<TaskId>().expect("task id")),
                placement: None,
                task_op: None,
                task_complete: None,
                extra: serde_json::Map::new(),
            },
        }
    }

    fn cron_origin(id: CronScheduleId, generation: CronGeneration) -> MessageWriteOrigin {
        MessageWriteOrigin::CronRun {
            schedule_id: id,
            generation,
        }
    }
}

#[test]
fn active_cron_generation_admits_once_and_rejects_duplicate_after_stop_or_restart() {
    let h = Harness::new();
    let schedules = h.backend.cron_schedule_store();
    let first = h.start_schedule();
    let message = h.message("BK-CRON-1");
    let origin = Harness::cron_origin(first.id, first.generation);

    let inserted = h
        .backend
        .message_store()
        .admit_message_with_provenance(&message, origin)
        .expect("active generation admitted")
        .operation
        .expect("task admission");
    assert!(inserted.existing.is_none());
    assert!(
        h.backend
            .task_store()
            .load_task(&h.team, &"BK-CRON-1".parse().unwrap())
            .expect("cron local task effect")
            .is_some()
    );

    let duplicate = h
        .backend
        .message_store()
        .admit_message_with_provenance(&message, origin)
        .expect("same active generation can resolve a retry")
        .operation
        .expect("duplicate admission");
    assert!(duplicate.existing.is_some());

    assert!(
        schedules
            .record_stop(first.id, first.generation, stamp(3), "test stop")
            .expect("stop current run")
    );
    let stopped = h
        .backend
        .message_store()
        .admit_message_with_provenance(&message, origin)
        .expect("rejection is returned as committed task outcome")
        .operation
        .expect_err("stopped generation cannot resolve even an existing id");
    assert!(
        stopped
            .to_string()
            .contains("stale or its schedule is no longer active")
    );

    let restarted = schedules
        .start(NewCronSchedule {
            team: h.team.clone(),
            phase: h.phase.clone(),
            name: "nightly".to_owned(),
            cron_expr: "*/5 * * * *".to_owned(),
            module_path: PathBuf::from("/hooks/nightly.py"),
            cwd: PathBuf::from("/repo"),
            args: Vec::new(),
            timeout_ms: 30_000,
            jitter_s: 0,
            custom: serde_json::Map::new(),
            stop: StopCondition::Hook,
            created_by: "lead".parse().expect("creator"),
            created_at: stamp(4),
        })
        .expect("restart increments generation");
    assert_eq!(restarted.id, first.id);
    assert_eq!(restarted.generation.get(), first.generation.get() + 1);
    let stale = h
        .backend
        .message_store()
        .admit_message_with_provenance(&h.message("BK-CRON-2"), origin)
        .expect("rejection is returned as committed task outcome")
        .operation
        .expect_err("old generation cannot insert after restart");
    assert!(
        stale
            .to_string()
            .contains("stale or its schedule is no longer active")
    );

    h.backend
        .message_store()
        .admit_message_with_provenance(
            &h.message("BK-CRON-3"),
            Harness::cron_origin(restarted.id, restarted.generation),
        )
        .expect("new generation admitted")
        .operation
        .expect("new generation task admission");
}

#[test]
fn local_and_peer_message_task_effects_remain_distinct() {
    let h = Harness::new();
    let local = h.message("BK-LOCAL");
    h.backend
        .message_store()
        .admit_message_with_provenance(&local, MessageWriteOrigin::Local)
        .expect("local message")
        .operation
        .expect("local task effect");
    assert!(
        h.backend
            .task_store()
            .load_task(&h.team, &"BK-LOCAL".parse().unwrap())
            .expect("load local task")
            .is_some()
    );

    let peer = h.message("BK-PEER");
    h.backend
        .message_store()
        .admit_message_with_provenance(&peer, MessageWriteOrigin::Peer)
        .expect("peer message")
        .operation
        .expect("peer does not apply task effect");
    assert!(
        h.backend
            .task_store()
            .load_task(&h.team, &"BK-PEER".parse().unwrap())
            .expect("load peer task")
            .is_none()
    );
}

#[test]
fn task_move_generation_fence_is_checked_inside_writer_before_mutation() {
    let h = Harness::new();
    let first_task = h.message("BK-MOVE-1");
    let second_task = h.message("BK-MOVE-2");
    for message in [&first_task, &second_task] {
        h.backend
            .message_store()
            .admit_message_with_provenance(message, MessageWriteOrigin::Local)
            .expect("local task assignment")
            .operation
            .expect("task assigned");
    }
    let schedules = h.backend.cron_schedule_store();
    let active = h.start_schedule();
    let origin = Harness::cron_origin(active.id, active.generation);
    h.backend
        .task_store()
        .move_task_with_origin(
            &h.team,
            &"BK-MOVE-1".parse().unwrap(),
            &"lead".parse().unwrap(),
            &MoveTarget::End,
            stamp(2),
            origin,
        )
        .expect("active fenced task move")
        .operation
        .expect("active task move applied");
    let before = h
        .backend
        .task_store()
        .open_tasks(&atm_storage::MemberKey::new(
            h.team.clone(),
            "alice".parse().unwrap(),
        ))
        .expect("tasks before stale attempt");
    assert_eq!(before[0].task_id.as_str(), "BK-MOVE-2");

    assert!(
        schedules
            .record_stop(active.id, active.generation, stamp(3), "test stop")
            .expect("stop active schedule")
    );
    let stale = h
        .backend
        .task_store()
        .move_task_with_origin(
            &h.team,
            &"BK-MOVE-2".parse().unwrap(),
            &"lead".parse().unwrap(),
            &MoveTarget::End,
            stamp(4),
            origin,
        )
        .expect("stale rejection outcome is committed without task effects")
        .operation
        .expect_err("stopped generation cannot move tasks");
    assert!(
        stale
            .to_string()
            .contains("stale or its schedule is no longer active")
    );
    let after = h
        .backend
        .task_store()
        .open_tasks(&atm_storage::MemberKey::new(
            h.team.clone(),
            "alice".parse().unwrap(),
        ))
        .expect("tasks after stale attempt");
    assert_eq!(
        after
            .iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        "stale generation must not change task positions"
    );
}

struct AckReplyBuilder {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl atm_storage::AcknowledgementReplyBuilder for AckReplyBuilder {
    fn build_reply(&self, source: &Message) -> Result<Message, atm_storage::AtmError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut reply = source.clone();
        let id = AtmMessageId::new();
        reply.message_key = MessageKey::from(id);
        reply.envelope.message_id = Some(id);
        reply.envelope.from = "alice".parse().expect("ack actor");
        reply.envelope.text = "acknowledged".to_owned();
        reply.envelope.requires_ack = false;
        reply.envelope.pending_ack_at = None;
        reply.envelope.acknowledged_at = None;
        reply.envelope.acknowledges_message_id = source.envelope.message_id;
        reply.envelope.parent_message_id = source.envelope.message_id;
        Ok(reply)
    }
}

fn pending_ack_source(h: &Harness) -> Message {
    let mut source = h.message("BK-ACK");
    source.envelope.task_id = None;
    source.envelope.requires_ack = true;
    source.envelope.pending_ack_at = Some(stamp(2));
    h.backend
        .message_store()
        .save_message(&source)
        .expect("pending ack source persisted");
    source
}

#[tokio::test]
async fn cron_acknowledgement_requires_current_active_generation_before_reply_builder() {
    use atm_storage::AcknowledgementSource;

    let h = Harness::new();
    let schedule = h.start_schedule();
    let source = pending_ack_source(&h);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let origin = Harness::cron_origin(schedule.id, schedule.generation);
    let committed = h
        .backend
        .async_message_store()
        .acknowledge_message_atomically_with_origin_async(
            AcknowledgementSource {
                team: source.team.clone(),
                agent: source.agent.clone(),
                message_id: source.envelope.message_id.expect("message id"),
            },
            std::sync::Arc::new(AckReplyBuilder {
                calls: calls.clone(),
            }),
            origin,
        )
        .await
        .expect("current cron ACK admitted")
        .operation
        .expect("ack transaction committed");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(committed.source.envelope.acknowledged_at.is_some());
    assert!(committed.source.envelope.pending_ack_at.is_none());
    assert!(
        h.backend
            .message_store()
            .load_message(&committed.reply.message_key)
            .expect("load ack reply")
            .is_some()
    );
}

#[tokio::test]
async fn cron_acknowledgement_stale_or_stopped_fence_has_no_partial_effects() {
    use atm_storage::AcknowledgementSource;

    for restart in [false, true] {
        let h = Harness::new();
        let schedules = h.backend.cron_schedule_store();
        let schedule = h.start_schedule();
        let source = pending_ack_source(&h);
        if restart {
            schedules
                .record_stop(schedule.id, schedule.generation, stamp(3), "restart")
                .expect("end before restart");
            schedules
                .start(NewCronSchedule {
                    team: h.team.clone(),
                    phase: h.phase.clone(),
                    name: "nightly".to_owned(),
                    cron_expr: "*/5 * * * *".to_owned(),
                    module_path: PathBuf::from("/hooks/nightly.py"),
                    cwd: PathBuf::from("/repo"),
                    args: Vec::new(),
                    timeout_ms: 30_000,
                    jitter_s: 0,
                    custom: serde_json::Map::new(),
                    stop: StopCondition::Hook,
                    created_by: "lead".parse().expect("creator"),
                    created_at: stamp(4),
                })
                .expect("new active generation");
        } else {
            schedules
                .record_stop(schedule.id, schedule.generation, stamp(3), "stop")
                .expect("stop schedule");
        }
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let error = h
            .backend
            .async_message_store()
            .acknowledge_message_atomically_with_origin_async(
                AcknowledgementSource {
                    team: source.team.clone(),
                    agent: source.agent.clone(),
                    message_id: source.envelope.message_id.expect("message id"),
                },
                std::sync::Arc::new(AckReplyBuilder {
                    calls: calls.clone(),
                }),
                Harness::cron_origin(schedule.id, schedule.generation),
            )
            .await
            .expect("stale rejection returned as committed outcome")
            .operation
            .expect_err("stopped or stale generation must reject ACK");
        assert!(
            error
                .to_string()
                .contains("stale or its schedule is no longer active")
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let persisted = h
            .backend
            .message_store()
            .load_message(&source.message_key)
            .expect("load pending source")
            .expect("source remains persisted");
        assert!(persisted.envelope.pending_ack_at.is_some());
        assert!(persisted.envelope.acknowledged_at.is_none());
    }
}

#[tokio::test]
async fn delayed_cron_read_and_clear_recheck_generation_before_any_mutation() {
    let h = Harness::new();
    let schedules = h.backend.cron_schedule_store();
    let schedule = h.start_schedule();
    let stale_origin = Harness::cron_origin(schedule.id, schedule.generation);
    let store = h.backend.async_message_store();

    let read_message = h.message("BK-READ-FENCE");
    h.backend
        .message_store()
        .save_message(&read_message)
        .expect("seed read target");
    schedules
        .record_stop(
            schedule.id,
            schedule.generation,
            stamp(3),
            "controlled stop",
        )
        .expect("stop before delayed read transition applies");

    let read_error = store
        .apply_read_display_state_with_origin_async(
            atm_storage::MailboxScope::new(h.team.clone(), read_message.agent.clone()),
            vec![read_message.message_key.clone()],
            Some(stamp(3)),
            stale_origin,
        )
        .await
        .expect_err("delayed read transition must reject the stopped generation");
    assert!(
        read_error
            .to_string()
            .contains("stale or its schedule is no longer active")
    );
    let unchanged = h
        .backend
        .message_store()
        .load_message(&read_message.message_key)
        .expect("load read target")
        .expect("read target remains stored");
    assert!(
        !unchanged.envelope.read,
        "stale read must not mark the message"
    );

    let clear_message = h.message("BK-CLEAR-FENCE");
    h.backend
        .message_store()
        .save_message(&clear_message)
        .expect("seed clear target");
    let clear_error = h
        .backend
        .message_store()
        .delete_message_with_origin(&clear_message.message_key, stale_origin)
        .expect_err("delayed clear must reject the stopped generation");
    assert!(
        clear_error
            .to_string()
            .contains("stale or its schedule is no longer active")
    );
    assert!(
        h.backend
            .message_store()
            .load_message(&clear_message.message_key)
            .expect("load clear target")
            .is_some(),
        "stale clear must not delete the message"
    );

    let current = schedules
        .start(NewCronSchedule {
            team: h.team.clone(),
            phase: h.phase.clone(),
            name: "nightly".to_owned(),
            cron_expr: "*/5 * * * *".to_owned(),
            module_path: PathBuf::from("/hooks/nightly.py"),
            cwd: PathBuf::from("/repo"),
            args: Vec::new(),
            timeout_ms: 30_000,
            jitter_s: 0,
            custom: serde_json::Map::new(),
            stop: StopCondition::Hook,
            created_by: "lead".parse().expect("creator"),
            created_at: stamp(4),
        })
        .expect("start current generation");
    h.backend
        .message_store()
        .delete_message_with_origin(
            &clear_message.message_key,
            Harness::cron_origin(current.id, current.generation),
        )
        .expect("current generation can clear its target");
    assert!(
        h.backend
            .message_store()
            .load_message(&clear_message.message_key)
            .expect("confirm cleared message")
            .is_none()
    );
}
