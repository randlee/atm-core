//! Real process-exit proof for the daemon shutdown SLO.
//!
//! The parent re-executes this test binary as a child that composes the
//! daemon on its own multi-thread runtime, exports to a collector living in
//! the parent process, and stops on request: `shutdown_replacement_daemon`,
//! runtime teardown (which releases abandoned SDK blocking calls), process
//! exit. The parent times the stop request to the child's exit status.
#![cfg(test)]

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::receiver::Receiver;
use super::{
    Daemon, DaemonObservability, EXPORT_WAIT, endpoint_env, exported_counts, sent_message_id,
    task_record,
};
use atm_core::observability::AtmTelemetryExportState;
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
pub(super) const CHILD_SCENARIO_SENTINEL: &str = "BD6-CHILD-SCENARIO-COMPLETED";

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
    let output = spawn_child_scenario_with(scenario, envs);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        child_execution_is_proven(&output),
        "scenario child {scenario} did not complete successfully: {}\noutput:\n{stdout}",
        output.status
    );
}

fn spawn_child_scenario_with(scenario: &str, envs: &[(&str, &str)]) -> std::process::Output {
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", scenario, "--nocapture", "--test-threads=1"])
        .env(CHILD_SCENARIO, scenario)
        .env_remove(CHILD_MODE)
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn scenario child");
    let mut stdout = child.stdout.take().expect("scenario child stdout");
    let (stdout_tx, stdout_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = stdout_tx.send(bytes);
    });
    let (bytes, status) = await_eof_then_exit(
        &mut child,
        &stdout_rx,
        Instant::now() + Duration::from_secs(90),
        &format!("scenario child {scenario}"),
    );
    std::process::Output {
        status,
        stdout: bytes.expect("scenario child stdout bytes"),
        stderr: Vec::new(),
    }
}

/// Kills and reaps a child that outlived its deadline, so no failing test
/// leaves a process behind.
fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Waits for the child's stdout reader to finish (`eof` carries its result or
/// is dropped at EOF), then confirms the exit, all inside one absolute
/// `deadline`. Stdout EOF alone does not prove exit. On any timeout the child
/// is killed and reaped before the failure.
fn await_eof_then_exit<T>(
    child: &mut Child,
    eof: &mpsc::Receiver<T>,
    deadline: Instant,
    what: &str,
) -> (Option<T>, ExitStatus) {
    let carried = match eof.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(value) => Some(value),
        Err(mpsc::RecvTimeoutError::Disconnected) => None,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            kill_and_reap(child);
            panic!("{what} did not close stdout before its deadline");
        }
    };
    loop {
        if let Some(status) = child.try_wait().expect("poll child exit") {
            return (carried, status);
        }
        if Instant::now() >= deadline {
            kill_and_reap(child);
            panic!("{what} closed stdout but did not exit before its deadline");
        }
        std::thread::yield_now();
    }
}

fn child_execution_is_proven(output: &std::process::Output) -> bool {
    output.status.success()
        && String::from_utf8_lossy(&output.stdout).contains(CHILD_SCENARIO_SENTINEL)
}

#[test]
fn wrong_scenario_name_does_not_prove_child_execution() {
    let output = spawn_child_scenario_with(
        "telemetry_lifecycle_tests::exit::misspelled_child_scenario",
        &[],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "wrong-name child output: {stdout}");
    assert!(
        stdout.contains("running 0 tests"),
        "wrong-name child output: {stdout}"
    );
    assert!(
        !child_execution_is_proven(&output),
        "a successful zero-test child must fail the execution proof"
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
    let (eof_tx, eof_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        // Dropped at stdout EOF, which is the reader's completion signal.
        let _eof = eof_tx;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.contains("BD6-READY") {
                let _ = ready_tx.send(());
            }
        }
    });
    if ready_rx.recv_timeout(Duration::from_secs(60)).is_err() {
        kill_and_reap(&mut child);
        panic!("exit-proof child never became ready");
    }
    let mut stdin = child.stdin.take().expect("child stdin");
    let started = Instant::now();
    writeln!(stdin, "stop").expect("request stop");
    drop(stdin);
    let (_, status) = await_eof_then_exit(
        &mut child,
        &eof_rx,
        started + Duration::from_secs(30),
        "exit-proof child",
    );
    let elapsed = started.elapsed();
    assert!(status.success(), "child exit status {status}");
    elapsed
}

/// Positive: a child still serving when its deadline passes is killed and
/// reaped by the timeout path itself, before the failure is raised.
/// Negative: the child is never left running; its status is already collected
/// (a non-success kill status) when the panic reaches the caller.
#[test]
#[serial_test::parallel(slo)]
fn timed_out_child_is_killed_and_reaped_before_the_failure() {
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD_MODE, "clean")
        .env_remove(CHILD_ENDPOINT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn serving child");
    let stdout = child.stdout.take().expect("child stdout");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (eof_tx, eof_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let _eof = eof_tx;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.contains("BD6-READY") {
                let _ = ready_tx.send(());
            }
        }
    });
    if ready_rx.recv_timeout(Duration::from_secs(60)).is_err() {
        kill_and_reap(&mut child);
        panic!("serving child never became ready");
    }
    // Stdin stays open, so the child keeps serving past the deadline.
    let _stdin = child.stdin.take().expect("child stdin");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        await_eof_then_exit(
            &mut child,
            &eof_rx,
            Instant::now() + Duration::from_secs(2),
            "serving child",
        )
    }));
    assert!(outcome.is_err(), "a serving child must time out");
    let status = child
        .try_wait()
        .expect("poll child")
        .expect("the timeout path reaped the child before failing");
    assert!(!status.success(), "the child was killed: {status}");
}

/// Positive: with the task queue full and a collector that never answers,
/// the daemon process exits within the 10s force SLO.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial(slo)]
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
#[serial_test::serial(slo)]
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
    println!("{CHILD_SCENARIO_SENTINEL}");
}

/// Runs [`final_record_child`] against `endpoint` and returns the retained
/// JSONL lines left on disk after the child process exited.
async fn final_record_lines(endpoint: String) -> Vec<String> {
    // The logger root is the log directory's parent, as for the host `logs`.
    let root = tempfile::tempdir().expect("log root");
    let logs = root.path().join("logs");
    let log_dir = logs.to_str().expect("utf-8 log dir").to_owned();
    tokio::task::spawn_blocking(move || {
        run_child_scenario_with(
            FINAL_RECORD_CHILD,
            &[(CHILD_LOG_DIR, &log_dir), (CHILD_ENDPOINT, &endpoint)],
        );
    })
    .await
    .expect("parent driver");
    std::fs::read_to_string(logs.join(atm_observability::CANONICAL_LOG_FILE_NAME))
        .expect("retained log file")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Index of the final lifecycle record, required to follow every backlog
/// record on disk.
fn final_record_index(lines: &[String]) -> usize {
    let backlog: Vec<usize> = (0..lines.len())
        .filter(|&index| lines[index].contains(BACKLOG_RECORD))
        .collect();
    assert_eq!(
        backlog.len(),
        BACKLOG,
        "every admitted backlog record is on disk; last lines: {:?}",
        lines.iter().rev().take(3).collect::<Vec<_>>()
    );
    let index = lines
        .iter()
        .position(|line| line.contains(FINAL_RECORD))
        .unwrap_or_else(|| panic!("the final lifecycle record is on disk: {:?}", lines.last()));
    assert!(
        backlog.iter().all(|&queued| queued < index),
        "the final lifecycle record follows the backlog"
    );
    index
}

/// Positive: after `shutdown_replacement_daemon` and a real process exit, the
/// final lifecycle record is on disk behind a full backlog, and the collector
/// received it before the logger provider stopped. SDK diagnostics are never
/// exported back to the collector. Omitting the first flush loses it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::parallel(slo)]
async fn final_lifecycle_record_survives_process_exit() {
    let healthy = Receiver::start(false).await;
    let lines = final_record_lines(healthy.endpoint.clone()).await;
    final_record_index(&lines);
    let exported: Vec<String> = healthy
        .capture
        .logs
        .lock()
        .unwrap()
        .iter()
        .map(|record| format!("{record:?}"))
        .collect();
    assert!(
        exported.iter().any(|record| record.contains(FINAL_RECORD)),
        "the collector received the final lifecycle record"
    );
    assert!(
        !exported
            .iter()
            .any(|record| record.contains("opentelemetry") || record.contains("tonic")),
        "SDK diagnostics are never exported to the collector"
    );
    healthy.stop().await;
}

/// Positive: against a collector that never answers, the SDK's
/// provider-shutdown diagnostics reach disk after the final lifecycle record.
/// Delivery to the stalled collector is not asserted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::parallel(slo)]
async fn provider_shutdown_diagnostics_reach_disk_after_the_final_record() {
    let stalled = Receiver::start(true).await;
    let lines = final_record_lines(stalled.endpoint.clone()).await;
    let index = final_record_index(&lines);
    let sdk = lines[index + 1..]
        .iter()
        .filter(|line| line.contains("\"target\":\"opentelemetry_sdk\""))
        .count();
    assert!(
        sdk >= 1,
        "provider-shutdown diagnostics follow the final record on disk: {:?}",
        &lines[index..]
    );
    stalled.stop().await;
}

const COMBINED_CHILD: &str = "telemetry_lifecycle_tests::exit::combined_lifecycle_child";
const COMBINED_LOG: &str = "BD6_COMBINED_DELIVERED";
const COMBINED_TASKS: [&str; 3] = ["BD6-C1", "BD6-C2", "BD6-C3"];
/// Far above the task queue capacity, so admission must reject some.
const FLOOD: u64 = 4096;

/// Prints `marker` and waits for the parent's next stdin line.
async fn handshake(marker: String) {
    println!("{marker}");
    std::io::stdout().flush().expect("flush marker");
    tokio::task::spawn_blocking(|| std::io::stdin().read_line(&mut String::new()))
        .await
        .expect("stdin reader")
        .expect("parent line");
}

/// Child half of [`delivered_export_then_full_backlog_behind_stall`]; a no-op
/// unless launched by it.
#[test]
fn combined_lifecycle_child() {
    if !is_child_scenario(COMBINED_CHILD) {
        return;
    }
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
        let (root, observability) = Daemon::bootstrap(env).await;
        observability
            .install_tracing_bridge()
            .expect("the child process owns the global tracing bridge");
        let daemon = Daemon::compose(root, observability).await;
        for task in COMBINED_TASKS {
            sent_message_id(
                daemon
                    .write(daemon.request("sender", "recipient", task, None))
                    .await
                    .expect("assignment"),
            );
        }
        tracing::info!(target: "atm_daemon_bootstrap::lifecycle", code = COMBINED_LOG, "delivered");
        let committed = daemon.committed(&COMBINED_TASKS).await.len() as u64;
        handshake(format!("BD6-DELIVERING {committed}")).await;

        // The collector now stalls: fill the task queue behind it.
        for seq in 1..=FLOOD {
            daemon.workers.task_telemetry.try_emit(task_record(seq));
        }
        let task = daemon.workers.task_telemetry.diagnostics();
        let workflow = Arc::clone(daemon.workers.workflow_telemetry.diagnostics());
        assert!(task.snapshot().dropped_full > 0, "the task queue is full");
        let observability = daemon.observability.clone();
        handshake("BD6-READY".to_owned()).await;
        daemon.shutdown().await.expect("child daemon shutdown");

        let counts = task.snapshot();
        assert_eq!(
            counts.emitted
                + counts.dropped_full
                + counts.dropped_timeout
                + counts.dropped_failure
                + counts.dropped_shutdown,
            committed + FLOOD,
            "every task record is counted exactly once: {counts:?}"
        );
        // Runtime queue loss and SDK transport failure are both present and
        // disjoint: doctor's loss counts are exactly the runtime's, and the
        // stalled export shows only as the failure state.
        let health = observability.export_health_for_test();
        assert_ne!(health.state, AtmTelemetryExportState::Healthy, "{health:?}");
        assert!(health.last_failure.is_some(), "{health:?}");
        let load = |counter: &std::sync::atomic::AtomicU64| counter.load(Ordering::Relaxed);
        assert_eq!(
            (
                health.emitted,
                health.dropped_full,
                health.dropped_timeout,
                health.dropped_failure,
                health.dropped_shutdown,
            ),
            (
                counts.emitted,
                counts.dropped_full + load(&workflow.dropped_full),
                counts.dropped_timeout + load(&workflow.dropped_timeout),
                counts.dropped_failure + load(&workflow.dropped_failure),
                counts.dropped_shutdown + load(&workflow.dropped_shutdown),
            ),
            "{health:?}"
        );
    });
    drop(runtime);
    println!("{CHILD_SCENARIO_SENTINEL}");
}

/// Forwards each child stdout line; the second receiver disconnects at stdout
/// EOF.
fn forward_lines(
    stdout: std::process::ChildStdout,
) -> (mpsc::Receiver<String>, mpsc::Receiver<()>) {
    let (lines_tx, lines) = mpsc::channel();
    let (eof_tx, eof) = mpsc::channel();
    std::thread::spawn(move || {
        let _eof = eof_tx;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if lines_tx.send(line).is_err() {
                return;
            }
        }
    });
    (lines, eof)
}

/// The first child line containing `marker`.
async fn line_with(lines: &Arc<Mutex<mpsc::Receiver<String>>>, marker: &'static str) -> String {
    let lines = Arc::clone(lines);
    tokio::task::spawn_blocking(move || {
        let lines = lines.lock().expect("child lines");
        loop {
            let line = lines
                .recv_timeout(Duration::from_secs(60))
                .unwrap_or_else(|_| panic!("child never printed {marker}"));
            if line.contains(marker) {
                return line;
            }
        }
    })
    .await
    .expect("line reader")
}

/// Positive: one daemon process delivers task traces, metrics and a bridged
/// log to the collector; the collector then stalls with an export in flight
/// and the task queue fills behind it. Together: every task record is counted
/// exactly once in the runtime counters, doctor's loss counts equal those
/// runtime counts while the stalled export shows only as the SDK failure
/// state, and the stop request to process exit stays within the 10s force SLO.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial(slo)]
async fn delivered_export_then_full_backlog_behind_stall() {
    let receiver = Receiver::start(false).await;
    let capture = receiver.capture.clone();
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", COMBINED_CHILD, "--nocapture", "--test-threads=1"])
        .env(CHILD_SCENARIO, COMBINED_CHILD)
        .env(CHILD_ENDPOINT, &receiver.endpoint)
        .env_remove(CHILD_MODE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn combined child");
    let mut stdin = child.stdin.take().expect("child stdin");
    let (lines, eof) = forward_lines(child.stdout.take().expect("child stdout"));
    let lines = Arc::new(Mutex::new(lines));

    let delivering = line_with(&lines, "BD6-DELIVERING").await;
    let committed: usize = delivering
        .rsplit(' ')
        .next()
        .and_then(|count| count.parse().ok())
        .expect("committed row count");
    capture
        .wait(EXPORT_WAIT, "delivered traces, metrics and logs", || {
            let spans = capture.spans.lock().unwrap();
            spans
                .iter()
                .filter(|span| span.name == "atm.task.event")
                .count()
                >= committed
                && exported_counts(&capture).get("assigned") == Some(&3)
                && capture
                    .logs
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|record| format!("{record:?}").contains(COMBINED_LOG))
        })
        .await;
    capture.begin_stall();
    let before = capture.started.load(Ordering::SeqCst);
    writeln!(stdin, "flood").expect("request flood");
    line_with(&lines, "BD6-READY").await;
    capture
        .wait(EXPORT_WAIT, "an export stalled in flight", || {
            capture.started.load(Ordering::SeqCst) > before
        })
        .await;

    let started = Instant::now();
    writeln!(stdin, "stop").expect("request stop");
    drop(stdin);
    // The test owns `child`; the bounded blocking wait runs in place.
    let (_, status) = tokio::task::block_in_place(|| {
        await_eof_then_exit(
            &mut child,
            &eof,
            started + Duration::from_secs(30),
            "combined child",
        )
    });
    let elapsed = started.elapsed();
    assert!(status.success(), "child exit status {status}");
    line_with(&lines, CHILD_SCENARIO_SENTINEL).await;
    assert!(
        elapsed <= Duration::from_secs(10),
        "stop to exit took {elapsed:?}"
    );
    receiver.stop().await;
}
