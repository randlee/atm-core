use super::*;
use atm_core::boundary::RequestDispatcher;
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::schema::AgentMember;
use atm_core::send::{SendMessageSource, SendOutcome, SendRequest};
use atm_core::test_support::{ROLE_TEAM_LEAD, TEST_QA, TEST_TEAM};
use atm_core::types::{AgentName, ReadSelection, TeamName};
use atm_storage::{
    AddPeerInterfaceCommand, AllowHostCommand, AtmMessageId, PeerInterfaceKey, PeerInterfaceKind,
};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

const ARCH: &str = ROLE_TEAM_LEAD;
const ARCH_TEAM: &str = TEST_TEAM;
const CM5: &str = TEST_QA;
const CM5_TEAM: &str = "test-peer-team";
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

    let arch_dispatcher = start_cross_host_dispatcher(&arch_home, &arch_db, self_ip, arch_port);
    let cm5_dispatcher = start_cross_host_dispatcher(&cm5_home, &cm5_db, self_ip, cm5_port);
    let arch_ctx = CallerContext::new(
        &arch_dispatcher.dispatcher,
        &arch_home,
        &arch_workspace,
        ARCH,
        ARCH_TEAM,
    );
    let cm5_ctx = CallerContext::new(
        &cm5_dispatcher.dispatcher,
        &cm5_home,
        &cm5_workspace,
        CM5,
        CM5_TEAM,
    );
    let remote_host = self_ip.to_string();
    wait_for_peer_listener(
        arch_dispatcher
            .peer_transport
            .bound_addr_for_test()
            .expect("arch bound addr"),
    );
    wait_for_peer_listener(
        cm5_dispatcher
            .peer_transport
            .bound_addr_for_test()
            .expect("cm5 bound addr"),
    );

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

    let received = read_message(
        cm5_ctx.dispatcher,
        cm5_ctx.home,
        cm5_ctx.current_dir,
        cm5_ctx.caller_identity,
        cm5_ctx.caller_team,
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
            .and_then(|metadata| metadata.get("atm"))
            .and_then(serde_json::Value::as_object)
            .and_then(|atm| atm.get("remoteHost"))
            .and_then(serde_json::Value::as_str),
        Some(remote_host.as_str())
    );

    let ack = send_ack(&cm5_ctx, recipient_message_id, "cross-host ack success");
    match ack.reply_disposition {
        atm_core::ack::AckReplyDisposition::Sent {
            reply_message_id: _,
            ..
        } => {}
    }

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

    let second_received = read_message(
        cm5_ctx.dispatcher,
        cm5_ctx.home,
        cm5_ctx.current_dir,
        cm5_ctx.caller_identity,
        cm5_ctx.caller_team,
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

    let ack_error = send_ack_expect_error(
        &cm5_ctx,
        second_recipient_message_id,
        "cross-host ack should fail while source peer listener is down",
    );
    assert!(
        ack_error.is_daemon_unavailable()
            || ack_error.code == atm_core::error_codes::AtmErrorCode::RemoteDeliveryOutcomeUnknown,
        "{ack_error:?}"
    );

    let still_pending = read_message(
        cm5_ctx.dispatcher,
        cm5_ctx.home,
        cm5_ctx.current_dir,
        cm5_ctx.caller_identity,
        cm5_ctx.caller_team,
        ReadSelection::PendingAck,
        Some(second_recipient_message_id),
    );
    let still_pending_message = still_pending.message.expect("still pending message");
    assert_eq!(
        still_pending_message.envelope.message_id,
        Some(second_recipient_message_id)
    );
    assert!(still_pending_message.envelope.pending_ack_at.is_some());
    assert!(still_pending_message.envelope.acknowledged_at.is_none());

    cm5_dispatcher
        .peer_transport
        .shutdown()
        .expect("shutdown receiver peer listener");
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
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            Box::new(request),
        )))
        .expect("send request should succeed")
    {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome,
        other => panic!("unexpected send response: {other:?}"),
    }
}

fn send_ack(
    caller: &CallerContext<'_>,
    message_id: AtmMessageId,
    body: &str,
) -> atm_core::ack::AckOutcome {
    match caller
        .dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            atm_core::ack::AckRequest {
                home_dir: caller.home.to_path_buf(),
                current_dir: caller.current_dir.to_path_buf(),
                caller_identity: caller
                    .caller_identity
                    .parse::<AgentName>()
                    .expect("caller identity"),
                caller_team: caller.caller_team.parse::<TeamName>().expect("caller team"),
                message_id,
                reply_body: body.to_string(),
            },
        )))
        .expect("ack should succeed")
    {
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => outcome,
        other => panic!("unexpected ack response: {other:?}"),
    }
}

fn send_ack_expect_error(
    caller: &CallerContext<'_>,
    message_id: AtmMessageId,
    body: &str,
) -> atm_core::error::AtmError {
    caller
        .dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            atm_core::ack::AckRequest {
                home_dir: caller.home.to_path_buf(),
                current_dir: caller.current_dir.to_path_buf(),
                caller_identity: caller
                    .caller_identity
                    .parse::<AgentName>()
                    .expect("caller identity"),
                caller_team: caller.caller_team.parse::<TeamName>().expect("caller team"),
                message_id,
                reply_body: body.to_string(),
            },
        )))
        .expect_err("ack should fail")
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
