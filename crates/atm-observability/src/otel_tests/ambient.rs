#![cfg(test)]

use super::*;
use std::io;
use std::process::{Output, Stdio};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::task::JoinHandle;

async fn read_pipe<R: AsyncRead + Unpin>(mut pipe: R) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output).await?;
    Ok(output)
}

async fn join_pipe(reader: JoinHandle<io::Result<Vec<u8>>>) -> io::Result<Vec<u8>> {
    reader
        .await
        .map_err(|error| io::Error::other(format!("child output reader task failed: {error}")))?
}

async fn run_child(command: &mut Command) -> io::Result<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout_reader = tokio::spawn(read_pipe(stdout));
    let stderr_reader = tokio::spawn(read_pipe(stderr));
    let status = child.wait().await?;

    let stdout = join_pipe(stdout_reader).await?;
    let stderr = join_pipe(stderr_reader).await?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[tokio::test]
async fn explicit_atm_configuration_ignores_ambient_otel_settings() {
    // Isolate environment mutation from every other test and SDK worker.
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "otel_tests::ambient::ambient_child",
            "--nocapture",
        ])
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:1")
        .env("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", "http://127.0.0.1:1")
        .env(
            "OTEL_EXPORTER_OTLP_HEADERS",
            "authorization=Bearer ambient-secret,x-ambient=secret",
        )
        .env(
            "OTEL_EXPORTER_OTLP_TRACES_HEADERS",
            "authorization=Bearer ambient-secret,x-ambient=secret",
        )
        .env("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json")
        .env("OTEL_EXPORTER_OTLP_COMPRESSION", "unsupported")
        .env("OTEL_EXPORTER_OTLP_TIMEOUT", "1")
        .env(
            "OTEL_RESOURCE_ATTRIBUTES",
            "service.name=wrong,secret=ambient-secret",
        )
        .env("OTEL_SERVICE_NAME", "wrong")
        .env("OTEL_TRACES_SAMPLER", "always_off")
        .env("OTEL_METRIC_EXPORT_INTERVAL", "3600000")
        .env("OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE", "delta")
        .env("OTEL_BSP_MAX_QUEUE_SIZE", "1")
        .env("OTEL_BSP_MAX_EXPORT_BATCH_SIZE", "1");
    let output = run_child(&mut command)
        .await
        .expect("ambient-settings child process should run");
    assert!(
        output.status.success(),
        "ambient-settings child failed; status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("ambient-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("ambient-secret"));
}

#[tokio::test(start_paused = true)]
async fn ambient_child() {
    // The parent invokes this exact libtest filter in a separate process.
    if !std::env::args().any(|argument| argument == "otel_tests::ambient::ambient_child") {
        return;
    }
    let receiver = Receiver::start(false).await;
    let trace_before = receiver.capture.finished(Signal::Trace);
    let metric_before = receiver.capture.finished(Signal::Metric);
    let setup = setup_transport(&config(&receiver.endpoint));
    setup
        .0
        .sink
        .emit(record("ambient", TaskTelemetryKind::Completed, 1, 1))
        .unwrap();
    shutdown_production(setup).await;
    receiver
        .capture
        .wait("a span and a metric export", || {
            !receiver.capture.spans.lock().unwrap().is_empty()
                && !receiver.capture.metrics.lock().unwrap().is_empty()
        })
        .await;
    {
        let resources = receiver.capture.resources.lock().unwrap();
        assert!(!resources.is_empty());
        for resource in resources.iter() {
            assert_eq!(resource.attributes.len(), 1);
            assert_eq!(resource.attributes[0].key, "service.name");
            assert!(format!("{:?}", resource.attributes[0].value).contains("atm-fixture"));
        }
    }
    wait_for_task_exports(&receiver, trace_before, metric_before).await;
    assert_transport_metadata(&receiver.capture);
    {
        let metrics = receiver.capture.metrics.lock().unwrap();
        assert!(
            metrics
                .iter()
                .any(|metric| metric.name == "atm.task.events")
        );
        for metric in metrics.iter() {
            if let Some(Data::Sum(sum)) = &metric.data {
                assert_eq!(
                    sum.aggregation_temporality,
                    opentelemetry_proto::tonic::metrics::v1::AggregationTemporality::Cumulative
                        as i32
                );
            }
        }
    }
    receiver.stop().await;
}
