use atm_storage::contract::sealed;
use atm_storage::{AgentAddress, RegisteredToolSenderIssuer, ToolPrincipal};

pub struct InspectionIssuer {
    principal: ToolPrincipal,
    address: AgentAddress,
}

impl InspectionIssuer {
    pub fn cron() -> Self {
        Self {
            principal: ToolPrincipal::Cron,
            address: "cron@tool".parse().expect("fixed valid tool address"),
        }
    }
}

impl sealed::Sealed for InspectionIssuer {}

impl RegisteredToolSenderIssuer for InspectionIssuer {
    fn principal(&self) -> &ToolPrincipal {
        &self.principal
    }

    fn address(&self) -> &AgentAddress {
        &self.address
    }
}
