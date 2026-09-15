use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::service_runtime::LocalServiceRuntime;
use atm_storage::GraftEndpointStoreError;

/// Invoke a closure with the installed retained local runtime.
#[doc(hidden)]
pub fn with_default_local_service_runtime<T>(
    f: impl FnOnce(&LocalServiceRuntime) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    let runtime = crate::service_runtime_store::default_runtime()?;
    f(&runtime)
}

/// Maps graft storage failures to the canonical ATM error contract.
pub fn graft_store_error(error: GraftEndpointStoreError) -> AtmError {
    match error {
        GraftEndpointStoreError::NotOwner => AtmError::new(
            AtmErrorCode::GraftReceiverNotOwner,
            "graft receiver lease is owned by another generation",
        ),
        GraftEndpointStoreError::Absent => AtmError::new(
            AtmErrorCode::GraftReceiverNotRegistered,
            "graft receiver lease is absent; re-announcement required",
        ),
        GraftEndpointStoreError::AlreadyActive => {
            AtmError::validation("graft receiver lease is already active")
        }
        GraftEndpointStoreError::Storage {
            code,
            message,
            cause,
        } => {
            let error = AtmError::new(code, message);
            match cause {
                Some(cause) => error.with_cause(cause),
                None => error,
            }
        }
    }
}
