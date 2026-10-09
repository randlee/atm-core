//! The retained shutdown-loss record: stopping the daemon with the collector
//! down leaves one `ATM_TELEMETRY_SHUTDOWN_LOSS` record on disk.
#![cfg(test)]

use atm_core::test_support::FakeEnvSource;

use super::{Daemon, DaemonObservability, exit, lossy_task_runtime};

const SHUTDOWN_LOSS_CHILD: &str = "telemetry_lifecycle_tests::shutdown_loss::shutdown_loss_child";

/// Positive: a daemon stopped with its collector down and known runtime
/// losses writes one retained `ATM_TELEMETRY_SHUTDOWN_LOSS` record carrying
/// the nonzero loss counts, the provider shutdown outcome and the timeout
/// flag. Runs in a child process because the bridge is process-global.
#[test]
#[serial_test::serial(slo)]
fn stopping_with_the_collector_down_retains_the_shutdown_loss_record() {
    exit::run_child_scenario(SHUTDOWN_LOSS_CHILD);
}

/// Child half of [`stopping_with_the_collector_down_retains_the_shutdown_loss_record`];
/// a no-op unless launched by [`exit::run_child_scenario`].
#[test]
fn shutdown_loss_child() {
    if !exit::is_child_scenario(SHUTDOWN_LOSS_CHILD) {
        return;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("child runtime");
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let down = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        // The logs outlive the daemon root, which shutdown removes. The logger
        // root is the log directory's parent, as for the host `logs`.
        let log_root = tempfile::tempdir().expect("log root");
        let logs = log_root.path().join("logs");
        // `both` keeps the retained JSONL beside the OTLP export.
        let env = FakeEnvSource::new([
            ("ATM_OTEL_ENDPOINT", Some(down.as_str())),
            ("ATM_LOG_DESTINATION", Some("both")),
        ]);
        let observability = DaemonObservability::bootstrap_from(&env, logs.clone())
            .await
            .expect("daemon observability bootstraps");
        observability
            .install_tracing_bridge()
            .expect("the child process owns the global tracing bridge");
        let (lossy, _release) = lossy_task_runtime().await;
        observability.attach_runtime_telemetry(lossy.diagnostics());
        let root = tempfile::tempdir().expect("daemon root");
        let daemon = Daemon::compose(root, observability).await;
        daemon
            .shutdown()
            .await
            .expect("shutdown result is the listener's");

        let lines = exit::retained_lines(&logs);
        let records: Vec<serde_json::Value> = lines
            .iter()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("retained JSONL"))
            .filter(|record| record["fields"]["code"] == "ATM_TELEMETRY_SHUTDOWN_LOSS")
            .collect();
        let [record] = records.as_slice() else {
            panic!("one shutdown loss record on disk: {lines:#?}");
        };
        let fields = &record["fields"];
        assert_eq!(fields["dropped_full"], 4, "{record}");
        for count in [
            "emitted",
            "dropped_timeout",
            "dropped_failure",
            "dropped_shutdown",
        ] {
            assert!(fields[count].is_u64(), "{count} recorded: {record}");
        }
        assert!(
            ["completed", "failed", "timed_out"]
                .contains(&fields["outcome"].as_str().unwrap_or("")),
            "provider shutdown outcome recorded: {record}"
        );
        assert!(fields["shutdown_timed_out"].is_boolean(), "{record}");
    });
    drop(runtime);
    println!("{}", exit::CHILD_SCENARIO_SENTINEL);
}
