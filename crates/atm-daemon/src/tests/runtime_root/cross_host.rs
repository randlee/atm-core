use super::*;
use atm_core::boundary::RequestDispatcher;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::schema::AgentMember;
use atm_core::send::{SendMessageSource, SendOutcome, SendRequest};
use atm_core::test_support::{ROLE_TEAM_LEAD, TEST_QA, TEST_TEAM};
use atm_core::types::{AgentName, ReadSelection, TeamName};
use atm_storage::{
    AddPeerInterfaceCommand, AllowHostCommand, AtmMessageId, PeerInterfaceKey, PeerInterfaceKind,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const ARCH: &str = ROLE_TEAM_LEAD;
const ARCH_TEAM: &str = TEST_TEAM;
const CM5: &str = TEST_QA;
const CM5_TEAM: &str = "test-peer-team";
const CROSS_HOST_CHILD_ENV: &str = "ATM_CROSS_HOST_CHILD_STATE";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrossHostChildState {
    home_dir: PathBuf,
    workspace_dir: PathBuf,
    db_path: PathBuf,
    local_ipc_socket_path: PathBuf,
    ready_file: PathBuf,
    stop_file: PathBuf,
    bind_ip: Ipv4Addr,
    bind_port: u16,
}

#[test]
#[serial_test::serial(env)]
fn cross_host_send_and_ack_round_trip_and_failed_ack_stays_pending() {
    install_retained_runtime_factory();
    let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
    let _reset = LifecycleFlagResetGuard::install(lifecycle);
    let self_ip = discover_non_loopback_ipv4_for_test();
    let arch_port = reserve_free_port(self_ip);
    let cm5_port = reserve_free_port(self_ip);
    assert_ne!(arch_port, cm5_port, "listener ports must differ");

    let tempdir = TempDir::new().expect("tempdir");
    let arch_home = tempdir.path().join("arch-home");
    let arch_workspace = tempdir.path().join("arch-workspace");
    let arch_db = tempdir.path().join("arch-mail.db");
    std::fs::create_dir_all(&arch_home).expect("arch home");
    std::fs::create_dir_all(&arch_workspace).expect("arch workspace");
    write_workspace_config(&arch_workspace);
    write_team_config_for(&arch_home, ARCH_TEAM, &[ARCH]);
    add_member_via_retained_admin(&arch_db, &arch_home, ARCH_TEAM, ARCH, &arch_workspace);
    configure_cross_host_sqlite(&arch_db, self_ip, cm5_port, &[self_ip]);

    let cm5_home = tempdir.path().join("cm5-home");
    let cm5_workspace = tempdir.path().join("cm5-workspace");
    let cm5_db = tempdir.path().join("cm5-mail.db");
    std::fs::create_dir_all(&cm5_home).expect("cm5 home");
    std::fs::create_dir_all(&cm5_workspace).expect("cm5 workspace");
    write_workspace_config(&cm5_workspace);
    write_team_config_for(&cm5_home, CM5_TEAM, &[CM5]);
    add_member_via_retained_admin(&cm5_db, &cm5_home, CM5_TEAM, CM5, &cm5_workspace);
    configure_cross_host_sqlite(&cm5_db, self_ip, arch_port, &[self_ip]);

    let child_state = CrossHostChildState {
        home_dir: cm5_home.clone(),
        workspace_dir: cm5_workspace.clone(),
        db_path: cm5_db.clone(),
        local_ipc_socket_path: tempdir.path().join("cm5-daemon.sock"),
        ready_file: tempdir.path().join("cm5-ready"),
        stop_file: tempdir.path().join("cm5-stop"),
        bind_ip: self_ip,
        bind_port: cm5_port,
    };

    let mut cm5_process = spawn_cross_host_child(&child_state);

    let arch_dispatcher = start_cross_host_dispatcher(&arch_home, &arch_db, self_ip, arch_port);
    let arch_ctx = CallerContext::new(
        &arch_dispatcher.dispatcher,
        &arch_home,
        &arch_workspace,
        ARCH,
        ARCH_TEAM,
    );
    let remote_host = self_ip.to_string();
    wait_for_peer_listener(
        arch_dispatcher
            .peer_transport
            .bound_addr_for_test()
            .expect("arch bound addr"),
    );
    wait_for_child_ready(&child_state);
    wait_for_peer_listener(SocketAddr::from((
        child_state.bind_ip,
        child_state.bind_port,
    )));

    let first_send = send_compose(
        &arch_ctx,
        SendSpec::new(
            &format!("{CM5}@{CM5_TEAM}"),
            Some(remote_host.as_str()),
            "cross-host send success",
            true,
        ),
    );
    assert_eq!(first_send.outcome.as_str(), "sent");
    let source_message_id = first_send.message_id;

    let received = read_message_over_local_ipc(
        &child_state.local_ipc_socket_path,
        &cm5_home,
        &cm5_workspace,
        CM5,
        CM5_TEAM,
        ReadSelection::PendingAck,
        None,
    );
    let received_message = received.message.expect("received cross-host message");
    let recipient_message_id = received_message
        .envelope
        .message_id
        .expect("recipient message id");
    assert_eq!(received_message.envelope.text, "cross-host send success");
    assert_eq!(
        received_message
            .envelope
            .extra
            .get("metadata")
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata: &serde_json::Map<String, serde_json::Value>| metadata.get("atm"))
            .and_then(serde_json::Value::as_object)
            .and_then(|atm: &serde_json::Map<String, serde_json::Value>| atm.get("remoteHost"))
            .and_then(serde_json::Value::as_str),
        Some(remote_host.as_str())
    );

    let ack = send_ack_over_local_ipc(
        &child_state.local_ipc_socket_path,
        &cm5_home,
        &cm5_workspace,
        CM5,
        CM5_TEAM,
        recipient_message_id,
        "cross-host ack success",
    );
    assert!(!ack.reply_message_id.to_string().is_empty());

    let ack_reply_message =
        wait_for_ack_reply(&arch_ctx, source_message_id, "cross-host ack success");
    assert_eq!(ack_reply_message.text, "cross-host ack success");
    assert_eq!(
        ack_reply_message.acknowledges_message_id,
        Some(source_message_id)
    );
    assert_eq!(
        ack_reply_message
            .extra
            .get("metadata")
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata: &serde_json::Map<String, serde_json::Value>| metadata.get("atm"))
            .and_then(serde_json::Value::as_object)
            .and_then(|atm: &serde_json::Map<String, serde_json::Value>| atm.get("remoteHost"))
            .and_then(serde_json::Value::as_str),
        Some(remote_host.as_str())
    );

    let second_send = send_compose(
        &arch_ctx,
        SendSpec::new(
            &format!("{CM5}@{CM5_TEAM}"),
            Some(remote_host.as_str()),
            "cross-host failed ack stays pending",
            true,
        ),
    );
    assert_eq!(second_send.outcome.as_str(), "sent");

    let second_received = read_message_over_local_ipc(
        &child_state.local_ipc_socket_path,
        &cm5_home,
        &cm5_workspace,
        CM5,
        CM5_TEAM,
        ReadSelection::PendingAck,
        None,
    );
    let second_message = second_received.message.expect("second cross-host message");
    let second_recipient_message_id = second_message
        .envelope
        .message_id
        .expect("second recipient message id");
    assert_eq!(
        second_message.envelope.text,
        "cross-host failed ack stays pending"
    );

    arch_dispatcher
        .peer_transport
        .shutdown()
        .expect("shutdown source peer listener");

    let ack_error = dispatch_ack_over_local_ipc(
        &child_state.local_ipc_socket_path,
        &cm5_home,
        &cm5_workspace,
        CM5,
        CM5_TEAM,
        second_recipient_message_id,
        "cross-host ack should fail while source peer listener is down",
    )
    .expect_err("remote ack should fail closed while source peer listener is down");
    assert_eq!(
        ack_error.code,
        atm_core::error_codes::AtmErrorCode::DaemonUnavailable
    );

    let still_pending = read_message_over_local_ipc(
        &child_state.local_ipc_socket_path,
        &cm5_home,
        &cm5_workspace,
        CM5,
        CM5_TEAM,
        ReadSelection::All,
        Some(second_recipient_message_id),
    );
    let still_pending_message = still_pending
        .message
        .expect("acknowledged message after deferred remote ack");
    assert_eq!(
        still_pending_message.envelope.message_id,
        Some(second_recipient_message_id)
    );
    assert!(still_pending_message.envelope.pending_ack_at.is_some());
    assert!(still_pending_message.envelope.acknowledged_at.is_none());

    stop_cross_host_child(&child_state, &mut cm5_process);
}

#[test]
#[ignore = "helper invoked by cross-host parent test only"]
#[serial_test::serial(env)]
fn cross_host_child_process_runtime() {
    let Some(raw_state) = std::env::var_os(CROSS_HOST_CHILD_ENV) else {
        return;
    };
    let state: CrossHostChildState =
        serde_json::from_str(&raw_state.to_string_lossy()).expect("decode child state");
    run_cross_host_child_process(state);
}

struct CrossHostHarness {
    dispatcher: Arc<DaemonRequestDispatcher>,
    peer_transport: crate::PeerTransportRuntime,
}

struct CallerContext<'a> {
    dispatcher: &'a Arc<DaemonRequestDispatcher>,
    home: &'a std::path::Path,
    current_dir: &'a std::path::Path,
    caller_identity: &'a str,
    caller_team: &'a str,
}

impl<'a> CallerContext<'a> {
    fn new(
        dispatcher: &'a Arc<DaemonRequestDispatcher>,
        home: &'a std::path::Path,
        current_dir: &'a std::path::Path,
        caller_identity: &'a str,
        caller_team: &'a str,
    ) -> Self {
        Self {
            dispatcher,
            home,
            current_dir,
            caller_identity,
            caller_team,
        }
    }
}

struct SendSpec<'a> {
    to: &'a str,
    remote_host: Option<&'a str>,
    body: &'a str,
    requires_ack: bool,
}

fn spawn_cross_host_child(state: &CrossHostChildState) -> Child {
    let current_exe = std::env::current_exe().expect("current exe");
    let state_json = serde_json::to_string(state).expect("encode child state");
    Command::new(current_exe)
        .arg("--ignored")
        .arg("cross_host_child_process_runtime")
        .arg("--nocapture")
        .env(CROSS_HOST_CHILD_ENV, state_json)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn cross-host child test process")
}

fn wait_for_child_ready(state: &CrossHostChildState) {
    let started = std::time::Instant::now();
    loop {
        if state.ready_file.exists() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "cross-host child did not publish readiness within the bounded window"
        );
        std::thread::park_timeout(Duration::from_millis(25));
    }
}

fn stop_cross_host_child(state: &CrossHostChildState, child: &mut Child) {
    fs::write(&state.stop_file, b"stop").expect("write child stop file");
    let status = child.wait().expect("wait for cross-host child");
    assert!(
        status.success(),
        "cross-host child exited unsuccessfully: {status}"
    );
}

fn run_cross_host_child_process(state: CrossHostChildState) {
    install_retained_runtime_factory();
    let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
    let _reset = LifecycleFlagResetGuard::install(lifecycle.clone());
    let _env = EnvGuard::set_many([
        (
            "ATM_HOME",
            Some(state.home_dir.to_str().expect("utf8 child atm home")),
        ),
        (
            "ATM_CONFIG_HOME",
            Some(
                state
                    .home_dir
                    .parent()
                    .expect("child config parent")
                    .to_str()
                    .expect("utf8 child config home"),
            ),
        ),
        (
            SQLITE_RUNTIME_PATH_ENV,
            Some(state.db_path.to_str().expect("utf8 child db path")),
        ),
        (
            "HOME",
            Some(
                state
                    .home_dir
                    .parent()
                    .expect("child home parent")
                    .to_str()
                    .expect("utf8 child home parent"),
            ),
        ),
        ("USERPROFILE", None),
    ]);

    let harness = start_cross_host_dispatcher(
        &state.home_dir,
        &state.db_path,
        state.bind_ip,
        state.bind_port,
    );
    let dispatch_for_runtime: Arc<dyn RequestDispatcher + Send + Sync> = harness.dispatcher.clone();
    let server_transport = LocalIpcServerTransportAdapter::new();
    let runtime = server_transport
        .prepare_runtime_at_socket_path_for_home(
            state.local_ipc_socket_path.clone(),
            &state.home_dir,
        )
        .expect("prepare child local ipc runtime");
    let mut runtime = runtime;
    let endpoint_guard = runtime
        .take_endpoint_guard()
        .expect("take child endpoint guard");
    let stop_file = state.stop_file.clone();
    let ready_file = state.ready_file.clone();
    std::thread::spawn(move || {
        loop {
            if stop_file.exists() {
                lifecycle.set_terminate_for_test(true);
                break;
            }
            std::thread::park_timeout(Duration::from_millis(25));
        }
    });
    runtime
        .serve_with_runtime_hooks(
            dispatch_for_runtime,
            RuntimeServeHooks {
                endpoint_guard,
                graceful_drain_deadline: Duration::from_millis(500),
                force_cancel_deadline: Duration::from_secs(2),
                begin_shutdown: || Ok(()),
                reload_runtime_view: || Ok(()),
                finalize_shutdown: || {},
                publish_ready: move || {
                    fs::write(&ready_file, b"ready").map_err(|source| {
                        AtmError::daemon_unavailable(
                            "cross-host child failed to publish the ready signal file",
                        )
                        .with_source(source)
                    })
                },
            },
        )
        .expect("serve child local ipc runtime");
    harness
        .peer_transport
        .shutdown()
        .expect("shutdown child peer listener");
}

impl<'a> SendSpec<'a> {
    fn new(to: &'a str, remote_host: Option<&'a str>, body: &'a str, requires_ack: bool) -> Self {
        Self {
            to,
            remote_host,
            body,
            requires_ack,
        }
    }
}

fn reserve_free_port(bind_ip: Ipv4Addr) -> u16 {
    TcpListener::bind(SocketAddr::from((bind_ip, 0)))
        .expect("reserve free port")
        .local_addr()
        .expect("reserved local addr")
        .port()
}

fn wait_for_peer_listener(endpoint: SocketAddr) {
    let started = std::time::Instant::now();
    loop {
        if TcpStream::connect_timeout(&endpoint, Duration::from_millis(50)).is_ok() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "peer listener at {endpoint} did not become reachable within the bounded window"
        );
        std::thread::park_timeout(Duration::from_millis(25));
    }
}

fn wait_for_ack_reply(
    caller: &CallerContext<'_>,
    acknowledges_message_id: AtmMessageId,
    expected_text: &str,
) -> atm_core::schema::InboxMessage {
    let started = std::time::Instant::now();
    loop {
        let outcome = read_message(
            caller.dispatcher,
            caller.home,
            caller.current_dir,
            caller.caller_identity,
            caller.caller_team,
            ReadSelection::All,
            None,
        );
        if let Some(message) = outcome.message
            && message.envelope.acknowledges_message_id == Some(acknowledges_message_id)
            && message.envelope.text == expected_text
        {
            return message.envelope;
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "ack reply for {acknowledges_message_id} did not appear within the bounded window"
        );
        std::thread::park_timeout(Duration::from_millis(25));
    }
}

fn start_cross_host_dispatcher(
    atm_home: &std::path::Path,
    db_path: &std::path::Path,
    bind_ip: Ipv4Addr,
    bind_port: u16,
) -> CrossHostHarness {
    let assembly = open_sqlite_boundary(db_path).expect("sqlite boundary");
    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_with_observability(
        Some(assembly.remote_replay_store.clone()),
        Some(assembly.allowed_host_store_arc()),
        Some(assembly.peer_security_store_arc()),
        crate::peer_transport::PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(30),
            peer_listen_addr: Some(SocketAddr::from((bind_ip, bind_port))),
        },
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.to_path_buf(),
        status_cache,
        db_path.to_path_buf(),
        peer_transport.clone(),
    ));
    let listen_addr = SocketAddr::from((bind_ip, bind_port));
    let outcomes = peer_transport
        .reload_listeners(vec![listen_addr], dispatcher.clone())
        .expect("start peer listener");
    assert_eq!(outcomes.len(), 1, "expected one listener outcome");
    assert!(
        outcomes[0].error_message.is_none(),
        "peer listener failed to bind {}: {:?}",
        listen_addr,
        outcomes[0].error_message
    );
    assert_eq!(outcomes[0].bound_addr, Some(listen_addr));
    CrossHostHarness {
        dispatcher,
        peer_transport,
    }
}

fn configure_cross_host_sqlite(
    db_path: &std::path::Path,
    bind_ip: Ipv4Addr,
    remote_port: u16,
    allowed_hosts: &[Ipv4Addr],
) {
    let assembly = open_sqlite_boundary(db_path).expect("sqlite boundary");
    let configured_by = format!("{}@{}", ROLE_TEAM_LEAD, TEST_TEAM);
    let interface_name = format!("if-{bind_ip}-{remote_port}");
    assembly
        .peer_interface_config_store
        .add_interface(
            AddPeerInterfaceCommand::new(
                interface_name.clone(),
                bind_ip.into(),
                bind_ip.into(),
                remote_port,
                PeerInterfaceKind::Loopback,
                configured_by.clone(),
            )
            .expect("add interface"),
        )
        .expect("store interface");
    assembly
        .peer_interface_config_store
        .set_interface_enabled(
            &PeerInterfaceKey::new(interface_name, bind_ip.into(), remote_port)
                .expect("peer interface key"),
            true,
        )
        .expect("enable interface");
    for host in allowed_hosts {
        assembly
            .allowed_host_store_arc()
            .allow_host(
                AllowHostCommand::new(
                    host.to_string(),
                    configured_by.clone(),
                    Some("cross-host test allow".to_string()),
                )
                .expect("allow host command"),
            )
            .expect("allow host");
    }
}

fn write_team_config_for(home_dir: &std::path::Path, team: &str, members: &[&str]) {
    let team_dir = home_dir.join(".claude").join("teams").join(team);
    std::fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes dir");
    let config = atm_core::schema::TeamConfig {
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

fn send_compose(caller: &CallerContext<'_>, spec: SendSpec<'_>) -> SendOutcome {
    let mut request = SendRequest::new(
        caller.home.to_path_buf(),
        caller.current_dir.to_path_buf(),
        caller
            .caller_identity
            .parse::<AgentName>()
            .expect("caller identity"),
        spec.to,
        caller.caller_team.parse::<TeamName>().expect("caller team"),
        SendMessageSource::Inline(spec.body.to_string()),
        None,
        spec.requires_ack,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = atm_core::send::parse_send_target(spec.to, spec.remote_host)
        .expect("parse send target")
        .remote_host;
    match caller
        .dispatcher
        .dispatch(RequestEnvelope::Send(Box::new(request)))
        .expect("send request should succeed")
    {
        ResponseEnvelope::Send(outcome) => outcome,
        other => panic!("unexpected send response: {other:?}"),
    }
}

fn read_message_over_local_ipc(
    socket_path: &std::path::Path,
    home: &std::path::Path,
    current_dir: &std::path::Path,
    caller_identity: &str,
    caller_team: &str,
    selection: ReadSelection,
    message_id: Option<AtmMessageId>,
) -> ReadOutcome {
    let message_id_filter = message_id.map(|id| id.to_string());
    let request = RequestEnvelope::Receive(
        ReadQuery::new(
            home.to_path_buf(),
            current_dir.to_path_buf(),
            caller_identity
                .parse::<AgentName>()
                .expect("caller identity"),
            None,
            caller_team.parse::<TeamName>().expect("caller team"),
            selection,
            false,
            false,
            message_id_filter.as_deref(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query"),
    );
    match dispatch_over_local_ipc(socket_path, request).expect("read over local ipc") {
        ResponseEnvelope::Receive(outcome) => *outcome,
        other => panic!("unexpected local ipc read response: {other:?}"),
    }
}

fn send_ack_over_local_ipc(
    socket_path: &std::path::Path,
    home: &std::path::Path,
    current_dir: &std::path::Path,
    caller_identity: &str,
    caller_team: &str,
    message_id: AtmMessageId,
    body: &str,
) -> atm_core::ack::AckOutcome {
    match dispatch_ack_over_local_ipc(
        socket_path,
        home,
        current_dir,
        caller_identity,
        caller_team,
        message_id,
        body,
    )
    .expect("ack over local ipc")
    {
        ResponseEnvelope::Ack(outcome) => outcome,
        other => panic!("unexpected local ipc ack response: {other:?}"),
    }
}

fn dispatch_ack_over_local_ipc(
    socket_path: &std::path::Path,
    home: &std::path::Path,
    current_dir: &std::path::Path,
    caller_identity: &str,
    caller_team: &str,
    message_id: AtmMessageId,
    body: &str,
) -> Result<ResponseEnvelope, atm_core::error::AtmError> {
    let request = canonical_ack_request(
        home,
        current_dir,
        caller_identity,
        caller_team,
        message_id,
        body,
    );
    dispatch_over_local_ipc(socket_path, request)
}

fn dispatch_over_local_ipc(
    socket_path: &std::path::Path,
    request: RequestEnvelope,
) -> Result<ResponseEnvelope, atm_core::error::AtmError> {
    let ipc_name = atm_core::protocol::daemon_local_ipc_name_from_path(socket_path)
        .expect("ipc name")
        .into_owned();
    let frame =
        atm_core::protocol::request_to_frame_payload(next_request_id(), request).expect("frame");
    let mut stream =
        crate::test_support::connect_local_ipc_with_timeout(ipc_name, Duration::from_secs(3))
            .map_err(|source| {
                AtmError::daemon_unavailable("connect local ipc for cross-host child")
                    .with_source(source)
            })?;
    configure_test_local_ipc_timeouts(&stream);
    atm_core::protocol::write_frame(&mut stream, &frame, "write local ipc frame")?;
    stream.flush().map_err(|source| {
        AtmError::daemon_unavailable("flush local ipc frame").with_source(source)
    })?;
    let response_frame = atm_core::protocol::read_frame(
        &mut stream,
        "read local ipc frame",
        "local ipc response frame too large",
    )?
    .expect("local ipc response frame");
    let (_, response) = atm_core::protocol::response_from_frame_payload(response_frame)?;
    match response {
        ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
        other => Ok(other),
    }
}

fn read_message(
    dispatcher: &Arc<DaemonRequestDispatcher>,
    home: &std::path::Path,
    current_dir: &std::path::Path,
    caller_identity: &str,
    caller_team: &str,
    selection: ReadSelection,
    message_id: Option<AtmMessageId>,
) -> ReadOutcome {
    let message_id_filter = message_id.map(|id| id.to_string());
    let query = ReadQuery::new(
        home.to_path_buf(),
        current_dir.to_path_buf(),
        caller_identity
            .parse::<AgentName>()
            .expect("caller identity"),
        None,
        caller_team.parse::<TeamName>().expect("caller team"),
        selection,
        false,
        false,
        message_id_filter.as_deref(),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("read query");
    match dispatcher
        .dispatch(RequestEnvelope::Receive(query))
        .expect("read should succeed")
    {
        ResponseEnvelope::Receive(outcome) => *outcome,
        other => panic!("unexpected read response: {other:?}"),
    }
}
