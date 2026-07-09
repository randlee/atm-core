use crate::DaemonSubsystem;
use crate::SubsystemObservability;
use crate::lifecycle_control::LifecycleControlSourceAdapter;
use crate::local_ipc_transport::{
    DISPATCH_PANIC_RECOVERED_MESSAGE, LocalIpcServerTransportAdapter, RuntimeServeHooks,
    install_injected_accept_error_for_test,
};
use crate::test_observability::TestDaemonObservability;
use crate::test_support::{
    DoctorOnlyDispatcher, LifecycleFlagResetGuard, PanicDispatcher,
    configure_test_local_ipc_timeouts, connect_daemon_local_ipc_until_ready,
    connect_local_ipc_with_timeout,
};
use atm_core::boundary::RequestDispatcher;
use atm_core::doctor::DoctorQuery;
use atm_core::error::AtmError;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::test_support::EnvGuard;
use std::io::Write;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn windows_test_observability(atm_home: &std::path::Path) -> Arc<TestDaemonObservability> {
    Arc::new(
        TestDaemonObservability::new(atm_core::home::host_log_dir_from_home(atm_home))
            .expect("test observability"),
    )
}

fn windows_test_server_transport(
    observability: Arc<TestDaemonObservability>,
) -> LocalIpcServerTransportAdapter {
    LocalIpcServerTransportAdapter::new_with_observability(
        SubsystemObservability::new(DaemonSubsystem::LocalIpcTransport, observability.clone()),
        SubsystemObservability::new(DaemonSubsystem::HostOwnership, observability.clone()),
        SubsystemObservability::new(DaemonSubsystem::LifecycleControl, observability),
    )
}

#[test]
#[serial_test::serial(env)]
fn windows_local_ipc_runtime_terminate_finishes_within_deadline() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        (
            "ATM_CONFIG_HOME",
            Some(tempdir.path().to_str().expect("utf8 config home")),
        ),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = LocalIpcServerTransportAdapter::new();
    let mut runtime = server_transport
        .prepare_runtime_at_socket_path(socket_path)
        .expect("prepare runtime");
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let (lifecycle, _reset) = {
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let reset = LifecycleFlagResetGuard::install(lifecycle.clone());
        (lifecycle, reset)
    };
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> = Arc::new(DoctorOnlyDispatcher);
    let (serve_result_tx, serve_result_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(0);

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
                    ready_tx.send(()).ok();
                    Ok(())
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let local_ipc_name =
        atm_core::protocol::daemon_local_ipc_name_from_path(&tempdir.path().join("daemon.sock"))
            .expect("ipc name");
    ready_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("daemon ready within deadline");
    lifecycle.set_terminate_for_test(true);

    serve_result_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
    assert!(
        connect_local_ipc_with_timeout(local_ipc_name, Duration::from_millis(250)).is_err(),
        "windows same-host runtime should reject new local IPC connections after shutdown",
    );
}

#[test]
#[serial_test::serial(env)]
fn windows_local_ipc_accept_error_injection_fails_fast_and_logs_once() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        (
            "ATM_CONFIG_HOME",
            Some(tempdir.path().to_str().expect("utf8 config home")),
        ),
        ("ATM_LOG_DIR", None),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let observability = windows_test_observability(&atm_home);
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = windows_test_server_transport(observability.clone());
    let mut runtime = server_transport
        .prepare_runtime_at_socket_path_for_home(socket_path, &atm_home)
        .expect("prepare runtime");
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
    let _reset = LifecycleFlagResetGuard::install(lifecycle);
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> = Arc::new(DoctorOnlyDispatcher);
    let (serve_result_tx, serve_result_rx) = mpsc::channel();
    let (inject_tx, inject_rx) = mpsc::sync_channel(1);
    install_injected_accept_error_for_test(&mut runtime, inject_tx);

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
                publish_ready: || Ok(()),
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    inject_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("accept error should inject within 1s");
    let shutdown_started = Instant::now();
    let error = serve_result_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("serve result should arrive within 1s")
        .expect_err("serve should fail after injected accept error");
    assert!(
        shutdown_started.elapsed() <= Duration::from_secs(1),
        "accept error path should remain bounded on Windows",
    );
    assert!(error.message.contains("accept error") || error.message.contains("accepting"));
    observability
        .wait_for_message_contains(
            "injected daemon local IPC accept error for test",
            Duration::from_secs(5),
        )
        .expect("accept error should be observable in the retained test log");
    join.join().expect("join serve thread");
}

#[test]
#[serial_test::serial(env)]
fn windows_local_ipc_dispatch_panic_during_shutdown_finishes_within_deadline() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        (
            "ATM_CONFIG_HOME",
            Some(tempdir.path().to_str().expect("utf8 config home")),
        ),
        ("ATM_LOG_DIR", None),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let observability = windows_test_observability(&atm_home);
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = windows_test_server_transport(observability.clone());
    let mut runtime = server_transport
        .prepare_runtime_at_socket_path_for_home(socket_path.clone(), &atm_home)
        .expect("prepare runtime");
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
    let _reset = LifecycleFlagResetGuard::install(lifecycle.clone());
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> = Arc::new(PanicDispatcher);
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
                            "panic recovery test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Restore the bounded ready-signal handshake before retrying the Windows same-host panic recovery test.",
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
    });
    let request_id = atm_core::protocol::next_request_id();
    let frame = atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
    atm_core::protocol::write_frame(&mut stream, &frame, "write doctor frame").expect("write");
    stream.flush().expect("flush");
    let response_frame =
        atm_core::protocol::read_frame(&mut stream, "read panic response", "panic frame too large")
            .expect("read frame")
            .expect("response frame");
    let (response_id, response) =
        atm_core::protocol::response_from_frame_payload(response_frame).expect("decode response");
    assert_eq!(response_id, request_id);
    assert!(matches!(response, ResponseEnvelope::Error(_)));

    let shutdown_started = Instant::now();
    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("recv serve result")
        .expect("serve runtime result");
    assert!(
        shutdown_started.elapsed() <= Duration::from_secs(5),
        "dispatcher panic shutdown should remain bounded on Windows",
    );
    observability
        .wait_for_message_contains(DISPATCH_PANIC_RECOVERED_MESSAGE, Duration::from_secs(5))
        .expect("panic-path recovery should be observable in the retained test log");
    join.join().expect("join serve thread");
}
