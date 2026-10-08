//! In-process OTLP gRPC collector for the daemon lifecycle proofs.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opentelemetry_proto::tonic::collector::{
    logs::v1 as logs, metrics::v1 as metrics, trace::v1 as traces,
};
use opentelemetry_proto::tonic::{logs::v1::LogRecord, metrics::v1::Metric, trace::v1::Span};
use tokio::sync::{Notify, oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::codec::CompressionEncoding;
use tonic::{Request, Response, Status};

#[derive(Clone, Default)]
pub(super) struct Capture {
    pub spans: Arc<Mutex<Vec<Span>>>,
    pub logs: Arc<Mutex<Vec<LogRecord>>>,
    pub metrics: Arc<Mutex<Vec<Metric>>>,
    pub started: Arc<AtomicUsize>,
    changed: Arc<Notify>,
    /// A stalled export never answers; the test drops the receiver instead.
    stall: bool,
}

impl Capture {
    async fn respond(&self) {
        self.started.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_waiters();
        if self.stall {
            std::future::pending::<()>().await;
        }
    }

    /// Waits until `condition` holds, panicking with `what` at `limit`.
    pub async fn wait(&self, limit: Duration, what: &str, condition: impl Fn() -> bool) {
        let reached = tokio::time::timeout(limit, async {
            loop {
                let changed = self.changed.notified();
                if condition() {
                    return;
                }
                changed.await;
            }
        })
        .await;
        assert!(
            reached.is_ok(),
            "receiver never observed {what} within {limit:?}"
        );
    }
}

#[tonic::async_trait]
impl traces::trace_service_server::TraceService for Capture {
    async fn export(
        &self,
        request: Request<traces::ExportTraceServiceRequest>,
    ) -> Result<Response<traces::ExportTraceServiceResponse>, Status> {
        self.spans.lock().unwrap().extend(
            request
                .into_inner()
                .resource_spans
                .into_iter()
                .flat_map(|resource| resource.scope_spans)
                .flat_map(|scope| scope.spans),
        );
        self.respond().await;
        Ok(Response::new(traces::ExportTraceServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl logs::logs_service_server::LogsService for Capture {
    async fn export(
        &self,
        request: Request<logs::ExportLogsServiceRequest>,
    ) -> Result<Response<logs::ExportLogsServiceResponse>, Status> {
        self.logs.lock().unwrap().extend(
            request
                .into_inner()
                .resource_logs
                .into_iter()
                .flat_map(|resource| resource.scope_logs)
                .flat_map(|scope| scope.log_records),
        );
        self.respond().await;
        Ok(Response::new(logs::ExportLogsServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl metrics::metrics_service_server::MetricsService for Capture {
    async fn export(
        &self,
        request: Request<metrics::ExportMetricsServiceRequest>,
    ) -> Result<Response<metrics::ExportMetricsServiceResponse>, Status> {
        self.metrics.lock().unwrap().extend(
            request
                .into_inner()
                .resource_metrics
                .into_iter()
                .flat_map(|resource| resource.scope_metrics)
                .flat_map(|scope| scope.metrics),
        );
        self.respond().await;
        Ok(Response::new(
            metrics::ExportMetricsServiceResponse::default(),
        ))
    }
}

pub(super) struct Receiver {
    pub endpoint: String,
    pub capture: Capture,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl Receiver {
    pub async fn start(stall: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let capture = Capture {
            stall,
            ..Capture::default()
        };
        let (stop, stopped) = oneshot::channel();
        let server = tonic::transport::Server::builder()
            .add_service(
                traces::trace_service_server::TraceServiceServer::new(capture.clone())
                    .accept_compressed(CompressionEncoding::Gzip),
            )
            .add_service(
                logs::logs_service_server::LogsServiceServer::new(capture.clone())
                    .accept_compressed(CompressionEncoding::Gzip),
            )
            .add_service(
                metrics::metrics_service_server::MetricsServiceServer::new(capture.clone())
                    .accept_compressed(CompressionEncoding::Gzip),
            )
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = stopped.await;
            });
        let task = tokio::spawn(async move {
            let _ = server.await;
        });
        Self {
            endpoint,
            capture,
            stop,
            task,
        }
    }

    /// Stops accepting; a stalled export never drains, so it is aborted.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let mut task = self.task;
        if tokio::time::timeout(Duration::from_secs(3), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
    }
}
