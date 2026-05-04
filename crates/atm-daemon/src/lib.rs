#![forbid(unsafe_code)]
#![allow(dead_code)]

//! Skeleton crate for Phase R daemon runtime work.

pub(crate) mod composition;

use atm_core::boundary;

/// Placeholder runtime transport for the daemon server boundary.
#[derive(Debug, Default)]
pub(crate) struct LocalSocketServerTransport;

impl LocalSocketServerTransport {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for LocalSocketServerTransport {}

impl boundary::ServerTransport for LocalSocketServerTransport {}

/// Placeholder runtime sink for daemon-emitted notifications.
#[derive(Debug, Default)]
pub(crate) struct DaemonNotificationSink;

impl DaemonNotificationSink {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonNotificationSink {}

impl boundary::NotificationSink for DaemonNotificationSink {}

/// Placeholder runtime source for daemon status snapshots.
#[derive(Debug, Default)]
pub(crate) struct DaemonStatusSource;

impl DaemonStatusSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonStatusSource {}

impl boundary::StatusSource for DaemonStatusSource {}

/// Placeholder runtime source for daemon watch events.
#[derive(Debug, Default)]
pub(crate) struct FileWatchEventSource;

impl FileWatchEventSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for FileWatchEventSource {}

impl boundary::WatchEventSource for FileWatchEventSource {}

/// Placeholder runtime coordinator for daemon reconcile work.
#[derive(Debug, Default)]
pub(crate) struct DaemonReconcileCoordinator;

impl DaemonReconcileCoordinator {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl boundary::sealed::Sealed for DaemonReconcileCoordinator {}

impl boundary::ReconcileCoordinator for DaemonReconcileCoordinator {}
