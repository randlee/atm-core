use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::boundary::{self, RequestDispatcher};
use atm_core::error::AtmError;

#[derive(Debug)]
pub(crate) struct PreparedRuntimeServer;

impl PreparedRuntimeServer {
    pub(crate) fn serve_with_runtime_hooks<BeginShutdown, ReloadRuntimeView, FinalizeShutdown>(
        self,
        _dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
        _graceful_drain_deadline: Duration,
        _force_cancel_deadline: Duration,
        _begin_shutdown: BeginShutdown,
        _reload_runtime_view: ReloadRuntimeView,
        _finalize_shutdown: FinalizeShutdown,
    ) -> Result<(), AtmError>
    where
        BeginShutdown: Fn() -> Result<(), AtmError>,
        ReloadRuntimeView: Fn() -> Result<(), AtmError>,
        FinalizeShutdown: Fn(),
    {
        Err(unsupported_local_ipc_error())
    }
}

#[derive(Debug, Default)]
pub(crate) struct LocalIpcServerTransportAdapter;

impl LocalIpcServerTransportAdapter {
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) fn prepare_runtime(&self) -> Result<PreparedRuntimeServer, AtmError> {
        Err(unsupported_local_ipc_error())
    }

    pub(crate) fn prepare_runtime_at_socket_path(
        &self,
        _endpoint_path: PathBuf,
    ) -> Result<PreparedRuntimeServer, AtmError> {
        Err(unsupported_local_ipc_error())
    }
}

impl boundary::sealed::Sealed for LocalIpcServerTransportAdapter {}

impl boundary::ServerTransport for LocalIpcServerTransportAdapter {
    fn serve(
        &self,
        _dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<(), AtmError> {
        Err(unsupported_local_ipc_error())
    }
}

fn unsupported_local_ipc_error() -> AtmError {
    AtmError::daemon_unavailable(
        "daemon same-host local IPC runtime is not available on Windows in Phase S.1",
    )
    .with_recovery(
        "Run the daemon on a Unix host for same-host local IPC tests, or use the Windows-target branch where the dedicated runtime transport is implemented.",
    )
}
