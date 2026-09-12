#![cfg(test)]

use std::sync::Arc;

use atm_core::test_support::{TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_TEAM};
use atm_http_runtime::CanonicalWriteHandler;
use atm_storage::{MoveTarget, QueuePosition, TaskEventKind};
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

#[tokio::test]
#[serial(env)]
async fn move_head_end_before_via_cli() {
    let f = LoopbackFixture::new("recipient");
    seed(&f).await;
    move_task(&f, "T3", MoveTarget::Head).await;
    move_task(&f, "T3", MoveTarget::End).await;
    move_task(
        &f,
        "T3",
        MoveTarget::Before {
            task_id: "T2".parse().unwrap(),
        },
    )
    .await;
    let mut rows = f
        .task_store()
        .list_tasks(
            &TEST_TEAM.parse().unwrap(),
            Some(&"recipient".parse().unwrap()),
        )
        .unwrap();
    rows.sort_by_key(|row| row.position);
    assert_eq!(
        rows.iter()
            .map(|row| (row.task_id.as_str(), row.position.map(QueuePosition::get)))
            .collect::<Vec<_>>(),
        vec![("T1", Some(1)), ("T3", Some(2)), ("T2", Some(3))]
    );
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
        QueueGetNext(serde_json::Value),
        ReloadRuntimeView,
    }
    let fixtures = [
        serde_json::to_vec(&RequestEnvelope::QueueGetNext(
            atm_core::protocol::QueueGetNextRequest {
                team: TEST_TEAM.parse().unwrap(),
                member: TEST_SENDER.parse().unwrap(),
            },
        ))
        .unwrap(),
        serde_json::to_vec(&RequestEnvelope::ReloadRuntimeView).unwrap(),
    ];
    for fixture in fixtures {
        serde_json::from_slice::<RequestEnvelope>(&fixture).expect("1.6 decodes 1.5 fixture");
        match serde_json::from_slice::<RequestEnvelope15>(&fixture).expect("1.5 fixture") {
            RequestEnvelope15::QueueGetNext(value) => assert!(value.is_object()),
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
