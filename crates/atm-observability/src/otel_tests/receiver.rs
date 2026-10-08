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
    pub metadata: Arc<Mutex<Vec<tonic::metadata::MetadataMap>>>,
    pub resources: Arc<Mutex<Vec<opentelemetry_proto::tonic::resource::v1::Resource>>>,
    pub started: Arc<AtomicUsize>,
    pub finished: Arc<AtomicUsize>,
    pub changed: Arc<Notify>,
    pub stall: bool,
}

impl Capture {
    async fn response(&self) {
        self.started.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_waiters();
        struct Finished(Capture);
        impl Drop for Finished {
            fn drop(&mut self) {
                self.0.finished.fetch_add(1, Ordering::SeqCst);
                self.0.changed.notify_waiters();
            }
        }
        let _finished = Finished(self.clone());
        if self.stall {
            std::future::pending::<()>().await;
        }
    }

    pub async fn wait(&self, condition: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let changed = self.changed.notified();
                if condition() {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("receiver condition before deadline");
    }
}

#[tonic::async_trait]
impl traces::trace_service_server::TraceService for Capture {
    async fn export(
        &self,
        request: Request<traces::ExportTraceServiceRequest>,
    ) -> Result<Response<traces::ExportTraceServiceResponse>, Status> {
        self.resources.lock().unwrap().extend(
            request
                .get_ref()
                .resource_spans
                .iter()
                .filter_map(|resource| resource.resource.clone()),
        );
        self.metadata
            .lock()
            .unwrap()
            .push(request.metadata().clone());
        self.spans.lock().unwrap().extend(
            request
                .into_inner()
                .resource_spans
                .into_iter()
                .flat_map(|resource| resource.scope_spans)
                .flat_map(|scope| scope.spans),
        );
        self.response().await;
        Ok(Response::new(traces::ExportTraceServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl logs::logs_service_server::LogsService for Capture {
    async fn export(
        &self,
        request: Request<logs::ExportLogsServiceRequest>,
    ) -> Result<Response<logs::ExportLogsServiceResponse>, Status> {
        self.metadata
            .lock()
            .unwrap()
            .push(request.metadata().clone());
        self.logs.lock().unwrap().extend(
            request
                .into_inner()
                .resource_logs
                .into_iter()
                .flat_map(|resource| resource.scope_logs)
                .flat_map(|scope| scope.log_records),
        );
        self.response().await;
        Ok(Response::new(logs::ExportLogsServiceResponse::default()))
    }
}

#[tonic::async_trait]
impl metrics::metrics_service_server::MetricsService for Capture {
    async fn export(
        &self,
        request: Request<metrics::ExportMetricsServiceRequest>,
    ) -> Result<Response<metrics::ExportMetricsServiceResponse>, Status> {
        self.metadata
            .lock()
            .unwrap()
            .push(request.metadata().clone());
        self.metrics.lock().unwrap().extend(
            request
                .into_inner()
                .resource_metrics
                .into_iter()
                .flat_map(|resource| resource.scope_metrics)
                .flat_map(|scope| scope.metrics),
        );
        self.response().await;
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
        let task = tokio::spawn(
            tonic::transport::Server::builder()
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
                }),
        );
        let task = tokio::spawn(async move {
            task.await.unwrap().unwrap();
        });
        Self {
            endpoint,
            capture,
            stop,
            task,
        }
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(3), self.task)
            .await
            .unwrap()
            .unwrap();
    }
}
