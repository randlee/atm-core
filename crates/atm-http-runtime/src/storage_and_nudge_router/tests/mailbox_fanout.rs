use std::sync::Arc;

use atm_core::api::{ApiRequest, AuthenticatedIngress, RequestDeadline};
use atm_core::doctor::DoctorQuery;
use atm_core::error::AtmError;
use atm_core::observability::NullObservability;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_runtime::{
    DoctorProjection, DoctorProjectionConfig, DoctorProjectionContext, HandoffConfig,
    StorageDoctorProjection,
};
use atm_runtime_test_support::open_sqlite_boundary;

use super::*;

struct BaseOnlyDoctorProjection(StorageDoctorProjection);

#[async_trait::async_trait]
impl DoctorProjection for BaseOnlyDoctorProjection {
    async fn project(
        &self,
        query: DoctorQuery,
        context: DoctorProjectionContext,
        _deadline: RequestDeadline,
    ) -> Result<atm_core::doctor::DoctorReport, AtmError> {
        self.0
            .project_without_deadline_for_tests(query, context)
            .await
    }
}

fn message_queries(
    root: &std::path::Path,
) -> (
    atm_core::list::ListQuery,
    atm_core::read::PeekQuery,
    atm_core::read::ReadQuery,
) {
    let recipient: atm_core::types::AgentName = "recipient".parse().expect("recipient");
    let team: atm_core::types::TeamName = "test-team".parse().expect("team");
    let list = atm_core::list::ListQuery::new(
        root.to_path_buf(),
        root.to_path_buf(),
        recipient.clone(),
        None,
        team.clone(),
        atm_core::types::ReadSelection::All,
        false,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("list query");
    let peek = atm_core::read::PeekQuery::new(
        root.to_path_buf(),
        root.to_path_buf(),
        recipient.clone(),
        None,
        team.clone(),
        atm_core::types::ReadSelection::All,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("peek query");
    let read = atm_core::read::ReadQuery::new(
        root.to_path_buf(),
        root.to_path_buf(),
        recipient,
        None,
        team,
        atm_core::types::ReadSelection::All,
        false,
        true,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("read query");
    (list, peek, read)
}

fn fanout_requests(
    router: StorageAndNudgeRouter,
    list: atm_core::list::ListQuery,
    peek: atm_core::read::PeekQuery,
    read: atm_core::read::ReadQuery,
) -> Vec<tokio::task::JoinHandle<Result<atm_core::api::ApiResponse, AtmError>>> {
    (0..12)
        .map(|index| {
            let router = router.clone();
            let request = match index % 4 {
                0 => ApiRequest::Messages(Box::new(atm_core::api::MessageCollectionRequest::List(
                    list.clone(),
                ))),
                1 => ApiRequest::Messages(Box::new(atm_core::api::MessageCollectionRequest::Peek(
                    peek.clone(),
                ))),
                2 => ApiRequest::Messages(Box::new(
                    atm_core::api::MessageCollectionRequest::Receive(read.clone()),
                )),
                _ => ApiRequest::Doctor(DoctorQuery::default()),
            };
            tokio::spawn(async move {
                router
                    .dispatch(
                        request,
                        AuthenticatedIngress::Local,
                        RequestDeadline::unbounded(),
                    )
                    .await
            })
        })
        .collect()
}

pub(super) async fn run_mailbox_and_doctor_fanout() {
    let fixture = fixture(true, None, None);
    fixture
        .router
        .dispatch(
            ApiRequest::new(RequestEnvelope::Write(Box::new(write_request(
                fixture.home_dir.clone(),
                fixture.current_dir.clone(),
            )))),
            AuthenticatedIngress::Local,
            RequestDeadline::unbounded(),
        )
        .await
        .expect("seed mailbox");

    let assembly = open_sqlite_boundary(&fixture.database_path).expect("reopen async boundary");
    let async_mailbox_runtime = assembly
        .async_mailbox_runtime
        .with_state_handoff(HandoffConfig::default())
        .expect("start mailbox handoff");
    let doctor_projection = StorageDoctorProjection::start(
        DoctorProjectionConfig::default(),
        assembly.service_runtime,
        assembly.doctor_ports,
        Arc::new(NullObservability),
    )
    .expect("start doctor projection");
    let router = fixture
        .router
        .clone()
        .with_async_mailbox_runtime(Arc::new(async_mailbox_runtime))
        .with_doctor_projection(Arc::new(BaseOnlyDoctorProjection(doctor_projection)));
    let bridge = router.control_path_sync_bridge.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let release_tx = Arc::new(TestSignal::default());
    let release_rx = Arc::clone(&release_tx);
    let bridge_task = tokio::spawn(async move {
        bridge
            .run(RequestDeadline::unbounded(), move || {
                started_tx.send(()).expect("signal occupied bridge");
                release_rx.wait_for(1);
                Ok(())
            })
            .await
    });
    started_rx.await.expect("bridge started");

    let (list, peek, read) = message_queries(&fixture.home_dir);
    let calls = fanout_requests(router, list, peek, read);
    let (doctor_calls, mailbox_calls): (Vec<_>, Vec<_>) = calls
        .into_iter()
        .enumerate()
        .partition(|(index, _)| index % 4 == 3);
    for (_, call) in mailbox_calls {
        call.await
            .expect("mailbox fanout task joins")
            .expect("mailbox request completes while bridge is occupied");
    }
    release_tx.signal();
    bridge_task
        .await
        .expect("bridge task joins")
        .expect("bridge task completes");
    for (_, call) in doctor_calls {
        let response = call
            .await
            .expect("doctor fanout task joins")
            .expect("doctor request completes after the bridge is released");
        assert!(matches!(response.into_inner(), ResponseEnvelope::Doctor(_)));
    }
}
