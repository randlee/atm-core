#![cfg(test)]

use std::sync::Arc;

use atm_core::test_support::{TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_TEAM};
use atm_http_runtime::CanonicalWriteHandler;
use atm_storage::{
    MemberKey, MoveTarget, QueuePosition, ReminderOutcome, TaskEventKind, TaskOp, TaskRow,
    TaskState,
};
use serial_test::serial;

use super::*;
use crate::commands::send::require_daemon_api;
use crate::composition::tests::LoopbackFixture;

fn caller() -> CallerArgs {
    CallerArgs {
        actor: Some(TEST_SENDER.into()),
        team: Some(TEST_TEAM.into()),
    }
}

fn assign(task: &str) -> TaskAssignCommand {
    TaskAssignCommand {
        assignee: TEST_RECIPIENT_ADDRESS.parse().unwrap(),
        task_id: Some(task.parse().unwrap()),
        before: None,
        head: false,
        message: MessageSourceArgs {
            text: Some(format!("assign {task}")),
            file: None,
            stdin: false,
            template: None,
            vars: None,
        },
        json: true,
        caller: caller(),
    }
}

async fn seed(f: &LoopbackFixture) {
    for task in ["T1", "T2", "T3"] {
        let obs = CliObservability::fallback();
        let composition = f.composition(&obs);
        assign(task)
            .execute(&composition, f.home_dir.clone(), f.current_dir.clone())
            .await
            .unwrap();
    }
}

async fn move_task(f: &LoopbackFixture, task: &str, target: MoveTarget) -> String {
    let (head, end, before) = match target {
        MoveTarget::Head => (true, false, None),
        MoveTarget::End => (false, true, None),
        MoveTarget::Before { task_id } => (false, false, Some(task_id)),
    };
    let command = TaskMoveCommand {
        task_id: task.parse().unwrap(),
        head,
        end,
        before,
        json: true,
        caller: caller(),
    };
    let obs = CliObservability::fallback();
    let composition = f.composition(&obs);
    command
        .execute(&composition, resolve_context(&caller()).unwrap())
        .await
        .unwrap()
}

async fn list_tasks(f: &LoopbackFixture) -> Vec<TaskRow> {
    let command = TaskListCommand {
        all: false,
        json: true,
        caller: CallerArgs {
            actor: Some("recipient".into()),
            team: Some(TEST_TEAM.into()),
        },
    };
    let context = resolve_context(&command.caller).unwrap();
    let obs = CliObservability::fallback();
    serde_json::from_str(
        &command
            .execute(
                &f.composition(&obs),
                context,
                f.home_dir.clone(),
                f.current_dir.clone(),
            )
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
#[serial(env)]
async fn move_head_end_before_via_cli() {
    let f = LoopbackFixture::new("recipient");
    seed(&f).await;
    let assigned_at = list_tasks(&f)
        .await
        .into_iter()
        .map(|row| (row.task_id, row.assigned_at))
        .collect::<std::collections::BTreeMap<_, _>>();
    f.task_store()
        .record_reminder(
            &MemberKey::new(TEST_TEAM.parse().unwrap(), "recipient".parse().unwrap()),
            &"T1".parse().unwrap(),
            atm_storage::IsoTimestamp::now(),
            ReminderOutcome::Emitted,
        )
        .unwrap();
    let mut start = atm_core::send::SendRequest::new(
        f.home_dir.clone(),
        f.current_dir.clone(),
        "atm-daemon".parse().unwrap(),
        TEST_RECIPIENT_ADDRESS,
        TEST_TEAM.parse().unwrap(),
        atm_core::send::SendMessageSource::Inline("start T1".into()),
        None,
        false,
        Some("T1".parse().unwrap()),
        false,
    )
    .unwrap();
    start.task_op = Some(TaskOp::Start);
    let obs = CliObservability::fallback();
    f.composition(&obs).send(start).await.unwrap();
    move_task(&f, "T3", MoveTarget::Head).await;
    let after_head = list_tasks(&f).await;
    assert_eq!(
        after_head
            .iter()
            .find(|row| row.task_id.as_str() == "T1")
            .unwrap()
            .state,
        TaskState::Active
    );
    assert_eq!(
        after_head
            .iter()
            .find(|row| row.task_id.as_str() == "T3")
            .unwrap()
            .position
            .map(QueuePosition::get),
        Some(2)
    );
    move_task(&f, "T3", MoveTarget::End).await;
    move_task(
        &f,
        "T3",
        MoveTarget::Before {
            task_id: "T2".parse().unwrap(),
        },
    )
    .await;
    let mut rows = list_tasks(&f).await;
    rows.sort_by_key(|row| row.position);
    assert_eq!(
        rows.iter()
            .map(|row| (row.task_id.as_str(), row.position.map(QueuePosition::get)))
            .collect::<Vec<_>>(),
        vec![("T1", Some(1)), ("T3", Some(2)), ("T2", Some(3))]
    );
    for row in rows {
        assert_eq!(row.assigned_at, assigned_at[&row.task_id]);
    }
}

#[test]
fn task_move_to_pre_1_6_0_daemon_is_refused_before_send() {
    let verdict = atm_core::protocol::CompatibilityVerdict::Compatible {
        daemon_release: atm_core::protocol::ReleaseVersion::parse("1.5.14").unwrap(),
        daemon_schema_version: 1,
        daemon_http_api_version: HttpApiVersion::parse("1.5.0").unwrap(),
    };
    let error = require_daemon_api(
        &verdict,
        HttpApiVersion::parse("1.6.0").unwrap(),
        "task move",
    )
    .unwrap_err();
    assert_eq!(crate::exit_code_for_error(&anyhow::Error::from(error)), 4);
}

#[tokio::test]
#[serial(env)]
async fn peer_ingress_rejects_task_move_explicitly() {
    let f = LoopbackFixture::new("recipient");
    seed(&f).await;
    let router = f.router(Arc::new(atm_core::observability::NullObservability));
    let error = router
        .dispatch(
            atm_core::api::ApiRequest::new(RequestEnvelope::TaskMove(TaskMoveRequest {
                caller_identity: TEST_SENDER.parse().unwrap(),
                caller_team: TEST_TEAM.parse().unwrap(),
                task_id: "T1".parse().unwrap(),
                target: MoveTarget::End,
            })),
            atm_core::AuthenticatedIngress::Peer,
            atm_core::RequestDeadline::after(std::time::Duration::from_secs(1)),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .message()
            .contains("only through authenticated local HTTP adapters")
    );
    assert_ne!(
        f.task_store()
            .list_task_events(&TEST_TEAM.parse().unwrap(), &"T1".parse().unwrap(), None)
            .unwrap()
            .last()
            .unwrap()
            .event,
        TaskEventKind::Moved
    );
}

#[test]
fn protocol_1_5_0_fixtures_decode_on_1_6_0() {
    #[derive(Debug, serde::Deserialize)]
    enum RequestEnvelope15 {
        Write(serde_json::Value),
        CompatibilityPreflight(serde_json::Value),
        Heartbeat(serde_json::Value),
        QueueGetNext(serde_json::Value),
        GraftReceiverRegister(serde_json::Value),
        GraftReceiverRefresh(serde_json::Value),
        GraftReceiverUnregister(serde_json::Value),
        GraftReceiverLookup(serde_json::Value),
        List(serde_json::Value),
        Peek(serde_json::Value),
        Receive(serde_json::Value),
        Clear(serde_json::Value),
        Doctor(serde_json::Value),
        Search(serde_json::Value),
        ReloadRuntimeView,
    }
    let team: TeamName = TEST_TEAM.parse().unwrap();
    let sender: AgentName = TEST_SENDER.parse().unwrap();
    let owner = atm_core::protocol::OwnerGeneration::new("01J00000000000000000000000").unwrap();
    let fixtures = vec![
        RequestEnvelope::Write(Box::new(
            atm_core::send::SendRequest::new(
                ".".into(),
                ".".into(),
                sender.clone(),
                TEST_RECIPIENT_ADDRESS,
                team.clone(),
                atm_core::send::SendMessageSource::Inline("fixture".into()),
                None,
                false,
                None,
                false,
            )
            .unwrap(),
        )),
        RequestEnvelope::CompatibilityPreflight(atm_core::protocol::CompatibilityPreflight {
            client_release: atm_core::protocol::ReleaseVersion::parse("1.5.14").unwrap(),
            cli_schema_version: 1,
            http_api_version: HttpApiVersion::parse("1.5.0").unwrap(),
        }),
        RequestEnvelope::Heartbeat(atm_core::protocol::TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: sender.clone(),
            pid: 42,
            observed_at: atm_storage::IsoTimestamp::now(),
            activity: atm_core::protocol::HeartbeatActivity::Idle,
            session_id: None,
        }),
        RequestEnvelope::QueueGetNext(atm_core::protocol::QueueGetNextRequest {
            team: team.clone(),
            member: sender.clone(),
        }),
        RequestEnvelope::GraftReceiverRegister(atm_core::protocol::GraftReceiverRegistration {
            team: team.clone(),
            agent: sender.clone(),
            endpoint: "127.0.0.1:43101".parse().unwrap(),
            capability: atm_core::protocol::LocalCapability::generate().unwrap(),
            owner_generation: owner.clone(),
        }),
        RequestEnvelope::GraftReceiverRefresh(atm_core::protocol::GraftReceiverRefreshRequest {
            team: team.clone(),
            agent: sender.clone(),
            owner_generation: owner.clone(),
        }),
        RequestEnvelope::GraftReceiverUnregister(atm_core::protocol::GraftReceiverUnregistration {
            team: team.clone(),
            agent: sender.clone(),
            owner_generation: owner,
        }),
        RequestEnvelope::GraftReceiverLookup {
            team: team.clone(),
            agent: sender.clone(),
        },
        RequestEnvelope::List(
            atm_core::list::ListQuery::new(
                ".".into(),
                ".".into(),
                sender.clone(),
                None,
                team.clone(),
                atm_core::types::ReadSelection::Unread,
                false,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ),
        RequestEnvelope::Peek(
            atm_core::read::PeekQuery::new(
                ".".into(),
                ".".into(),
                sender.clone(),
                None,
                team.clone(),
                atm_core::types::ReadSelection::Unread,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ),
        RequestEnvelope::Receive(
            atm_core::read::ReadQuery::new(
                ".".into(),
                ".".into(),
                sender.clone(),
                None,
                team.clone(),
                atm_core::types::ReadSelection::Unread,
                false,
                true,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ),
        RequestEnvelope::Clear(atm_core::clear::ClearQuery {
            home_dir: ".".into(),
            current_dir: ".".into(),
            caller_identity: sender.clone(),
            caller_team: team.clone(),
            older_than: None,
            idle_only: false,
            dry_run: true,
        }),
        RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
            home_dir: ".".into(),
            current_dir: ".".into(),
            caller_team: Some(team),
            caller_identity: Some(sender),
            ..Default::default()
        }),
        RequestEnvelope::Search(Box::new(atm_core::search::SearchRequest {
            query: atm_core::search::SearchInput::default(),
            lifecycle: None,
        })),
        RequestEnvelope::ReloadRuntimeView,
    ];
    assert_eq!(fixtures.len(), 15, "1.5 exposed fifteen request variants");
    for request in fixtures {
        let fixture = serde_json::to_vec(&request).unwrap();
        serde_json::from_slice::<RequestEnvelope>(&fixture).expect("1.6 decodes 1.5 fixture");
        match serde_json::from_slice::<RequestEnvelope15>(&fixture).expect("1.5 fixture") {
            RequestEnvelope15::Write(value)
            | RequestEnvelope15::CompatibilityPreflight(value)
            | RequestEnvelope15::Heartbeat(value)
            | RequestEnvelope15::QueueGetNext(value)
            | RequestEnvelope15::GraftReceiverRegister(value)
            | RequestEnvelope15::GraftReceiverRefresh(value)
            | RequestEnvelope15::GraftReceiverUnregister(value)
            | RequestEnvelope15::GraftReceiverLookup(value)
            | RequestEnvelope15::List(value)
            | RequestEnvelope15::Peek(value)
            | RequestEnvelope15::Receive(value)
            | RequestEnvelope15::Clear(value)
            | RequestEnvelope15::Doctor(value)
            | RequestEnvelope15::Search(value) => assert!(value.is_object()),
            RequestEnvelope15::ReloadRuntimeView => {}
        }
    }

    let task_move = serde_json::to_vec(&RequestEnvelope::TaskMove(TaskMoveRequest {
        caller_identity: TEST_SENDER.parse().unwrap(),
        caller_team: TEST_TEAM.parse().unwrap(),
        task_id: "T1".parse().unwrap(),
        target: MoveTarget::Head,
    }))
    .expect("task move fixture");
    let error = serde_json::from_slice::<RequestEnvelope15>(&task_move)
        .expect_err("1.5 enum must reject the 1.6-only variant");
    assert!(error.to_string().contains("unknown variant `TaskMove`"));
}
