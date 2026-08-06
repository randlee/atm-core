//! Connector-neutral client-side HTTP translation.
//!
//! This module owns no socket, DNS, TLS, or listener setup.  Physical
//! connectors are deliberately introduced by AL.5--AL.7.  It owns the one
//! route-body encoder and result decoder used after a connector is selected.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use atm_core::api::{
    ApiRequest, ApiResponse, DaemonApiClient, HttpRequest, RequestDeadline, decode_http_response,
    encode_http_request,
};
use atm_core::boundary;
use atm_core::error::{AtmError, AtmErrorCode};

/// Physical connection setup for the one shared HTTP client.
///
/// Implementations own only endpoint/DNS resolution, connection, TLS, request
/// write, and response read. They must preserve the supplied absolute deadline
/// and return the existing [`AtmError`] vocabulary with their concrete cause.
/// This is a connector seam, not a second ATM client boundary.
#[allow(
    dead_code,
    reason = "AL.4 defines the shared client before AL.5--AL.7 add physical connectors"
)]
#[async_trait]
pub(crate) trait HttpRuntimeConnector: Send + Sync {
    async fn exchange(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, AtmError>;
}

/// One framework-backed client operation for every physical adapter.
///
/// The connector is the only place UDS, loopback, or TLS can differ. Request
/// encoding, route selection, response decoding, and outcome mapping remain
/// in this type.
#[allow(
    dead_code,
    reason = "AL.4 defines the shared client before AL.5--AL.7 construct physical connectors"
)]
#[derive(Debug, Clone)]
pub(crate) struct HttpRuntimeClient<Connector> {
    connector: Arc<Connector>,
    request_timeout: Duration,
}

impl<Connector> HttpRuntimeClient<Connector> {
    #[allow(
        dead_code,
        reason = "AL.5--AL.7 own construction from physical connector configuration"
    )]
    #[must_use]
    pub(crate) fn new(connector: Arc<Connector>, request_timeout: Duration) -> Self {
        Self {
            connector,
            request_timeout,
        }
    }

    #[allow(
        dead_code,
        reason = "AL.5--AL.7 own construction from physical connector configuration"
    )]
    async fn execute_with_deadline(
        &self,
        request: ApiRequest,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError>
    where
        Connector: HttpRuntimeConnector,
    {
        let request = request.into_inner();
        let encoded = encode_http_request(&request, &[])?;
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::new(
                AtmErrorCode::WaitTimeout,
                "HTTP client request budget elapsed before connector dispatch",
            )
        })?;
        let response = tokio::time::timeout(remaining, self.connector.exchange(encoded, deadline))
            .await
            .map_err(|_| {
                AtmError::new(
                    AtmErrorCode::WaitTimeout,
                    "HTTP client request exceeded its absolute request budget",
                )
            })??;
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                format!(
                    "{name}: {}",
                    value.to_str().unwrap_or("<non-UTF-8 header value>")
                )
            })
            .collect::<Vec<_>>();
        decode_http_response(
            &request,
            response.status().as_u16(),
            &headers,
            response.body(),
        )
        .map(ApiResponse::new)
    }
}

impl<Connector> boundary::sealed::Sealed for HttpRuntimeClient<Connector> where
    Connector: Send + Sync
{
}

#[async_trait]
impl<Connector> DaemonApiClient for HttpRuntimeClient<Connector>
where
    Connector: HttpRuntimeConnector + 'static,
{
    async fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
        self.execute_with_deadline(request, RequestDeadline::after(self.request_timeout))
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use atm_core::api::{ApiRequest, DaemonApiClient, HttpRequest, RequestDeadline};
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendRequest};
    use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use atm_core::types::{AgentName, CommandAction, TeamName};

    use super::{HttpRuntimeClient, HttpRuntimeConnector};

    #[derive(Default)]
    struct RecordingConnector {
        requests: Mutex<Vec<HttpRequest>>,
        responses: Mutex<VecDeque<Result<axum::http::Response<Vec<u8>>, AtmError>>>,
    }

    #[async_trait]
    impl HttpRuntimeConnector for RecordingConnector {
        async fn exchange(
            &self,
            request: HttpRequest,
            _deadline: RequestDeadline,
        ) -> Result<axum::http::Response<Vec<u8>>, AtmError> {
            self.requests.lock().expect("requests").push(request);
            self.responses
                .lock()
                .expect("responses")
                .pop_front()
                .expect("configured response")
        }
    }

    fn write_request() -> RequestEnvelope {
        RequestEnvelope::Write(Box::new(
            SendRequest::new(
                ".".into(),
                ".".into(),
                AgentName::from_validated(TEST_SENDER),
                TEST_RECIPIENT,
                TeamName::from_validated(TEST_TEAM),
                SendMessageSource::Inline("shared client".to_owned()),
                None,
                false,
                None,
                false,
            )
            .expect("request"),
        ))
    }

    fn sent_response() -> axum::http::Response<Vec<u8>> {
        let outcome = atm_core::send::SendOutcome {
            action: CommandAction::Send,
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_RECIPIENT),
            sender: AgentName::from_validated(TEST_SENDER),
            outcome: SendCommandOutcome::Sent,
            message_id: atm_core::schema::AtmMessageId::new(),
            requires_ack: false,
            task_id: None,
            summary: None,
            message: None,
            warnings: Vec::new(),
            dry_run: false,
        };
        axum::http::Response::builder()
            .status(201)
            .body(serde_json::to_vec(&outcome).expect("encode outcome"))
            .expect("response")
    }

    #[tokio::test]
    async fn future_uds_loopback_and_tls_connectors_share_one_write_translation() {
        for connector_kind in ["uds", "loopback", "tls"] {
            let connector = Arc::new(RecordingConnector::default());
            connector
                .responses
                .lock()
                .expect("responses")
                .push_back(Ok(sent_response()));
            let client = HttpRuntimeClient::new(connector.clone(), Duration::from_secs(1));

            let response = client
                .execute(ApiRequest::new(write_request()))
                .await
                .expect("shared client response");

            assert!(matches!(
                response.into_inner(),
                ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
            ));
            let requests = connector.requests.lock().expect("requests");
            assert_eq!(requests.len(), 1, "{connector_kind} connector");
            assert_eq!(requests[0].method, "POST", "{connector_kind} connector");
            assert_eq!(
                requests[0].path, "/v1/atm/messages",
                "{connector_kind} connector"
            );
            assert!(
                serde_json::from_slice::<atm_core::send::WriteRequest>(&requests[0].body).is_ok(),
                "{connector_kind} connector"
            );
        }
    }

    #[tokio::test]
    async fn response_status_preserves_the_adr_032_error_body() {
        let connector = Arc::new(RecordingConnector::default());
        let expected = AtmError::validation("rejected by server");
        connector.responses.lock().expect("responses").push_back(Ok(
            axum::http::Response::builder()
                .status(400)
                .body(serde_json::to_vec(&expected).expect("encode error"))
                .expect("response"),
        ));
        let client = HttpRuntimeClient::new(connector, Duration::from_secs(1));

        let response = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect("typed error is a response");

        assert!(
            matches!(response.into_inner(), ResponseEnvelope::Error(error) if error == expected)
        );
    }

    #[tokio::test]
    async fn malformed_response_body_uses_the_existing_protocol_error() {
        let connector = Arc::new(RecordingConnector::default());
        connector.responses.lock().expect("responses").push_back(Ok(
            axum::http::Response::builder()
                .status(201)
                .body(b"not a response envelope".to_vec())
                .expect("response"),
        ));
        let client = HttpRuntimeClient::new(connector, Duration::from_secs(1));

        let error = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect_err("malformed response must not be accepted");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
    }

    #[tokio::test]
    async fn every_connector_failure_cause_preserves_its_typed_context() {
        for cause in [
            "endpoint/DNS resolution",
            "connect/refusal/network reachability",
            "TLS handshake/hostname/mTLS authorization",
            "request write",
            "cancellation",
            "runtime shutdown",
        ] {
            let connector = Arc::new(RecordingConnector::default());
            let expected = AtmError::new(
                AtmErrorCode::DaemonUnavailable,
                format!("{cause} failed while using the configured connector"),
            );
            connector
                .responses
                .lock()
                .expect("responses")
                .push_back(Err(expected.clone()));
            let client = HttpRuntimeClient::new(connector, Duration::from_secs(1));

            let error = client
                .execute(ApiRequest::new(write_request()))
                .await
                .expect_err(cause);

            assert_eq!(error, expected, "{cause}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn request_deadline_bounds_the_connector_once() {
        struct WaitingConnector;
        #[async_trait]
        impl HttpRuntimeConnector for WaitingConnector {
            async fn exchange(
                &self,
                _request: HttpRequest,
                _deadline: RequestDeadline,
            ) -> Result<axum::http::Response<Vec<u8>>, AtmError> {
                tokio::time::sleep(Duration::from_secs(60)).await;
                unreachable!("deadline must cancel the connector")
            }
        }

        let client = HttpRuntimeClient::new(Arc::new(WaitingConnector), Duration::from_millis(1));
        let task =
            tokio::spawn(async move { client.execute(ApiRequest::new(write_request())).await });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(1)).await;
        let error = task
            .await
            .expect("timeout task completes")
            .expect_err("request must time out");
        assert_eq!(error.code(), AtmErrorCode::WaitTimeout);
    }
}
