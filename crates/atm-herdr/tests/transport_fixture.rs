#![cfg(feature = "test-utils")]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use atm_core::{HerdrAgentName, HerdrSession, RequestDeadline};
use atm_herdr::testing::production_invoker_with_test_binary_and_environment;
use atm_herdr::{HerdrError, HerdrProcessAdapter};

#[path = "support/fake_herdr_socket/mod.rs"]
mod fake_herdr_socket;

#[derive(Debug)]
struct FixtureManifest {
    mode: String,
    delta: Option<String>,
    response: Option<String>,
}

impl FixtureManifest {
    fn parse(bytes: &[u8]) -> Self {
        let value: serde_json::Value = serde_json::from_slice(bytes).expect("manifest JSON");
        Self {
            mode: value
                .get("mode")
                .and_then(serde_json::Value::as_str)
                .expect("manifest mode")
                .to_owned(),
            delta: value
                .get("delta")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            response: value
                .get("response")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        }
    }
}

fn invoker(mode: String, delta: Option<&str>) -> atm_herdr::HerdrProcessInvoker {
    let mut environment = vec![("FAKE_HERDR_MODE".to_owned(), mode)];
    if let Some(delta) = delta {
        environment.push(("FAKE_HERDR_DELTA".to_owned(), delta.to_owned()));
    }
    production_invoker_with_test_binary_and_environment(
        env!("CARGO_BIN_EXE_fake-herdr").into(),
        environment,
    )
}

fn fixture_manifests() -> Vec<(PathBuf, FixtureManifest)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/herdr-versions");
    let mut fixtures = fs::read_dir(root)
        .expect("fixture directories")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|directory| {
            let manifest = directory.join("manifest.json");
            manifest.exists().then(|| {
                let bytes = fs::read(manifest).expect("manifest");
                let manifest = FixtureManifest::parse(&bytes);
                (directory, manifest)
            })
        })
        .collect::<Vec<_>>();
    fixtures.sort_by(|(left, _), (right, _)| left.cmp(right));
    fixtures
}

fn manifest_mode(directory: &Path, manifest: &FixtureManifest) -> String {
    manifest.response.as_ref().map_or_else(
        || manifest.mode.clone(),
        |_| format!("{}:{}", manifest.mode, directory.display()),
    )
}

async fn get(
    invoker: &atm_herdr::HerdrProcessInvoker,
    session: Option<&HerdrSession>,
) -> Result<atm_herdr::HerdrGetOutcome, HerdrError> {
    get_with_deadline(
        invoker,
        session,
        RequestDeadline::after(Duration::from_secs(1)),
    )
    .await
}

async fn get_with_deadline(
    invoker: &atm_herdr::HerdrProcessInvoker,
    session: Option<&HerdrSession>,
    deadline: RequestDeadline,
) -> Result<atm_herdr::HerdrGetOutcome, HerdrError> {
    let agent = HerdrAgentName::new("fixture-agent").expect("agent name");
    invoker
        .get(&agent, session, deadline, atm_herdr::BreakerPolicy::Shared)
        .await
}

#[tokio::test]
async fn production_transport_round_trips_argv_and_two_sessions() {
    let invoker = invoker("echo-argv-and-herdr-session".to_owned(), None);
    for session in ["fixture-one", "fixture-two"] {
        let session = HerdrSession::new(session).expect("session");
        let result = get(&invoker, Some(&session))
            .await
            .expect("fake child response decodes through CliIo");
        let expected_name = format!("fixture-agent:{session}");
        assert_eq!(
            result.snapshot.name.as_deref(),
            Some(expected_name.as_str())
        );
    }
}

#[tokio::test]
async fn every_manifest_drives_its_declared_mode_and_delta() {
    for (directory, manifest) in fixture_manifests() {
        let invoker = invoker(
            manifest_mode(&directory, &manifest),
            manifest.delta.as_deref(),
        );
        let result = get(&invoker, None).await;
        if manifest.delta.is_some() {
            assert_eq!(result, Err(HerdrError::AgentPromptStalled));
        } else {
            assert_eq!(
                result
                    .expect("manifest fixture response")
                    .snapshot
                    .name
                    .as_deref(),
                Some("fake"),
            );
        }
    }
}

#[tokio::test]
async fn production_transport_tolerates_unknown_fields_and_crlf() {
    let result = get(&invoker("stdout-json-line".to_owned(), None), None)
        .await
        .expect("CRLF JSON with unknown fields");
    assert_eq!(result.snapshot.status, atm_herdr::HerdrAgentStatus::Working);
}

#[tokio::test]
async fn replay_runs_twice_through_one_production_invoker() {
    for (directory, manifest) in fixture_manifests() {
        if manifest.response.is_none() {
            continue;
        }
        let invoker = invoker(
            manifest_mode(&directory, &manifest),
            manifest.delta.as_deref(),
        );
        let first = get(&invoker, None).await.expect("first replay");
        let second = get(&invoker, None).await.expect("second replay");
        assert_eq!(first, second);
    }
}

#[cfg(unix)]
fn socket_path(suffix: &str) -> PathBuf {
    static NEXT_SOCKET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NEXT_SOCKET.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "atm-herdr-ay8-fixture-{}-{suffix}-{nonce}.sock",
        std::process::id(),
    ))
}

#[cfg(windows)]
fn socket_path(suffix: &str) -> PathBuf {
    static NEXT_PIPE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NEXT_PIPE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    PathBuf::from(format!(
        r"\\.\pipe\atm-herdr-ay8-fixture-{}-{suffix}-{nonce}",
        std::process::id(),
    ))
}

#[cfg(any(unix, windows))]
async fn socket_get(
    response: Vec<u8>,
    session: Option<&HerdrSession>,
) -> Result<atm_herdr::HerdrGetOutcome, HerdrError> {
    let path = socket_path("get");
    let server = fake_herdr_socket::FakeHerdrSocket::bind(&path).expect("fake socket bind");
    let server_task = tokio::spawn(server.serve_once(response));
    let invoker = atm_herdr::testing::production_invoker_with_test_socket(path.clone());
    let result = get(&invoker, session).await;
    let request = server_task
        .await
        .expect("fake socket task")
        .expect("request");
    let request: serde_json::Value = serde_json::from_slice(&request).expect("request JSON");
    assert_eq!(request["method"], "agent.get");
    #[cfg(unix)]
    let _ = std::fs::remove_file(path);
    result
}

#[cfg(any(unix, windows))]
fn socket_response(directory: &Path, manifest: &FixtureManifest) -> Vec<u8> {
    if manifest.delta.is_some() {
        {
            let mut response =
                br#"{"error":{"code":"agent_prompt_stalled","message":"fixture"}}"#.to_vec();
            response.push(b'\n');
            response
        }
    } else {
        let mut response = fs::read(directory.join("response.json")).expect("fixture response");
        response.push(b'\n');
        response
    }
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn every_manifest_is_equivalent_over_cli_and_socket_transports() {
    for (directory, manifest) in fixture_manifests() {
        let mode = manifest_mode(&directory, &manifest);
        let cli = get(&invoker(mode, manifest.delta.as_deref()), None).await;
        let socket = socket_get(socket_response(&directory, &manifest), None).await;
        assert_eq!(
            socket,
            cli,
            "transport mismatch for {}",
            directory.display()
        );
    }
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn socket_fixture_matrix_covers_late_start_and_one_request_connections() {
    let path = socket_path("late-start");
    let server = fake_herdr_socket::FakeHerdrSocket::bind(&path).expect("fake socket bind");
    let invoker = atm_herdr::testing::production_invoker_with_test_socket(path.clone());
    let call = tokio::spawn(async move { get(&invoker, None).await });
    tokio::task::yield_now().await;
    let mut response = br#"{"result":{"agent":{"name":"late","agent_status":"idle"}}}"#.to_vec();
    response.push(b'\n');
    let request = server
        .serve_once(response)
        .await
        .expect("late-start request");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&request).unwrap()["method"],
        "agent.get"
    );
    assert_eq!(
        call.await
            .expect("call task")
            .expect("late response")
            .snapshot
            .name
            .as_deref(),
        Some("late")
    );

    let second = get(
        &atm_herdr::testing::production_invoker_with_test_socket(path.clone()),
        None,
    )
    .await;
    assert!(matches!(second, Err(HerdrError::ServerUnavailable { .. })));
    #[cfg(unix)]
    let _ = std::fs::remove_file(path);
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn socket_fixture_matrix_covers_no_newline_oversized_and_stalled_read() {
    let absent = socket_path("absent");
    let result = get(
        &atm_herdr::testing::production_invoker_with_test_socket(absent.clone()),
        None,
    )
    .await;
    assert!(matches!(result, Err(HerdrError::ServerUnavailable { .. })));

    let no_newline = socket_path("no-newline");
    let server = fake_herdr_socket::FakeHerdrSocket::bind(&no_newline).expect("bind");
    let task = tokio::spawn(server.serve_once(br#"{}"#.to_vec()));
    let result = get(
        &atm_herdr::testing::production_invoker_with_test_socket(no_newline.clone()),
        None,
    )
    .await;
    task.await.expect("server").expect("request");
    assert!(matches!(result, Err(HerdrError::InternalError { .. })));

    for iteration in 0..50 {
        let oversized = socket_path(&format!("oversized-{iteration}"));
        let server = fake_herdr_socket::FakeHerdrSocket::bind(&oversized).expect("bind");
        let mut response = vec![b'x'; 1024 * 1024 + 1];
        response.push(b'\n');
        let (write_completed_tx, write_completed_rx) = tokio::sync::oneshot::channel();
        let server_task = tokio::spawn(server.serve_oversized(response, write_completed_tx));
        let invoker = atm_herdr::testing::production_invoker_with_test_socket(oversized.clone());
        let client_task = tokio::spawn(async move {
            get_with_deadline(
                &invoker,
                None,
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
        });
        write_completed_rx
            .await
            .expect("server reached the synchronized write boundary");
        let result = client_task.await.expect("client task");
        server_task
            .await
            .expect("server")
            .expect("request despite deliberate client abort");
        assert!(matches!(result, Err(HerdrError::InternalError { .. })));
        #[cfg(unix)]
        let _ = std::fs::remove_file(oversized);
    }

    let stalled = socket_path("stalled-read");
    let server = fake_herdr_socket::FakeHerdrSocket::bind(&stalled).expect("bind");
    let task = tokio::spawn(server.serve_and_stall());
    let result = get(
        &atm_herdr::testing::production_invoker_with_test_socket(stalled.clone()),
        None,
    )
    .await;
    task.abort();
    assert!(matches!(result, Err(HerdrError::Timeout)));
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn socket_fixture_matrix_covers_stalled_write_and_cancellation() {
    let stalled_write = socket_path("stalled-write");
    let server = fake_herdr_socket::FakeHerdrSocket::bind(&stalled_write).expect("bind");
    let task = tokio::spawn(server.stall_before_read());
    let invoker = atm_herdr::testing::production_invoker_with_test_socket(stalled_write);
    let agent = HerdrAgentName::new("fixture-agent").expect("agent name");
    let prompt = "synthetic stalled-write fixture\n".repeat(512 * 1024);
    let result = atm_herdr::testing::socket_prompt(
        &invoker,
        &agent,
        &prompt,
        RequestDeadline::after(Duration::from_millis(100)),
    )
    .await;
    task.abort();
    assert!(matches!(result, Err(HerdrError::Timeout)));

    let cancelled = socket_path("cancelled");
    let server = fake_herdr_socket::FakeHerdrSocket::bind(&cancelled).expect("bind");
    let server_task = tokio::spawn(server.serve_and_stall());
    let invoker = atm_herdr::testing::production_invoker_with_test_socket(cancelled);
    let call = tokio::spawn(async move { get(&invoker, None).await });
    tokio::task::yield_now().await;
    call.abort();
    let _ = call.await;
    server_task.abort();
}

#[cfg(windows)]
#[tokio::test]
async fn windows_named_pipe_fixture_round_trips_in_the_platform_lane() {
    let path = socket_path("windows-round-trip");
    let server = fake_herdr_socket::FakeHerdrSocket::bind(&path).expect("named pipe bind");
    let mut response = br#"{"result":{"agent":{"name":"windows","agent_status":"idle"}}}"#.to_vec();
    response.push(b'\n');
    let task = tokio::spawn(server.serve_once(response));
    let result = get(
        &atm_herdr::testing::production_invoker_with_test_socket(path),
        None,
    )
    .await
    .expect("named pipe response");
    task.await
        .expect("named pipe server")
        .expect("named pipe request");
    assert_eq!(result.snapshot.name.as_deref(), Some("windows"));
}
