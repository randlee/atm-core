//! The `atm` CLI routes its retained log records to the destination the
//! daemon uses (`ATM_LOG_DESTINATION`, ADR-064 D4), through the real binary.

use std::path::Path;
use std::process::Output;
use std::sync::{Arc, Mutex};

use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsServiceRequest, ExportLogsServiceResponse,
    logs_service_server::{LogsService, LogsServiceServer},
};
use opentelemetry_proto::tonic::logs::v1::LogRecord;
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::codec::CompressionEncoding;
use tonic::{Request, Response, Status};

/// The validation error `atm ack` records for a malformed message id.
const CODE: &str = "ATM_MESSAGE_VALIDATION_FAILED";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<LogRecord>>>);

#[tonic::async_trait]
impl LogsService for Capture {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        self.0.lock().unwrap().extend(
            request
                .into_inner()
                .resource_logs
                .into_iter()
                .flat_map(|resource| resource.scope_logs)
                .flat_map(|scope| scope.log_records),
        );
        Ok(Response::new(ExportLogsServiceResponse::default()))
    }
}

impl Capture {
    fn count(&self, needle: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|record| format!("{record:?}").contains(needle))
            .count()
    }
}

async fn atm_ack(home: &Path, endpoint: &str, destination: &str) -> Output {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_atm"))
        .args(["ack", "localhost", "received"])
        .env("ATM_HOME", home)
        .env("ATM_CONFIG_HOME", home.join("config"))
        .env("ATM_LOG_DIR", home.join("logs"))
        .env("ATM_TEAMS_DIR", home.join("teams"))
        .env("ATM_IDENTITY", "sender-a")
        .env("ATM_TEAM", "test-team")
        .env("ATM_OTEL_ENDPOINT", endpoint)
        .env("ATM_LOG_DESTINATION", destination)
        .env_remove("ATM_OTEL_AUTH_HEADER")
        .env_remove("ATM_OTEL_PROTOCOL")
        .output()
        .await
        .expect("run atm ack")
}

fn jsonl_count(home: &Path, needle: &str) -> Option<usize> {
    let log = std::fs::read_to_string(home.join("logs").join("atm.log.jsonl")).ok()?;
    Some(log.lines().filter(|line| line.contains(needle)).count())
}

/// Positive: `otel` and `both` deliver the CLI's command-error record to the
/// collector before the process exits (no wait after exit), and `file` and
/// `both` write it to the retained JSONL.
/// Negative: `file` exports nothing, `otel` writes no JSONL, and an
/// unsupported destination falls back to file logging without exporting.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_log_records_reach_the_configured_destination() {
    for (destination, exported, filed) in [
        ("file", 0, Some(1)),
        ("otel", 1, None),
        ("both", 1, Some(1)),
        ("syslog", 0, Some(1)),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let capture = Capture::default();
        let (stop, stopped) = oneshot::channel::<()>();
        let server = tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(
                    LogsServiceServer::new(capture.clone())
                        .accept_compressed(CompressionEncoding::Gzip),
                )
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                }),
        );
        let home = tempfile::tempdir().expect("temporary ATM environment");

        let output = atm_ack(home.path(), &endpoint, destination).await;

        assert_eq!(
            output.status.code(),
            Some(3),
            "{destination}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(capture.count(CODE), exported, "{destination}: exported");
        assert_eq!(
            jsonl_count(home.path(), CODE),
            filed,
            "{destination}: JSONL"
        );
        let _ = stop.send(());
        server.await.expect("server task").expect("server");
    }
}

fn atm_ack_with_endpoint_stderr(home: &Path, endpoint: &str, extra: &[&str]) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_atm"))
        .args(extra)
        .args(["ack", "localhost", "received"])
        .env("ATM_HOME", home)
        .env("ATM_CONFIG_HOME", home.join("config"))
        .env("ATM_LOG_DIR", home.join("logs"))
        .env("ATM_TEAMS_DIR", home.join("teams"))
        .env("ATM_IDENTITY", "sender-a")
        .env("ATM_TEAM", "test-team")
        .env("ATM_OTEL_ENDPOINT", endpoint)
        .env("ATM_LOG_DESTINATION", "otel")
        .env_remove("ATM_OTEL_AUTH_HEADER")
        .env_remove("ATM_OTEL_PROTOCOL")
        .output()
        .expect("run atm ack");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Positive: an invalid export configuration prints
/// `ATM_TELEMETRY_EXPORT_CONFIG_INVALID` on stderr without `--stderr-logs`.
/// Negative: a valid configuration prints no such warning.
#[test]
fn invalid_export_configuration_warns_without_stderr_logs() {
    let home = tempfile::tempdir().expect("tempdir");
    let invalid = atm_ack_with_endpoint_stderr(home.path(), "ftp://collector:4317", &[]);
    assert!(
        invalid.contains("ATM_TELEMETRY_EXPORT_CONFIG_INVALID"),
        "stderr: {invalid}"
    );
    let valid = atm_ack_with_endpoint_stderr(home.path(), "http://127.0.0.1:9", &[]);
    assert!(
        !valid.contains("ATM_TELEMETRY_EXPORT_CONFIG_INVALID"),
        "stderr: {valid}"
    );
}
