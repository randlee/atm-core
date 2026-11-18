//! Read-only auxiliary-route client for the authenticated local runtime.

use std::path::Path;
use std::time::Duration;

use atm_core::api::{HttpRequest, RequestBudget};
use atm_core::error::{AtmError, AtmErrorCode};

use crate::client::{HttpRuntimeClientFailure, HttpRuntimeConnector, LoopbackTcpConnector};

pub(crate) async fn get_json(
    endpoint_record_path: &Path,
    path: String,
    request_timeout: Duration,
) -> Result<Vec<u8>, AtmError> {
    if request_timeout.is_zero() {
        return Err(AtmError::config(
            "loopback HTTP client request timeout must be greater than zero",
        ));
    }
    if !path.starts_with('/') {
        return Err(AtmError::config(
            "loopback HTTP route path must start with '/'",
        ));
    }
    let connector = LoopbackTcpConnector::new(endpoint_record_path)?;
    let deadline = RequestBudget::finite(request_timeout).start();
    let exchange = connector.exchange(
        HttpRequest {
            method: "GET".to_owned(),
            path,
            headers: Vec::new(),
            body: Vec::new(),
        },
        deadline,
    );
    let response = match deadline.budget() {
        atm_core::api::DeadlineBudget::Remaining(remaining) => {
            tokio::time::timeout(remaining, exchange)
                .await
                .map_err(|_| connector.deadline_elapsed().into_atm_error())?
                .map_err(HttpRuntimeClientFailure::into_atm_error)?
        }
        atm_core::api::DeadlineBudget::Unbounded => exchange
            .await
            .map_err(HttpRuntimeClientFailure::into_atm_error)?,
        atm_core::api::DeadlineBudget::Expired => {
            return Err(AtmError::new(
                AtmErrorCode::WaitTimeout,
                "loopback diagnostic request budget elapsed before dispatch",
            ));
        }
    };
    if !response.status().is_success() {
        return Err(AtmError::daemon_unavailable(format!(
            "daemon diagnostic query returned HTTP {}",
            response.status()
        )));
    }
    Ok(response.into_body())
}
