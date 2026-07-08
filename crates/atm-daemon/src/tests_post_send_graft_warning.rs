use atm_core::ack::AckRequest;
use atm_core::boundary::{ReplaySource, RequestDispatcher, RosterHarness};
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::schema::{AgentMember, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD};
use atm_core::types::TeamName;
use atm_graft::{
    GraftClient, GraftSession, GraftSessionOptions, GraftSessionState, HostNudgeInjector,
};
use atm_runtime_test_support::open_sqlite_boundary;
use tempfile::TempDir;

use crate::LocalIpcServerTransportAdapter;
use crate::lifecycle_control::LifecycleControlSourceAdapter;
use crate::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};
use crate::test_support::LifecycleFlagResetGuard;

const TEST_TEAM: &str = "test-team";

fn replay_source_static(label: &'static str) -> ReplaySource {
    ReplaySource::new(label).unwrap_or_else(|_| unreachable!("static replay source must validate"))
}

fn install_test_roster_with_harness(
    db_path: &std::path::Path,
    members: &[(&str, RosterHarness, Option<&std::path::Path>)],
) {
    let assembly = open_sqlite_boundary(db_path).expect("assemble boundary");
    let roster_store = assembly.roster_store_arc();
    let team = TEST_TEAM.parse::<TeamName>().expect("team");
    let members = members
        .iter()
        .map(|(name, harness, home_dir)| {
            let mut member = AgentMember::with_name((*name).parse().expect("member"));
            if let Some(home_dir) = home_dir {
                member.home_dir = home_dir.to_path_buf().into();
            }
            let mut record = atm_core::boundary::roster_member_record_from_claude_code_member(
                team.clone(),
                member,
            );
            record.harness = *harness;
            record
        })
        .collect::<Vec<_>>();
    roster_store
        .replace_roster(
            &team,
            &members,
            Some(&replay_source_static("daemon-graft-warning-test")),
        )
        .expect("replace roster");
}

fn write_team_config(home_dir: &std::path::Path, members: &[&str]) {
    let team_dir = home_dir.join(".claude").join("teams").join(TEST_TEAM);
    std::fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes dir");
    let config = TeamConfig {
        members: members
            .iter()
            .map(|name| AgentMember::with_name((*name).parse().expect("member")))
            .collect(),
        ..Default::default()
    };
    std::fs::write(
        team_dir.join("config.json"),
        serde_json::to_vec(&config).expect("team config"),
    )
    .expect("write team config");
}

fn graft_warning_dispatcher() -> (
    TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    DaemonRequestDispatcher,
) {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
    install_test_roster_with_harness(
        &db_path,
        &[
            (
                ROLE_TEAM_LEAD,
                RosterHarness::ClaudeCode,
                Some(workspace_dir.as_path()),
            ),
            (
                "qa-a",
                RosterHarness::CodexCli,
                Some(workspace_dir.as_path()),
            ),
        ],
    );
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    (tempdir, atm_home, workspace_dir, dispatcher)
}

fn write_graft_enabled_config(workspace_dir: &std::path::Path) {
    std::fs::write(
        workspace_dir.join(".atm.toml"),
        "[atm.graft]\nenabled = true\n",
    )
    .expect("write graft config");
}

struct RunningDispatcherServer {
    lifecycle: LifecycleControlSourceAdapter,
    _reset: LifecycleFlagResetGuard,
    join: std::thread::JoinHandle<()>,
    result_rx: std::sync::mpsc::Receiver<Result<(), atm_core::error::AtmError>>,
}

impl RunningDispatcherServer {
    fn stop(self) {
        self.lifecycle.set_terminate_for_test(true);
        self.result_rx
            .recv_timeout(std::time::Duration::from_secs(15))
            .expect("recv serve result")
            .expect("serve runtime result");
        self.join.join().expect("join serve thread");
    }
}

fn spawn_dispatcher_server(
    socket_path: std::path::PathBuf,
    dispatcher: DaemonRequestDispatcher,
) -> (RunningDispatcherServer, std::sync::mpsc::Receiver<()>) {
    let server_transport = LocalIpcServerTransportAdapter::new();
    let mut runtime = server_transport
        .prepare_runtime_at_socket_path(socket_path)
        .expect("prepare runtime");
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
    let reset = LifecycleFlagResetGuard::install(lifecycle.clone());
    let dispatcher: std::sync::Arc<dyn RequestDispatcher + Send + Sync> =
        std::sync::Arc::new(dispatcher);
    let (serve_result_tx, serve_result_rx) = std::sync::mpsc::channel();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let join = std::thread::spawn(move || {
        let result = runtime.serve_with_runtime_hooks(
            dispatcher,
            crate::local_ipc_transport::RuntimeServeHooks {
                endpoint_guard,
                graceful_drain_deadline: std::time::Duration::from_millis(500),
                force_cancel_deadline: std::time::Duration::from_secs(2),
                begin_shutdown: || Ok(()),
                reload_runtime_view: || Ok(()),
                finalize_shutdown: || {},
                publish_ready: move || {
                    ready_tx.send(()).map_err(|_| {
                        atm_core::error::AtmError::daemon_unavailable(
                            "graft warning test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Rerun the graft warning test after restoring the bounded ready-signal handshake.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });
    (
        RunningDispatcherServer {
            lifecycle,
            _reset: reset,
            join,
            result_rx: serve_result_rx,
        },
        ready_rx,
    )
}

#[derive(Debug, Default)]
struct RecordingInjector {
    nudges: std::sync::Mutex<Vec<atm_core::boundary::PostSendHookEvent>>,
}

impl HostNudgeInjector for RecordingInjector {
    fn inject_nudge(
        &self,
        nudge: atm_core::boundary::PostSendHookEvent,
    ) -> Result<(), atm_core::error::AtmError> {
        self.nudges.lock().expect("nudges lock").push(nudge);
        Ok(())
    }
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_send_surfaces_typed_warning_when_graft_receiver_path_is_unavailable() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("hello graft".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect("send response");

    let outcome = match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome,
        other => panic!("expected send response, got {other:?}"),
    };
    assert_eq!(outcome.warnings.len(), 1);
    assert_eq!(
        outcome.warnings[0].code,
        Some(AtmErrorCode::PostSendGraftUnavailable)
    );
    assert!(outcome.warnings[0].recovery.is_some());
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_ack_surfaces_typed_warning_when_graft_reply_target_is_unavailable() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();

    let source_response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
                "qa-a".parse().expect("caller"),
                "team-lead@test-team",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("please ack".to_string()),
                None,
                true,
                None,
                false,
            )
            .expect("source send request"),
        )))
        .expect("source send response");
    let source_message_id = match source_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome.message_id,
        other => panic!("expected send response, got {other:?}"),
    };

    let ack_response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            AckRequest {
                home_dir: atm_home,
                current_dir: workspace_dir,
                caller_identity: ROLE_TEAM_LEAD.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                message_id: source_message_id,
                reply_body: "ack reply".to_string(),
            },
        )))
        .expect("ack response");

    let ack_outcome = match ack_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => outcome,
        other => panic!("expected ack response, got {other:?}"),
    };
    assert_eq!(ack_outcome.warnings.len(), 1);
    assert_eq!(
        ack_outcome.warnings[0].code,
        Some(AtmErrorCode::PostSendGraftUnavailable)
    );
    assert!(ack_outcome.warnings[0].recovery.is_some());
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_send_delivers_direct_graft_nudge_without_warning() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();
    write_graft_enabled_config(&workspace_dir);
    let socket_path = atm_home.join(".atm").join("daemon").join("atm-daemon.sock");
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        (
            "ATM_DAEMON_SOCKET",
            Some(socket_path.to_str().expect("utf8 socket path")),
        ),
        ("ATM_TEAM", Some(TEST_TEAM)),
        ("ATM_IDENTITY", Some("qa-a")),
        ("HOME", Some(atm_home.to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let (server, ready_rx) = spawn_dispatcher_server(socket_path, dispatcher);
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("daemon ready");

    let client = GraftClient::connect().expect("connect graft client");
    let injector = std::sync::Arc::new(RecordingInjector::default());
    let session = GraftSession::activate(
        client,
        GraftSessionOptions::for_current_process(
            workspace_dir.clone(),
            TEST_TEAM.parse().expect("team"),
            "qa-a".parse().expect("agent"),
        ),
        injector.clone() as std::sync::Arc<dyn HostNudgeInjector>,
    )
    .expect("activate graft session");
    assert_eq!(
        session.snapshot().expect("snapshot").state,
        GraftSessionState::Listening
    );

    let response = session
        .send(
            SendRequest::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("hello graft".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request"),
        )
        .expect("send response");

    assert!(response.warnings.is_empty());
    let nudges = injector.nudges.lock().expect("nudges lock");
    assert_eq!(nudges.len(), 1);
    assert_eq!(nudges[0].recipient.as_str(), "qa-a");
    assert_eq!(nudges[0].recipient_team.as_str(), TEST_TEAM);
    assert_eq!(nudges[0].message, "hello graft");

    drop(session);
    server.stop();
}
