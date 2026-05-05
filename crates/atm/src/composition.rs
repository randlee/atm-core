#![allow(dead_code)]

use atm_core::boundary::ClientTransport;
use atm_core::error::AtmError;
use atm_core::observability::{NullObservability, ObservabilityPort};
use std::error::Error as StdError;
use std::fmt;

#[derive(Debug, Default)]
pub(crate) struct SendCommandEntryPoint;

impl SendCommandEntryPoint {
    pub(crate) const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Default)]
pub(crate) struct ReceiveCommandEntryPoint;

impl ReceiveCommandEntryPoint {
    pub(crate) const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliBootstrapStubError {
    BootstrapNotWired,
}

impl fmt::Display for CliBootstrapStubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BootstrapNotWired => {
                f.write_str("CLI composition bootstrap scaffold is not wired")
            }
        }
    }
}

impl StdError for CliBootstrapStubError {}

pub(crate) struct CliComposition {
    transport: Box<dyn ClientTransport>,
    observability_port: Box<dyn ObservabilityPort + Send + Sync>,
    send_command: SendCommandEntryPoint,
    receive_command: ReceiveCommandEntryPoint,
}

impl CliComposition {
    pub(crate) fn from_transport(transport: Box<dyn ClientTransport>) -> Self {
        Self {
            transport,
            observability_port: Box::new(NullObservability),
            send_command: SendCommandEntryPoint::new(),
            receive_command: ReceiveCommandEntryPoint::new(),
        }
    }

    pub(crate) fn transport(&self) -> &dyn ClientTransport {
        self.transport.as_ref()
    }

    pub(crate) fn observability_port(&self) -> &(dyn ObservabilityPort + Send + Sync) {
        self.observability_port.as_ref()
    }

    pub(crate) fn send_command(&self) -> &SendCommandEntryPoint {
        &self.send_command
    }

    pub(crate) fn receive_command(&self) -> &ReceiveCommandEntryPoint {
        &self.receive_command
    }

    pub(crate) fn bootstrap() -> Result<Self, AtmError> {
        Err(AtmError::observability_bootstrap(
            "CLI composition bootstrap scaffold is not implemented yet",
        )
        .with_recovery("Wire the CLI transport and command entry points before using bootstrap().")
        .with_source(CliBootstrapStubError::BootstrapNotWired))
    }
}
