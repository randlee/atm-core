//! Real process-exit proof of the daemon shutdown sequence.
//!
//! The parent re-executes this test binary as a child that composes the
//! daemon on its own multi-thread runtime, exports to a collector living in
//! the parent process, and stops on request: `shutdown_replacement_daemon`,
//! runtime teardown (which releases abandoned SDK blocking calls), process
//! exit. The parent awaits the child's exit status. No stop-time bound is
//! asserted here. The benchmark smoke run (`scripts/smoke`) checks only that a
//! clean stop of the shipped binary exits 0 within 5s; the 10s bound for a full
//! queue and a stalled collector is not measured by either.
#![cfg(test)]

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::receiver::Receiver;
use super::{
    Daemon, DaemonObservability, EXPORT_WAIT, assert_one_shutdown_deadline, endpoint_env,
    exported_counts, sent_message_id, task_record,
};
use crate::shutdown_probe::{Probe, observe};
use atm_core::observability::{AtmTelemetryExportHealth, AtmTelemetryExportState};
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
        let probe = Probe::new();
        observe(&probe, daemon.shutdown())
            .await
            .expect("child daemon shutdown");
        // One shared deadline, every step returned by it. The parent sees a
        // failure here as an unsuccessful exit.
        assert_one_shutdown_deadline(&probe.steps());
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

/// Tick of the bounded exit poll; the absolute deadline, not this, bounds the wait.
const EXIT_POLL: Duration = Duration::from_millis(20);

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
    // Bounded `try_wait` polling inside the same absolute deadline. On expiry
    // the child is killed and reaped here, by the owning handle, then fails.
    loop {
        if let Some(status) = child.try_wait().expect("poll child exit") {
            return (carried, status);
        }
        if Instant::now() >= deadline {
            kill_and_reap(child);
            panic!("{what} closed stdout but did not exit before its deadline");
        }
        std::thread::park_timeout(
            EXIT_POLL.min(deadline.saturating_duration_since(Instant::now())),
        );
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

/// Waits for proof that an export reached the collector before a stop request.
fn wait_for_pre_stop_export(
    collector_started: &mpsc::Receiver<()>,
) -> Result<(), mpsc::RecvTimeoutError> {
    collector_started.recv_timeout(EXPORT_WAIT)
}

/// Launches the child, waits until it serves, verifies an optional collector
/// export is already stalled, requests the stop and asserts the child exits
/// successfully, which includes its own shutdown-deadline proof. The 30 s
/// wait is a hang diagnostic only; no elapsed time is asserted.
fn stop_to_exit(mode: &str, endpoint: Option<&str>, collector_started: Option<mpsc::Receiver<()>>) {
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
    if let Some(collector_started) = collector_started {
        wait_for_pre_stop_export(&collector_started).unwrap_or_else(|error| {
            kill_and_reap(&mut child);
            panic!("stalled collector export did not start before stop: {error}");
        });
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
    assert!(status.success(), "child exit status {status}");
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

/// Positive: a child that closed its stdout report but never exits is killed
/// and reaped by the exit-wait timeout path, inside its deadline.
/// Negative: the child is never left running or blocking the wait; no external
/// kill binary is involved.
#[test]
#[serial_test::parallel(slo)]
fn child_that_never_exits_is_killed_and_reaped_by_the_exit_wait() {
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD_MODE, "clean")
        .env_remove(CHILD_ENDPOINT)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn never-exiting child");
    // Stdin stays open, so the child keeps serving and never exits on its own.
    let _stdin = child.stdin.take().expect("child stdin");
    let (eof_tx, eof_rx) = mpsc::channel::<()>();
    drop(eof_tx); // stdout reported EOF at once; only the exit wait can time out
    let started = Instant::now();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        await_eof_then_exit(
            &mut child,
            &eof_rx,
            started + Duration::from_secs(2),
            "never-exiting child",
        )
    }));
    assert!(outcome.is_err(), "a child that never exits must time out");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the exit wait returned at its deadline, not after a blocked wait"
    );
    let status = child
        .try_wait()
        .expect("poll child")
        .expect("the exit-wait path reaped the child before failing");
    assert!(!status.success(), "the child was killed: {status}");
}

#[test]
fn pre_stop_export_gate_rejects_a_missing_export() {
    let (collector_started, gate) = mpsc::channel();
    drop(collector_started);
    assert!(
        matches!(
            wait_for_pre_stop_export(&gate),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ),
        "the stop gate must reject a missing collector export"
    );
}

/// Positive: with the task queue full and a collector that never answers,
/// the daemon process exits successfully after shutdown, its shutdown steps
/// sharing the one deadline fixed at entry.
/// Negative: no elapsed time is asserted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial(slo)]
async fn process_exits_successfully_after_shutdown_with_full_queue_and_stalled_collector() {
    let stalled = Receiver::start(true).await;
    let endpoint = stalled.endpoint.clone();
    let capture = stalled.capture.clone();
    let (collector_started, gate) = mpsc::channel();
    let export_started = tokio::spawn(async move {
        capture
            .wait(
                EXPORT_WAIT,
                "a stalled collector export before stop",
                || capture.started.load(Ordering::SeqCst) > 0,
            )
            .await;
        collector_started
            .send(())
            .expect("parent driver waits for the collector export");
    });
    tokio::task::spawn_blocking(move || stop_to_exit("full", Some(&endpoint), Some(gate)))
        .await
        .expect("parent driver");
    export_started.await.expect("collector observation");
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

/// Positive: with a healthy collector the daemon process exits successfully
/// after a clean shutdown that flushed its export, its shutdown steps sharing
/// the one deadline fixed at entry.
/// Negative: no elapsed time is asserted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial(slo)]
async fn process_exits_successfully_after_clean_shutdown() {
    let healthy = Receiver::start(false).await;
    let endpoint = healthy.endpoint.clone();
    tokio::task::spawn_blocking(move || stop_to_exit("clean", Some(&endpoint), None))
        .await
        .expect("parent driver");
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
const CHILD_BACKLOG: &str = "ATM_BD6_BACKLOG";
/// Set when the parent will confirm on stdin that its collector stored the
/// final record; the child keeps its runtime, and so the SDK export tasks,
/// running until then.
const CHILD_HOLD: &str = "ATM_BD6_HOLD";
/// Failure-only bound on the held child's wait for the parent.
const PARENT_CONFIRMATION_LIMIT: Duration = Duration::from_secs(30);
/// Prefixes the child's export health, printed after daemon shutdown.
const EXPORT_HEALTH_MARKER: &str = "BD6-EXPORT-HEALTH ";
/// The bridge retains allowlisted fields only, never message text.
const FINAL_RECORD: &str = "ATM_DAEMON_SHUTDOWN_DRAINED";
const BACKLOG_RECORD: &str = "BD6_RETAINED_BACKLOG";
/// Long enough that the retained writer is still behind when the process
/// exits undrained (without the first flush the final record is lost), and
/// small enough that it, the final record and the daemon's other lifecycle
/// records fit the 256-record OpenTelemetry log queue (`atm_observability`
/// `EXPORT_QUEUE`) with nothing drained, so the SDK never drops the final
/// record as queue-full. A 512-record backlog filled that queue while the
/// first export was in flight and dropped it.
const DELIVERED_BACKLOG: usize = 192;
/// Below the 1024-event logger queue, so every record is admitted, and over
/// the OpenTelemetry log queue plus one export batch, so a collector that
/// never answers forces the SDK's log-dropping diagnostics.
const OVERFLOW_BACKLOG: usize = 512;

/// Child half of [`final_lifecycle_record_reaches_disk_and_collector`] and
/// [`provider_shutdown_diagnostics_reach_disk_after_the_final_record`]; a
/// no-op unless launched by them.
#[test]
fn final_record_child() {
    if !is_child_scenario(FINAL_RECORD_CHILD) {
        return;
    }
    let log_dir = std::env::var(CHILD_LOG_DIR).expect("log dir");
    let endpoint = std::env::var(CHILD_ENDPOINT).expect("collector endpoint");
    let backlog: usize = std::env::var(CHILD_BACKLOG)
        .expect("backlog")
        .parse()
        .expect("backlog count");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("child runtime");
    let observability = runtime.block_on(async {
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
        let daemon = Daemon::compose(root, observability.clone()).await;
        for seq in 0..backlog {
            tracing::info!(target: "atm_daemon_bootstrap::lifecycle", code = BACKLOG_RECORD, attempt = seq, "backlog");
        }
        daemon.shutdown().await.expect("child daemon shutdown");
        observability
    });
    if std::env::var_os(CHILD_HOLD).is_some() {
        // Shutdown may have abandoned its 1s wait on the final export, not the
        // export: the live runtime still delivers it.
        await_parent_confirmation();
    }
    let health =
        serde_json::to_string(&observability.export_health_for_test()).expect("export health json");
    println!("{EXPORT_HEALTH_MARKER}{health}");
    // Process exit follows; nothing in the child flushes the logger itself.
    drop(runtime);
    println!("{CHILD_SCENARIO_SENTINEL}");
}

/// Blocks until the parent writes a line on stdin, failing at
/// [`PARENT_CONFIRMATION_LIMIT`]. The reader thread is detached, so a failed
/// wait does not hold the process.
fn await_parent_confirmation() {
    let (sender, confirmed) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = sender.send(std::io::stdin().read_line(&mut line));
    });
    confirmed
        .recv_timeout(PARENT_CONFIRMATION_LIMIT)
        .expect("the parent confirmed that its collector stored the final record")
        .expect("read the parent's confirmation");
}

/// The retained JSONL lines in `logs` after the child exited.
pub(super) fn retained_lines(logs: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(logs.join(atm_observability::CANONICAL_LOG_FILE_NAME))
        .expect("retained log file")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The export health the child printed after its daemon shutdown.
fn child_export_health(stdout: &str) -> AtmTelemetryExportHealth {
    let line = stdout
        .lines()
        // libtest's progress text can share the line.
        .find_map(|line| line.split_once(EXPORT_HEALTH_MARKER).map(|(_, json)| json))
        .unwrap_or_else(|| panic!("the child printed its export health:\n{stdout}"));
    serde_json::from_str(line).expect("child export health parses")
}

/// Runs [`final_record_child`] with `backlog` records against `endpoint`, the
/// child exiting right after shutdown, and returns the retained JSONL lines
/// left on disk and the child's export health.
async fn final_record_exit(
    endpoint: String,
    backlog: usize,
) -> (Vec<String>, AtmTelemetryExportHealth) {
    // The logger root is the log directory's parent, as for the host `logs`.
    let root = tempfile::tempdir().expect("log root");
    let logs = root.path().join("logs");
    let log_dir = logs.to_str().expect("utf-8 log dir").to_owned();
    let output = tokio::task::spawn_blocking(move || {
        spawn_child_scenario_with(
            FINAL_RECORD_CHILD,
            &[
                (CHILD_LOG_DIR, &log_dir),
                (CHILD_ENDPOINT, &endpoint),
                (CHILD_BACKLOG, &backlog.to_string()),
            ],
        )
    })
    .await
    .expect("parent driver");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        child_execution_is_proven(&output),
        "final-record child did not complete successfully: {}\noutput:\n{stdout}",
        output.status
    );
    (retained_lines(&logs), child_export_health(&stdout))
}

/// Runs [`final_record_child`] holding after shutdown until `receiver` stored
/// the final record, then confirms on stdin, and returns the retained JSONL
/// lines left on disk after the child exited.
async fn final_record_held(receiver: &Receiver, backlog: usize) -> Vec<String> {
    let root = tempfile::tempdir().expect("log root");
    let logs = root.path().join("logs");
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            FINAL_RECORD_CHILD,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_SCENARIO, FINAL_RECORD_CHILD)
        .env_remove(CHILD_MODE)
        .env(CHILD_LOG_DIR, &logs)
        .env(CHILD_ENDPOINT, &receiver.endpoint)
        .env(CHILD_BACKLOG, backlog.to_string())
        .env(CHILD_HOLD, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn final-record child");
    let mut stdin = child.stdin.take().expect("final-record child stdin");
    let mut stdout = child.stdout.take().expect("final-record child stdout");
    let (stdout_tx, stdout_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = stdout_tx.send(bytes);
    });
    // Failure-only: a child that never delivers the record exits at its own
    // confirmation limit.
    let capture = &receiver.capture;
    capture
        .wait(EXPORT_WAIT, "the final lifecycle record", || {
            capture
                .logs
                .lock()
                .unwrap()
                .iter()
                .any(|record| format!("{record:?}").contains(FINAL_RECORD))
        })
        .await;
    writeln!(stdin, "stored").expect("confirm to the child");
    drop(stdin);
    let (bytes, status) = tokio::task::spawn_blocking(move || {
        // Hang diagnostic only; no elapsed time is asserted.
        await_eof_then_exit(
            &mut child,
            &stdout_rx,
            Instant::now() + Duration::from_secs(30),
            "held final-record child",
        )
    })
    .await
    .expect("parent driver");
    let output = std::process::Output {
        status,
        stdout: bytes.expect("final-record child stdout bytes"),
        stderr: Vec::new(),
    };
    assert!(
        child_execution_is_proven(&output),
        "held final-record child did not complete successfully: {}\noutput:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
    );
    retained_lines(&logs)
}

/// Index of the final lifecycle record, required to follow all `backlog`
/// records on disk.
fn final_record_index(lines: &[String], backlog: usize) -> usize {
    let queued: Vec<usize> = (0..lines.len())
        .filter(|&index| lines[index].contains(BACKLOG_RECORD))
        .collect();
    assert_eq!(
        queued.len(),
        backlog,
        "every admitted backlog record is on disk; last lines: {:?}",
        lines.iter().rev().take(3).collect::<Vec<_>>()
    );
    let index = lines
        .iter()
        .position(|line| line.contains(FINAL_RECORD))
        .unwrap_or_else(|| panic!("the final lifecycle record is on disk: {:?}", lines.last()));
    assert!(
        queued.iter().all(|&queued| queued < index),
        "the final lifecycle record follows the backlog"
    );
    index
}

/// Positive: after `shutdown_replacement_daemon` and a real process exit, the
/// final lifecycle record is on disk behind a full backlog, and the collector
/// holds it. The child exits only once the parent saw the record stored, so
/// no real-time delivery bound is asserted. Omitting the first flush loses it
/// from disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::parallel(slo)]
async fn final_lifecycle_record_reaches_disk_and_collector() {
    let healthy = Receiver::start(false).await;
    let lines = final_record_held(&healthy, DELIVERED_BACKLOG).await;
    final_record_index(&lines, DELIVERED_BACKLOG);
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
        "the collector holds the final lifecycle record"
    );
    assert!(
        !exported
            .iter()
            .any(|record| record.contains("opentelemetry") || record.contains("tonic")),
        "SDK diagnostics are never exported to the collector"
    );
    healthy.stop().await;
}

/// Positive: against a collector that never acknowledges, the child exits
/// (the scenario runner's deadline is a hang diagnostic only), its export
/// health reports the abandoned export, and, with a backlog over the export
/// queue, the SDK's log-dropping diagnostics reach disk after the final
/// lifecycle record. Delivery to the stalled collector is not asserted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::parallel(slo)]
async fn provider_shutdown_diagnostics_reach_disk_after_the_final_record() {
    let stalled = Receiver::start(true).await;
    let (lines, health) = final_record_exit(stalled.endpoint.clone(), OVERFLOW_BACKLOG).await;
    assert!(health.last_failure.is_some(), "{health:?}");
    assert_ne!(health.state, AtmTelemetryExportState::Healthy, "{health:?}");
    let index = final_record_index(&lines, OVERFLOW_BACKLOG);
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
        assert!(task.snapshot().dropped_full > 0, "the task queue is full");
        let observability = daemon.observability.clone();
        handshake("BD6-READY".to_owned()).await;
        let probe = Probe::new();
        observe(&probe, daemon.shutdown())
            .await
            .expect("child daemon shutdown");
        // The stalled collector is cut off by the export's own 1s bound, so
        // every step still returns by the shared deadline.
        assert_one_shutdown_deadline(&probe.steps());

        let counts = task.snapshot();
        assert_eq!(
            counts.emitted + counts.dropped_full + counts.dropped_failure + counts.dropped_shutdown,
            committed + FLOOD,
            "every task record is counted exactly once: {counts:?}"
        );
        // Runtime queue loss and SDK transport failure are both present and
        // disjoint: doctor's loss counts are exactly the runtime's, and the
        // stalled export shows only as the failure state.
        let health = observability.export_health_for_test();
        assert_ne!(health.state, AtmTelemetryExportState::Healthy, "{health:?}");
        assert!(health.last_failure.is_some(), "{health:?}");
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
                counts.dropped_full,
                0,
                counts.dropped_failure,
                counts.dropped_shutdown,
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
/// state, and every shutdown step shares the one deadline fixed at entry; no
/// elapsed time is asserted.
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

    // The driving steps run in their own task so a panicking or timed-out
    // step cannot leave `child` running: the test keeps ownership of `child`
    // and kills and reaps it before re-raising that panic.
    let driven = tokio::spawn({
        let (capture, lines) = (capture.clone(), Arc::clone(&lines));
        async move {
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

            (stdin, Instant::now())
        }
    });
    let (mut stdin, started) = match driven.await {
        Ok(driven) => driven,
        Err(error) => {
            kill_and_reap(&mut child);
            std::panic::resume_unwind(error.into_panic());
        }
    };
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
    assert!(status.success(), "child exit status {status}");
    line_with(&lines, CHILD_SCENARIO_SENTINEL).await;
    receiver.stop().await;
}
