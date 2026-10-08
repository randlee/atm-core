//! Real process-exit proof for the daemon shutdown SLO.
//!
//! The parent re-executes this test binary as a child that composes the
//! daemon on its own multi-thread runtime, exports to a collector living in
//! the parent process, and stops on request: `shutdown_replacement_daemon`,
//! runtime teardown (which releases abandoned SDK blocking calls), process
//! exit. The parent times the stop request to the child's exit status.
#![cfg(test)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use super::receiver::Receiver;
use super::{Daemon, DaemonObservability, endpoint_env, sent_message_id, task_record};
use atm_core::test_support::FakeEnvSource;

const CHILD_MODE: &str = "ATM_BD6_EXIT_CHILD";
const CHILD_ENDPOINT: &str = "ATM_BD6_EXIT_ENDPOINT";
const CHILD_TEST: &str = "telemetry_lifecycle_tests::exit::exit_proof_child";

/// Child half; a no-op unless launched by [`stop_to_exit`].
#[test]
fn exit_proof_child() {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return;
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("child runtime");
    runtime.block_on(async {
        let env = std::env::var(CHILD_ENDPOINT).map_or_else(
            |_| FakeEnvSource::empty(),
            |endpoint| endpoint_env(&endpoint),
        );
        let daemon = Daemon::start(env).await;
        for task in ["BD6-X1", "BD6-X2", "BD6-X3"] {
            sent_message_id(
                daemon
                    .write(daemon.request("sender", "recipient", task, None))
                    .await
                    .expect("child assignment"),
            );
        }
        if mode == "full" {
            // Saturate the bounded task queue behind the stalled exporter.
            for seq in 1..=4096 {
                daemon.workers.task_telemetry.try_emit(task_record(seq));
            }
            assert!(
                daemon
                    .workers
                    .task_telemetry
                    .diagnostics()
                    .snapshot()
                    .dropped_full
                    > 0,
                "the task queue is full"
            );
        }
        println!("BD6-READY");
        std::io::stdout().flush().expect("flush ready");
        let mut line = String::new();
        tokio::task::spawn_blocking(move || std::io::stdin().read_line(&mut line))
            .await
            .expect("stdin reader")
            .expect("stop request");
        daemon.shutdown().await.expect("child daemon shutdown");
    });
    drop(runtime);
}

const CHILD_SCENARIO: &str = "ATM_BD6_CHILD_SCENARIO";

/// Whether this process is the child launched for `scenario`.
pub(super) fn is_child_scenario(scenario: &str) -> bool {
    std::env::var(CHILD_SCENARIO).is_ok_and(|value| value == scenario)
}

/// Runs the `scenario` child test in its own process, so it owns the
/// process-global tracing bridge, and requires it to pass.
pub(super) fn run_child_scenario(scenario: &str) {
    run_child_scenario_with(scenario, &[]);
}

/// [`run_child_scenario`] with extra child environment.
pub(super) fn run_child_scenario_with(scenario: &str, envs: &[(&str, &str)]) {
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", scenario, "--nocapture", "--test-threads=1"])
        .env(CHILD_SCENARIO, scenario)
        .env_remove(CHILD_MODE)
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn scenario child");
    let (exit_tx, exit_rx) = mpsc::channel();
    let id = child.id();
    std::thread::spawn(move || {
        let _ = exit_tx.send(child.wait());
    });
    let Ok(status) = exit_rx.recv_timeout(Duration::from_secs(90)) else {
        let _ = Command::new("kill").arg(id.to_string()).status();
        panic!("scenario child {scenario} did not exit within 90s");
    };
    let status = status.expect("scenario child status");
    assert!(
        status.success(),
        "scenario child {scenario} failed: {status}"
    );
}

/// Launches the child, waits until it serves, requests the stop and returns
/// the time from the request to the child's successful exit.
fn stop_to_exit(mode: &str, endpoint: Option<&str>) -> Duration {
    let mut command = Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD_MODE, mode)
        .env_remove(CHILD_ENDPOINT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(endpoint) = endpoint {
        command.env(CHILD_ENDPOINT, endpoint);
    }
    let mut child = command.spawn().expect("spawn exit-proof child");
    let stdout = child.stdout.take().expect("child stdout");
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.contains("BD6-READY") {
                let _ = ready_tx.send(());
            }
        }
    });
    if ready_rx.recv_timeout(Duration::from_secs(60)).is_err() {
        let _ = child.kill();
        panic!("exit-proof child never became ready");
    }
    let mut stdin = child.stdin.take().expect("child stdin");
    let started = std::time::Instant::now();
    writeln!(stdin, "stop").expect("request stop");
    drop(stdin);
    let (exit_tx, exit_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = exit_tx.send(child.wait());
    });
    let status = exit_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("child exits")
        .expect("child status");
    let elapsed = started.elapsed();
    assert!(status.success(), "child exit status {status}");
    elapsed
}

/// Positive: with the task queue full and a collector that never answers,
/// the daemon process exits within the 10s force SLO.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_exits_within_ten_seconds_with_full_queue_and_stalled_collector() {
    let stalled = Receiver::start(true).await;
    let endpoint = stalled.endpoint.clone();
    let elapsed = tokio::task::spawn_blocking(move || stop_to_exit("full", Some(&endpoint)))
        .await
        .expect("parent driver");
    assert!(
        elapsed <= Duration::from_secs(10),
        "stop to exit took {elapsed:?}"
    );
    assert!(
        stalled
            .capture
            .started
            .load(std::sync::atomic::Ordering::SeqCst)
            >= 1,
        "the child really exported into the stalled collector"
    );
    stalled.stop().await;
}

/// Positive: with a healthy collector the daemon process exits within the
/// 5s clean-stop SLO, after flushing its export.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_exits_within_five_seconds_when_clean() {
    let healthy = Receiver::start(false).await;
    let endpoint = healthy.endpoint.clone();
    let elapsed = tokio::task::spawn_blocking(move || stop_to_exit("clean", Some(&endpoint)))
        .await
        .expect("parent driver");
    assert!(
        elapsed <= Duration::from_secs(5),
        "stop to exit took {elapsed:?}"
    );
    assert!(
        healthy
            .capture
            .spans
            .lock()
            .unwrap()
            .iter()
            .any(|span| span.name == "atm.task.event"),
        "shutdown flushed the child's task events"
    );
    healthy.stop().await;
}

const FINAL_RECORD_CHILD: &str = "telemetry_lifecycle_tests::exit::final_record_child";
const CHILD_LOG_DIR: &str = "ATM_BD6_LOG_DIR";
/// The bridge retains allowlisted fields only, never message text.
const FINAL_RECORD: &str = "ATM_DAEMON_SHUTDOWN_DRAINED";
const BACKLOG_RECORD: &str = "BD6_RETAINED_BACKLOG";
/// Below the 1024-event logger queue, so every record is admitted, and long
/// enough that the writer is still behind when the process exits undrained.
const BACKLOG: usize = 512;

/// Child half of [`final_lifecycle_record_survives_process_exit`]; a no-op
/// unless launched by it.
#[test]
fn final_record_child() {
    if !is_child_scenario(FINAL_RECORD_CHILD) {
        return;
    }
    let log_dir = std::env::var(CHILD_LOG_DIR).expect("log dir");
    let endpoint = std::env::var(CHILD_ENDPOINT).expect("collector endpoint");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("child runtime");
    runtime.block_on(async {
        let env = FakeEnvSource::new([
            ("ATM_OTEL_ENDPOINT", Some(endpoint.as_str())),
            ("ATM_LOG_DESTINATION", Some("both")),
        ]);
        // Production order: bootstrap, install the global bridge, compose.
        let observability = DaemonObservability::bootstrap_from(&env, log_dir.into())
            .await
            .expect("daemon observability");
        observability
            .install_tracing_bridge()
            .expect("the child process owns the global tracing bridge");
        let root = tempfile::tempdir().expect("daemon root");
        let daemon = Daemon::compose(root, observability).await;
        for seq in 0..BACKLOG {
            tracing::info!(target: "atm_daemon_bootstrap::lifecycle", code = BACKLOG_RECORD, attempt = seq, "backlog");
        }
        daemon.shutdown().await.expect("child daemon shutdown");
    });
    // Process exit follows; nothing in the child flushes the logger itself.
    drop(runtime);
}

/// Positive: after `shutdown_replacement_daemon` and a real process exit, the
/// final lifecycle record is the last line on disk behind a full backlog, and
/// the collector received it before the logger provider stopped. Omitting the
/// retained-logger drain loses it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn final_lifecycle_record_survives_process_exit() {
    let healthy = Receiver::start(false).await;
    // The logger root is the log directory's parent, as for the host `logs`.
    let root = tempfile::tempdir().expect("log root");
    let logs = root.path().join("logs");
    let log_dir = logs.to_str().expect("utf-8 log dir").to_owned();
    let endpoint = healthy.endpoint.clone();
    tokio::task::spawn_blocking(move || {
        run_child_scenario_with(
            FINAL_RECORD_CHILD,
            &[(CHILD_LOG_DIR, &log_dir), (CHILD_ENDPOINT, &endpoint)],
        );
    })
    .await
    .expect("parent driver");
    let jsonl = std::fs::read_to_string(logs.join(atm_observability::CANONICAL_LOG_FILE_NAME))
        .expect("retained log file");
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains(BACKLOG_RECORD))
            .count(),
        BACKLOG,
        "every admitted backlog record is on disk; last lines: {:?}",
        lines.iter().rev().take(3).collect::<Vec<_>>()
    );
    assert!(
        lines.last().is_some_and(|line| line.contains(FINAL_RECORD)),
        "the final lifecycle record is the last line on disk: {:?}",
        lines.last()
    );
    assert!(
        healthy
            .capture
            .logs
            .lock()
            .unwrap()
            .iter()
            .any(|record| format!("{record:?}").contains(FINAL_RECORD)),
        "the collector received the final lifecycle record"
    );
    healthy.stop().await;
}
