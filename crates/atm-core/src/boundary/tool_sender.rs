//! Core-owned authority wrapper for a registry-admitted virtual sender.
//!
//! The registry value is inspectable state.  This capability is deliberately
//! neither serializable nor publicly constructible; daemon composition mints
//! it only after registry admission.

use atm_storage::{AgentAddress, RegisteredToolSender};

use crate::caller_context::CallerContext;

#[derive(Debug)]
pub struct ToolSenderCapability {
    _private: (),
}

impl ToolSenderCapability {
    #[allow(dead_code)]
    pub(crate) fn admitted_by_daemon() -> Self {
        Self { _private: () }
    }
}

#[derive(Debug)]
pub enum SenderClaim {
    Ordinary {
        address: AgentAddress,
        caller: CallerContext,
    },
    DaemonTool {
        sender: RegisteredToolSender,
        proof: ToolSenderCapability,
    },
}
