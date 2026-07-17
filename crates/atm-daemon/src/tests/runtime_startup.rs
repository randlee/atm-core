use super::*;

#[test]
#[serial_test::serial(env)]
fn local_ipc_runtime_round_trips_doctor_requests_on_shared_transport() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        (
            "ATM_CONFIG_HOME",
            Some(tempdir.path().to_str().expect("utf8 config home")),
        ),
        (
            SQLITE_RUNTIME_PATH_ENV,
            Some(db_path.to_str().expect("utf8 sqlite db path")),
        ),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = LocalIpcServerTransportAdapter::new();
    let runtime = server_transport
        .prepare_runtime_at_socket_path_for_home(socket_path.clone(), &atm_home)
        .expect("prepare runtime");
    let mut runtime = runtime;
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let (lifecycle, _reset) = {
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let reset = LifecycleFlagResetGuard::install(lifecycle.clone());
        (lifecycle, reset)
    };
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> = Arc::new(DoctorOnlyDispatcher);
    let (serve_result_tx, serve_result_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);

    let join = std::thread::spawn(move || {
        let result = runtime.serve_with_runtime_hooks(
            dispatcher,
            RuntimeServeHooks {
                endpoint_guard,
                graceful_drain_deadline: Duration::from_millis(500),
                force_cancel_deadline: Duration::from_secs(2),
                begin_shutdown: || Ok(()),
                reload_runtime_view: || Ok(()),
                finalize_shutdown: || {},
                publish_ready: move || {
                    ready_tx.send(()).map_err(|_| {
                        AtmError::daemon_unavailable(
                            "doctor round-trip test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Rerun the same-host daemon test after restoring the bounded ready-signal handshake.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    configure_test_local_ipc_timeouts(&stream);
    let request = RequestEnvelope::Doctor(DoctorQuery {
        home_dir: tempdir.path().join("home"),
        current_dir: tempdir.path().join("cwd"),
        team_override: None,
        ..DoctorQuery::default()
    });
    let request_id = atm_core::protocol::next_request_id();
    let frame = atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
    atm_core::protocol::write_frame(&mut stream, &frame, "write doctor frame").expect("write");
    stream.flush().expect("flush");
    let response_frame =
        atm_core::protocol::read_frame(&mut stream, "read doctor frame", "doctor frame too large")
            .expect("read frame")
            .expect("response frame");
    let (response_id, response) =
        atm_core::protocol::response_from_frame_payload(response_frame).expect("decode response");
    assert_eq!(response_id, request_id);
    match response {
        ResponseEnvelope::Doctor(report) => {
            assert_eq!(report.summary.status, DoctorStatus::Healthy);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}

#[test]
#[serial_test::serial(env)]
fn compose_runtime_start_writes_retained_log_and_reports_healthy_observability() {
    install_retained_runtime_factory();
    let _drain_guard = ShutdownFinalizerDrainGuard;
    let tempdir = TempDir::new().expect("tempdir");
    let _cwd_guard = CwdGuard::install();
    std::env::set_current_dir(tempdir.path()).expect("set isolated cwd");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        (
            "ATM_CONFIG_HOME",
            Some(tempdir.path().to_str().expect("utf8 config home")),
        ),
        (
            SQLITE_RUNTIME_PATH_ENV,
            Some(db_path.to_str().expect("utf8 sqlite db path")),
        ),
        ("ATM_LOG_DIR", None),
        ("ATM_DAEMON_SOCKET", None),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
    let _reset = LifecycleFlagResetGuard::install(lifecycle.clone());
    let observability = std::sync::Arc::new(
        crate::test_observability::TestDaemonObservability::new(
            atm_core::home::host_log_dir_from_home(&atm_home),
        )
        .expect("test observability"),
    );
    let socket_path = tempdir.path().join("daemon.sock");
    let runtime =
        crate::composition::compose_runtime(observability.clone()).expect("compose runtime");
    let (result_tx, result_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let runtime_socket_path = socket_path.clone();

    let join = std::thread::spawn(move || {
        let result = runtime.start_with_socket_path_for_test(runtime_socket_path, Some(ready_tx));
        result_tx.send(result).expect("send runtime result");
    });

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    configure_test_local_ipc_timeouts(&stream);
    let request = RequestEnvelope::Doctor(DoctorQuery {
        home_dir: atm_home.clone(),
        current_dir: atm_home.clone(),
        team_override: None,
        ..DoctorQuery::default()
    });
    let request_id = atm_core::protocol::next_request_id();
    let frame = atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
    atm_core::protocol::write_frame(&mut stream, &frame, "write doctor frame").expect("write");
    stream.flush().expect("flush");
    let response_frame =
        atm_core::protocol::read_frame(&mut stream, "read doctor frame", "doctor frame too large")
            .expect("read frame")
            .expect("response frame");
    let (response_id, response) =
        atm_core::protocol::response_from_frame_payload(response_frame).expect("decode response");
    assert_eq!(response_id, request_id);
    match response {
        ResponseEnvelope::Doctor(report) => {
            assert_eq!(
                report.observability.logging_state,
                AtmObservabilityHealthState::Healthy
            );
        }
        other => panic!("expected doctor response, got {other:?}"),
    }

    observability
        .wait_for_message_contains("daemon start requested", Duration::from_secs(10))
        .expect("startup event should be recorded without busy-spin polling");

    lifecycle.set_terminate_for_test(true);
    result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv runtime result")
        .expect("runtime result");
    join.join().expect("join runtime thread");
    DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();
}
