#![allow(dead_code)]

use atm_core::boundary::ClientTransport;

pub(crate) struct CliComposition;

impl CliComposition {
    pub(crate) fn from_transport(_transport: Box<dyn ClientTransport>) -> Self {
        Self
    }

    pub(crate) fn bootstrap() -> Self {
        todo!("Phase R client composition wiring lands in a follow-on sprint")
    }
}
