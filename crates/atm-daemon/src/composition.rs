use crate::{
    DaemonNotificationSink, DaemonReconcileCoordinator, DaemonStatusSource, FileWatchEventSource,
    LocalSocketServerTransport,
};

/// Internal root for Phase R daemon runtime wiring.
#[derive(Debug, Default)]
pub(crate) struct RuntimeComposition {
    server_transport: LocalSocketServerTransport,
    notification_sink: DaemonNotificationSink,
    status_source: DaemonStatusSource,
    watch_event_source: FileWatchEventSource,
    reconcile_coordinator: DaemonReconcileCoordinator,
}

impl RuntimeComposition {
    pub(crate) fn new() -> Self {
        Self {
            server_transport: LocalSocketServerTransport::new(),
            notification_sink: DaemonNotificationSink::new(),
            status_source: DaemonStatusSource::new(),
            watch_event_source: FileWatchEventSource::new(),
            reconcile_coordinator: DaemonReconcileCoordinator::new(),
        }
    }

    pub(crate) fn server_transport(&self) -> &LocalSocketServerTransport {
        &self.server_transport
    }

    pub(crate) fn notification_sink(&self) -> &DaemonNotificationSink {
        &self.notification_sink
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

    pub(crate) fn start(&self) {
        unimplemented!("Phase R daemon runtime wiring is not implemented yet");
    }
}

pub(crate) fn compose_runtime() -> RuntimeComposition {
    RuntimeComposition::new()
}
