use crate::{
    DaemonConfigIngress, DaemonInboxExport, DaemonInboxIngress, DaemonNotificationSink,
    DaemonReconcileCoordinator, DaemonRequestDispatcher, DaemonStatusSource, FileWatchEventSource,
    LocalSocketServerTransport,
};
use atm_core::{boundary::ServerTransport, error::AtmError};
use std::error::Error as StdError;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeStartStubError {
    RuntimeStartNotWired,
}

impl fmt::Display for RuntimeStartStubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeStartNotWired => {
                f.write_str("daemon runtime composition start scaffold is not wired")
            }
        }
    }
}

impl StdError for RuntimeStartStubError {}

/// Internal root for Phase R daemon runtime wiring.
#[derive(Debug, Default)]
pub(crate) struct RuntimeComposition {
    server_transport: LocalSocketServerTransport,
    request_dispatcher: DaemonRequestDispatcher,
    notification_sink: DaemonNotificationSink,
    status_source: DaemonStatusSource,
    watch_event_source: FileWatchEventSource,
    reconcile_coordinator: DaemonReconcileCoordinator,
    config_ingress: DaemonConfigIngress,
    inbox_ingress: DaemonInboxIngress,
    inbox_export: DaemonInboxExport,
}

impl RuntimeComposition {
    pub(crate) fn new() -> Self {
        Self {
            server_transport: LocalSocketServerTransport::new(),
            request_dispatcher: DaemonRequestDispatcher::new(),
            notification_sink: DaemonNotificationSink::new(),
            status_source: DaemonStatusSource::new(),
            watch_event_source: FileWatchEventSource::new(),
            reconcile_coordinator: DaemonReconcileCoordinator::new(),
            config_ingress: DaemonConfigIngress::new(),
            inbox_ingress: DaemonInboxIngress::new(),
            inbox_export: DaemonInboxExport::new(),
        }
    }

    pub(crate) fn server_transport(&self) -> &LocalSocketServerTransport {
        &self.server_transport
    }

    pub(crate) fn notification_sink(&self) -> &DaemonNotificationSink {
        &self.notification_sink
    }

    pub(crate) fn request_dispatcher(&self) -> &DaemonRequestDispatcher {
        &self.request_dispatcher
    }

    pub(crate) fn status_source(&self) -> &DaemonStatusSource {
        &self.status_source
    }

    pub(crate) fn watch_event_source(&self) -> &FileWatchEventSource {
        &self.watch_event_source
    }

    pub(crate) fn reconcile_coordinator(&self) -> &DaemonReconcileCoordinator {
        &self.reconcile_coordinator
    }

    pub(crate) fn config_ingress(&self) -> &DaemonConfigIngress {
        &self.config_ingress
    }

    pub(crate) fn inbox_ingress(&self) -> &DaemonInboxIngress {
        &self.inbox_ingress
    }

    pub(crate) fn inbox_export(&self) -> &DaemonInboxExport {
        &self.inbox_export
    }

    pub(crate) fn start(&self) -> Result<(), AtmError> {
        Err(AtmError::observability_bootstrap(
            "daemon runtime start scaffold is not implemented yet",
        )
        .with_recovery(
            "Finish RuntimeComposition startup wiring before invoking the daemon entrypoint.",
        )
        .with_source(RuntimeStartStubError::RuntimeStartNotWired))
    }

    pub(crate) fn serve(&self) -> Result<(), AtmError> {
        self.server_transport.serve(&self.request_dispatcher)
    }
}

pub(crate) fn compose_runtime() -> RuntimeComposition {
    RuntimeComposition::new()
}
