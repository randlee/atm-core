use crate::DaemonSubsystem;
use crate::SubsystemObservability;
use crate::lifecycle_control::LifecycleControlSourceAdapter;
use crate::local_ipc_transport::{
    DISPATCH_PANIC_RECOVERED_MESSAGE, LocalIpcServerTransportAdapter, RuntimeServeHooks,
    install_injected_accept_error_for_test,
};
use crate::test_observability::TestDaemonObservability;
#[cfg(windows)]
use crate::test_support::connect_local_ipc_with_timeout;
use crate::test_support::{
    DoctorOnlyDispatcher, LifecycleFlagResetGuard, PanicDispatcher,
    configure_test_local_ipc_timeouts, connect_daemon_local_ipc_until_ready,
};
use atm_core::boundary::RequestDispatcher;
use atm_core::doctor::DoctorQuery;
use atm_core::error::AtmError;
#[cfg(unix)]
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::test_support::EnvGuard;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn local_ipc_depth_env(tempdir: &TempDir, atm_home: &Path) -> EnvGuard {
    EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        (
            "ATM_CONFIG_HOME",
            Some(tempdir.path().to_str().expect("utf8 config home")),
        ),
        ("ATM_LOG_DIR", None),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ])
}

fn local_ipc_depth_observability(atm_home: &Path) -> Arc<TestDaemonObservability> {
    Arc::new(
        TestDaemonObservability::new(atm_core::home::host_log_dir_from_home(atm_home))
            .expect("test observability"),
    )
}

fn local_ipc_depth_server_transport(
    observability: Arc<TestDaemonObservability>,
) -> LocalIpcServerTransportAdapter {
    LocalIpcServerTransportAdapter::new_with_observability(
        SubsystemObservability::new(DaemonSubsystem::LocalIpcTransport, observability.clone()),
        SubsystemObservability::new(DaemonSubsystem::HostOwnership, observability.clone()),
        SubsystemObservability::new(DaemonSubsystem::LifecycleControl, observability),
    )
}

fn send_doctor_request(
    socket_path: &Path,
    ready_rx: mpsc::Receiver<()>,
    home_dir: PathBuf,
    current_dir: PathBuf,
) -> ResponseEnvelope {
    let mut stream = connect_daemon_local_ipc_until_ready(socket_path, ready_rx);
    configure_test_local_ipc_timeouts(&stream);
    let request = RequestEnvelope::Doctor(DoctorQuery {
        home_dir,
        current_dir,
        team_override: None,
    });
    let request_id = atm_core::protocol::next_request_id();
    let frame = atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
    atm_core::protocol::write_frame(&mut stream, &frame, "write doctor frame").expect("write");
    stream.flush().expect("flush");
    let response_frame =
        atm_core::protocol::read_frame(&mut stream, "read response", "response frame too large")
            .expect("read frame")
            .expect("response frame");
    let (response_id, response) =
        atm_core::protocol::response_from_frame_payload(response_frame).expect("decode response");
    assert_eq!(response_id, request_id);
    response
}

#[test]
#[serial_test::serial(env)]
fn local_ipc_accept_error_injection_fails_fast_and_logs_once() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let _env = local_ipc_depth_env(&tempdir, &atm_home);
    let observability = local_ipc_depth_observability(&atm_home);
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = local_ipc_depth_server_transport(observability.clone());
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
        "accept error path should remain bounded",
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
fn local_ipc_post_terminate_rejection_is_bounded() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let _env = local_ipc_depth_env(&tempdir, &atm_home);
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = LocalIpcServerTransportAdapter::new();
    let mut runtime = server_transport
        .prepare_runtime_at_socket_path_for_home(socket_path.clone(), &atm_home)
        .expect("prepare runtime");
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
    let _reset = LifecycleFlagResetGuard::install(lifecycle.clone());
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
                            "shutdown rejection test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Restore the bounded ready-signal handshake before retrying the same-host daemon shutdown rejection test.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    #[cfg(unix)]
    {
        lifecycle.terminate_flag().store(true, Ordering::SeqCst);
        let response = send_doctor_request(
            &socket_path,
            ready_rx,
            tempdir.path().join("home"),
            tempdir.path().join("cwd"),
        );
        match response {
            ResponseEnvelope::Error(error) => {
                assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
                assert!(error.message.contains("shutting down"));
            }
            other => panic!("unexpected shutdown response: {other:?}"),
        }

        serve_result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("recv serve result")
            .expect("serve runtime result");
        join.join().expect("join serve thread");
    }

    #[cfg(windows)]
    {
        ready_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("daemon ready within deadline");
        lifecycle.set_terminate_for_test(true);
        serve_result_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("recv serve result")
            .expect("serve runtime result");
        join.join().expect("join serve thread");

        let local_ipc_name =
            atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path).expect("ipc name");
        assert!(
            connect_local_ipc_with_timeout(local_ipc_name.into_owned(), Duration::from_millis(250))
                .is_err(),
            "same-host runtime should reject new local IPC connections after shutdown",
        );
    }
}

#[test]
#[serial_test::serial(env)]
fn local_ipc_dispatch_panic_during_shutdown_is_bounded_and_logs_once() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let _env = local_ipc_depth_env(&tempdir, &atm_home);
    let observability = local_ipc_depth_observability(&atm_home);
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = local_ipc_depth_server_transport(observability.clone());
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
                            "Restore the bounded ready-signal handshake before retrying the same-host panic recovery test.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let response = send_doctor_request(
        &socket_path,
        ready_rx,
        tempdir.path().join("home"),
        tempdir.path().join("cwd"),
    );
    assert!(matches!(response, ResponseEnvelope::Error(_)));

    let shutdown_started = Instant::now();
    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("recv serve result")
        .expect("serve runtime result");
    assert!(
        shutdown_started.elapsed() <= Duration::from_secs(5),
        "dispatcher panic shutdown should remain bounded",
    );
    observability
        .wait_for_message_contains(DISPATCH_PANIC_RECOVERED_MESSAGE, Duration::from_secs(5))
        .expect("panic-path recovery should be observable in the retained test log");
    join.join().expect("join serve thread");

    #[cfg(unix)]
    assert!(
        !socket_path.exists(),
        "socket endpoint should be removed during shutdown even after a dispatch panic",
    );
}
