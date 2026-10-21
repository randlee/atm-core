use std::num::NonZeroU16;

use atm_core::api::RequestDeadline;
use atm_core::error::AtmError;
use atm_core::types::HostName;

use super::StorageAndNudgeRouter;

impl StorageAndNudgeRouter {
    pub(super) async fn resolved_peer_port(
        &self,
        host: &HostName,
        deadline: RequestDeadline,
    ) -> Result<NonZeroU16, AtmError> {
        let Some(resolver) = self.peer_port_resolver.clone() else {
            #[cfg(test)]
            {
                return Ok(self.direct_peer_port);
            }
            #[cfg(not(test))]
            {
                return Err(AtmError::peer_config_validation(
                    "outbound peer configuration is unavailable",
                ));
            }
        };
        let host = host.clone();
        self.control_path_sync_bridge
            .run(deadline, move || resolver.enabled_trusted_port(&host))
            .await
    }
}
