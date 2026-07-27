use atm_core::api::{ApiRequest, ApiResponse, DaemonApiClient};
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

use crate::SAME_HOST_REQUEST_DEADLINE;

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

fn request_requires_compatibility_verification(request: &RequestEnvelope) -> bool {
    matches!(
        request,
        RequestEnvelope::Write(_) | RequestEnvelope::Clear(_)
    )
}

impl boundary::sealed::Sealed for GraftLocalIpcClientTransport {}

impl DaemonApiClient for GraftLocalIpcClientTransport {
    fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
        self.round_trip(request.into_inner()).map(ApiResponse::new)
    }
}
