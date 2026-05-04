use super::*;
use crate::client::request_local;
use atm_core::observability::NullObservability;
use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
use serial_test::serial;
use tempfile::TempDir;

// CI should override this via ATM_TEST_TIMEOUT_MS when runners need a larger
// retry budget, but tests must still work hermetically when that env var is
// absent.
const DEFAULT_TEST_TIMEOUT_MS: u64 = 5_000;
const LOOPBACK_PROBE_ATTEMPTS: usize = 8;

struct TestEnvBuilder {
    team: &'static str,
    cwd_name: &'static str,
}

struct TestEnv {
    tempdir: TempDir,
    cwd: std::path::PathBuf,
}

impl TestEnvBuilder {
    fn new() -> Self {
        Self {
            team: TEST_TEAM,
            cwd_name: "workspace",
        }
    }

    fn build(self) -> TestEnv {
        let tempdir = TempDir::new().expect("tempdir");
        let cwd = tempdir.path().join(self.cwd_name);
        fs::create_dir_all(&cwd).expect("workspace dir");
        fs::write(
            cwd.join(".atm.toml"),
            format!("[atm]\ndefault_team = \"{}\"\n", self.team),
        )
        .expect("atm toml");
        TestEnv { tempdir, cwd }
    }
}

#[derive(Default)]
struct FakeDispatcher {
    // Test-only dispatcher state is guarded by a mutex so concurrent request
    // handlers can record ordering without racing the fixture assertions.
    responses: std::sync::Mutex<Vec<DaemonResponse>>,
    requests: std::sync::Mutex<Vec<DaemonRequest>>,
}

impl FakeDispatcher {
    fn queue_response(&self, response: DaemonResponse) {
        self.responses
            .lock()
            .expect("responses lock")
            .push(response);
    }

    fn request_count(&self) -> usize {
        self.requests.lock().expect("requests lock").len()
    }
}

impl RequestDispatcher for FakeDispatcher {
    fn dispatch(&self, request: DaemonRequest) -> Result<DaemonResponse, DispatchError> {
        self.requests.lock().expect("requests lock").push(request);
        self.responses
            .lock()
            .expect("responses lock")
            .pop()
            .ok_or_else(|| DispatchError::Unsupported(RequestKind::Heartbeat))
    }
}

#[test]
fn test_socket_client_uses_dispatcher_contract() {
    let dispatcher = FakeDispatcher::default();
    dispatcher.queue_response(DaemonResponse {
        kind: RequestKind::Doctor,
        payload_json: "{\"summary\":{\"status\":\"healthy\"}}".to_string(),
    });
    let client = TestSocketClient::new(&dispatcher);
    let response = client
        .request(DaemonRequest {
            team_name: TEST_TEAM.parse().expect("team"),
            agent_name: TEST_SENDER.parse().expect("agent"),
            payload: RequestPayload::Doctor(serde_json::json!({"team_override":TEST_TEAM})),
        })
        .expect("response");
    assert_eq!(response.kind, RequestKind::Doctor);
    assert_eq!(dispatcher.request_count(), 1);
}

#[test]
#[serial]
fn second_daemon_startup_fails_deterministically() {
    let tempdir = TempDir::new().expect("tempdir");
    let home_dir = tempdir.path().to_path_buf();
    let worker_threads = Arc::new(Mutex::new(Vec::new()));
    let first = start_runtime(
        DaemonConfig::from_home(home_dir.clone()),
        Arc::clone(&worker_threads),
        Arc::new(CoreDispatcher::new(
            home_dir.clone(),
            Arc::new(NullObservability),
        )),
    )
    .expect("first daemon");
    let error = match start_runtime(
        DaemonConfig::from_home(home_dir),
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(CoreDispatcher::new(
            tempdir.path().to_path_buf(),
            Arc::new(NullObservability),
        )),
    ) {
        Ok(handle) => panic!(
            "second daemon should fail, got endpoint {:?}",
            handle.local_endpoint()
        ),
        Err(error) => error,
    };
    assert_eq!(error.code, AtmErrorCode::DaemonAlreadyRunning);
    first.shutdown().expect("shutdown");
}

#[test]
#[serial]
fn stale_singleton_cleanup_allows_one_live_start_only() {
    let tempdir = TempDir::new().expect("tempdir");
    let paths = DaemonPaths::from_home(tempdir.path());
    fs::create_dir_all(&paths.state_dir).expect("state dir");
    fs::write(&paths.singleton_path, br#"{"pid":999999}"#).expect("stale singleton");
    let handle = start_runtime(
        DaemonConfig::from_home(tempdir.path().to_path_buf()),
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(CoreDispatcher::new(
            tempdir.path().to_path_buf(),
            Arc::new(NullObservability),
        )),
    )
    .expect("daemon with stale singleton");
    let error = match start_runtime(
        DaemonConfig::from_home(tempdir.path().to_path_buf()),
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(CoreDispatcher::new(
            tempdir.path().to_path_buf(),
            Arc::new(NullObservability),
        )),
    ) {
        Ok(handle) => panic!(
            "second daemon should still fail, got endpoint {:?}",
            handle.local_endpoint()
        ),
        Err(error) => error,
    };
    assert_eq!(error.code, AtmErrorCode::DaemonAlreadyRunning);
    handle.shutdown().expect("shutdown");
}

#[test]
#[serial]
fn local_same_host_daemon_api_flow_works() {
    let env = TestEnvBuilder::new().build();
    let tempdir = &env.tempdir;
    let current_dir = env.cwd;
    let worker_threads = Arc::new(Mutex::new(Vec::new()));
    let handle = start_runtime(
        DaemonConfig::from_home(tempdir.path().to_path_buf()),
        Arc::clone(&worker_threads),
        Arc::new(
            CoreDispatcher::new(tempdir.path().to_path_buf(), Arc::new(NullObservability))
                .with_worker_threads(Arc::clone(&worker_threads)),
        ),
    )
    .expect("runtime");
    let response = request_local(
        tempdir.path(),
        &DaemonRequest {
            team_name: TEST_TEAM.parse().expect("team"),
            agent_name: TEST_SENDER.parse().expect("agent"),
            payload: RequestPayload::Heartbeat(serde_json::json!({"pid": 42})),
        },
        SAME_HOST_REQUEST_TIMEOUT,
    )
    .expect("local response");
    assert_eq!(response.kind, RequestKind::Heartbeat);
    let doctor = request_local(
        tempdir.path(),
        &DaemonRequest {
            team_name: TEST_TEAM.parse().expect("team"),
            agent_name: TEST_SENDER.parse().expect("agent"),
            payload: RequestPayload::Doctor(serde_json::json!({
                "home_dir": tempdir.path(),
                "current_dir": current_dir,
                "team_override": TEST_TEAM
            })),
        },
        SAME_HOST_REQUEST_TIMEOUT,
    )
    .expect("doctor response");
    assert_eq!(doctor.kind, RequestKind::Doctor);
    handle.shutdown().expect("shutdown");
}

#[test]
fn bounded_remote_host_unreachable_behavior_is_typed() {
    let request = DaemonRequest {
        team_name: TEST_TEAM.parse().expect("team"),
        agent_name: TEST_SENDER.parse().expect("agent"),
        payload: RequestPayload::Send(serde_json::json!({"message":"hello"})),
    };
    for attempt in 0..LOOPBACK_PROBE_ATTEMPTS {
        let (_listener, address) = closed_loopback_address();
        match request_remote(address, &request, Duration::from_millis(250)) {
            Err(error) => {
                assert_eq!(error.code, AtmErrorCode::DaemonRemoteUnavailable);
                return;
            }
            Ok(response) => {
                assert!(
                    attempt + 1 < LOOPBACK_PROBE_ATTEMPTS,
                    "loopback probe address {address} was reclaimed on every attempt; last response: {:?}",
                    response.kind
                );
            }
        }
    }
    unreachable!("loopback retry guard must return or panic inside the loop");
}

fn closed_loopback_address() -> (TcpListener, std::net::SocketAddr) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("probe listener");
    let address = listener.local_addr().expect("probe addr");
    (listener, address)
}

#[test]
fn remote_acceptance_is_required_for_send_success() {
    let listener = bound_loopback_listener();
    listener
        .set_nonblocking(true)
        .expect("listener nonblocking");
    let address = listener.local_addr().expect("local addr");
    let dispatcher = Arc::new(FakeDispatcher::default());
    dispatcher.queue_response(DaemonResponse {
        kind: RequestKind::Send,
        payload_json: "{\"ok\":true}".to_string(),
    });
    let inflight = Arc::new(AtomicUsize::new(0));
    let worker_threads = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let worker = {
        let inflight = Arc::clone(&inflight);
        let worker_threads = Arc::clone(&worker_threads);
        let dispatcher = dispatcher.clone();
        let stop = Arc::clone(&stop);
        let ready_tx = ready_tx;
        thread::spawn(move || {
            accept_tcp_loop_with_ready(
                listener,
                stop,
                inflight,
                worker_threads,
                dispatcher,
                8,
                Some(ready_tx),
            )
        })
    };
    let request = DaemonRequest {
        team_name: TEST_TEAM.parse().expect("team"),
        agent_name: TEST_SENDER.parse().expect("agent"),
        payload: RequestPayload::Send(serde_json::json!({"message":"hello"})),
    };
    ready_rx
        .recv_timeout(test_timeout_budget(Duration::from_secs(5)))
        .expect("accept loop ready");
    let response =
        request_remote(address, &request, Duration::from_millis(250)).expect("remote response");
    assert_eq!(response.kind, RequestKind::Send);
    stop.store(true, Ordering::SeqCst);
    let shutdown_deadline = std::time::Instant::now() + test_timeout_budget(SHUTDOWN_FORCE_TIMEOUT);
    while !worker.is_finished() && std::time::Instant::now() < shutdown_deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        worker.is_finished(),
        "tcp accept worker did not stop within {:?}",
        test_timeout_budget(SHUTDOWN_FORCE_TIMEOUT)
    );
    if let Err(payload) = worker.join() {
        panic!(
            "tcp accept worker panicked during shutdown: {}",
            crate::shutdown::thread_panic_message(payload)
        );
    }
}

fn test_timeout_budget(default_timeout: Duration) -> Duration {
    std::env::var("ATM_TEST_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|timeout_ms| *timeout_ms > 0)
        .map(Duration::from_millis)
        .unwrap_or(default_timeout)
}

fn bound_loopback_listener() -> TcpListener {
    TcpListener::bind(("127.0.0.1", 0)).expect("probe listener")
}
