#![allow(dead_code)]

use atm_core::ack::{AckOutcome, AckRequest};
use atm_core::boundary;
use atm_core::boundary::ClientTransport;
use atm_core::clear::{ClearOutcome, ClearQuery};
use atm_core::doctor::{DoctorQuery, DoctorReport};
use atm_core::error::AtmError;
use atm_core::observability::ObservabilityPort;
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendOutcome, SendRequest};
use std::fmt;

use crate::observability::CliObservability;

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

struct LocalSocketClientTransport<'a> {
    observability: &'a (dyn ObservabilityPort + Send + Sync),
}

impl<'a> LocalSocketClientTransport<'a> {
    fn new(observability: &'a (dyn ObservabilityPort + Send + Sync)) -> Self {
        Self { observability }
    }
}

impl boundary::sealed::Sealed for LocalSocketClientTransport<'_> {}

impl ClientTransport for LocalSocketClientTransport<'_> {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Send(SendRequestEnvelope::Compose(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                    atm_core::send::send_mail(request, self.observability)?,
                )))
            }
            RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(request)) => {
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(
                    atm_core::ack::ack_mail(request, self.observability)?,
                )))
            }
            RequestEnvelope::Receive(query) => Ok(ResponseEnvelope::Receive(
                atm_core::read::read_mail(query, self.observability)?,
            )),
            RequestEnvelope::Clear(query) => Ok(ResponseEnvelope::Clear(
                atm_core::clear::clear_mail(query, self.observability)?,
            )),
            RequestEnvelope::Doctor(query) => Ok(ResponseEnvelope::Doctor(
                atm_core::doctor::run_doctor(query, self.observability)?,
            )),
        }
    }
}

pub(crate) struct CliComposition<'a> {
    transport: Box<dyn ClientTransport + 'a>,
    observability_port: &'a (dyn ObservabilityPort + Send + Sync),
    send_command: SendCommandEntryPoint,
    receive_command: ReceiveCommandEntryPoint,
}

impl fmt::Debug for CliComposition<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CliComposition")
            .field("transport", &"dyn ClientTransport")
            .field("observability_port", &"dyn ObservabilityPort")
            .field("send_command", &self.send_command)
            .field("receive_command", &self.receive_command)
            .finish()
    }
}

impl<'a> CliComposition<'a> {
    pub(crate) fn from_transport(
        transport: Box<dyn ClientTransport + 'a>,
        observability_port: &'a (dyn ObservabilityPort + Send + Sync),
    ) -> Self {
        Self {
            transport,
            observability_port,
            send_command: SendCommandEntryPoint::new(),
            receive_command: ReceiveCommandEntryPoint::new(),
        }
    }

    pub(crate) fn transport(&self) -> &(dyn ClientTransport + 'a) {
        self.transport.as_ref()
    }

    pub(crate) fn send_request(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        self.transport.send(request)
    }

    pub(crate) fn observability_port(&self) -> &(dyn ObservabilityPort + Send + Sync) {
        self.observability_port
    }

    pub(crate) fn send_command(&self) -> &SendCommandEntryPoint {
        &self.send_command
    }

    pub(crate) fn receive_command(&self) -> &ReceiveCommandEntryPoint {
        &self.receive_command
    }

    pub(crate) fn send(&self, request: SendRequest) -> Result<SendOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))? {
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => Ok(outcome),
            other => Err(unexpected_response("send", other)),
        }
    }

    pub(crate) fn ack(&self, request: AckRequest) -> Result<AckOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            request,
        )))? {
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => Ok(outcome),
            other => Err(unexpected_response("ack", other)),
        }
    }

    pub(crate) fn receive(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Receive(query))? {
            ResponseEnvelope::Receive(outcome) => Ok(outcome),
            other => Err(unexpected_response("receive", other)),
        }
    }

    pub(crate) fn clear(&self, query: ClearQuery) -> Result<ClearOutcome, AtmError> {
        match self.send_request(RequestEnvelope::Clear(query))? {
            ResponseEnvelope::Clear(outcome) => Ok(outcome),
            other => Err(unexpected_response("clear", other)),
        }
    }

    pub(crate) fn doctor(&self, query: DoctorQuery) -> Result<DoctorReport, AtmError> {
        match self.send_request(RequestEnvelope::Doctor(query))? {
            ResponseEnvelope::Doctor(report) => Ok(report),
            other => Err(unexpected_response("doctor", other)),
        }
    }

    pub(crate) fn bootstrap(observability: &'a CliObservability) -> Result<Self, AtmError> {
        Ok(Self::from_transport(
            Box::new(LocalSocketClientTransport::new(observability)),
            observability,
        ))
    }
}

fn unexpected_response(command: &str, response: ResponseEnvelope) -> AtmError {
    AtmError::validation(format!(
        "transport returned an unexpected response for `{command}`: {response:?}"
    ))
}
