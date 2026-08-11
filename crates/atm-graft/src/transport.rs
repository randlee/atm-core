use async_trait::async_trait;
use atm_core::api::{
    ApiRequest, ApiResponse, DaemonApiClient, request_requires_compatibility_verification,
};
use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::protocol::{
    CLI_SCHEMA_VERSION, CompatibilityPreflight, HttpApiVersion, RequestEnvelope, ResponseEnvelope,
};
use atm_daemon_client::{
    DaemonLocalIpcEndpoint, exchange_request as daemon_exchange_request,
    try_connect as daemon_try_connect,
};

pub(crate) use atm_daemon_client::unexpected_response;

use atm_http_runtime::SAME_HOST_REQUEST_DEADLINE;

#[deprecated(
    note = "legacy synchronous IPC adapter; graft request execution uses atm-http-runtime"
)]
#[derive(Debug)]
pub(crate) struct GraftLocalIpcClientTransport {
    endpoint: DaemonLocalIpcEndpoint,
}

impl GraftLocalIpcClientTransport {
    pub(crate) fn new(endpoint: DaemonLocalIpcEndpoint) -> Self {
        Self { endpoint }
    }

    pub(crate) fn probe_connection(&self) -> Result<(), AtmError> {
        daemon_try_connect(&self.endpoint).map(|_| ())
    }

    pub(crate) fn round_trip(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        if request_requires_compatibility_verification(&request) {
            let mut verified = atm_daemon_client::verify_connection_compatibility(
                &self.endpoint,
                CompatibilityPreflight {
                    client_release: atm_daemon_client::ReleaseVersion::current(),
                    cli_schema_version: CLI_SCHEMA_VERSION,
                    http_api_version: HttpApiVersion::current(),
                },
                SAME_HOST_REQUEST_DEADLINE,
            )?;
            return verified.dispatch_write(&self.endpoint, request, SAME_HOST_REQUEST_DEADLINE);
        }
        daemon_exchange_request(&self.endpoint, &request, SAME_HOST_REQUEST_DEADLINE)
    }
}

impl boundary::sealed::Sealed for GraftLocalIpcClientTransport {}

#[async_trait]
impl DaemonApiClient for GraftLocalIpcClientTransport {
    async fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
        let endpoint = self.endpoint.clone();
        let request = request.into_inner();
        tokio::task::spawn_blocking(move || {
            GraftLocalIpcClientTransport { endpoint }
                .round_trip(request)
                .map(ApiResponse::new)
        })
        .await
        .map_err(|source| {
            AtmError::daemon_unavailable("graft daemon request worker ended unexpectedly")
                .with_cause(source)
        })?
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use atm_core::api::DaemonApiClient;
    use atm_core::protocol::RequestEnvelope;
    use atm_core::send::{SendMessageSource, SendRequest};
    use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use atm_core::types::{AgentName, TeamName};
    use tempfile::tempdir;

    use super::GraftLocalIpcClientTransport;

    fn write_request() -> atm_core::api::ApiRequest {
        atm_core::api::ApiRequest::new(RequestEnvelope::Write(Box::new(
            SendRequest::new(
                ".".into(),
                ".".into(),
                AgentName::from_validated(TEST_SENDER),
                TEST_RECIPIENT,
                TeamName::from_validated(TEST_TEAM),
                SendMessageSource::Inline("cancel me".to_owned()),
                None,
                false,
                None,
                false,
            )
            .expect("request"),
        )))
    }

    #[tokio::test]
    async fn cancelling_async_execute_does_not_block_the_tokio_worker() {
        let tempdir = tempdir().expect("tempdir");
        let endpoint = atm_daemon_client::DaemonLocalIpcEndpoint::new(
            tempdir.path().join("unserved-daemon.sock"),
        )
        .expect("endpoint");
        let transport = Arc::new(GraftLocalIpcClientTransport::new(endpoint));
        let task = tokio::spawn(async move { transport.execute(write_request()).await });

        tokio::task::yield_now().await;
        task.abort();
        assert!(task.await.expect_err("cancelled task").is_cancelled());
    }
}
