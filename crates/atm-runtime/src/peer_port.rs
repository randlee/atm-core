use std::num::NonZeroU16;
use std::sync::Arc;

use atm_core::error::AtmError;
use atm_core::types::HostName;
use atm_storage::PeerConfigStore;

/// Exact trusted-peer port lookup behind the runtime's storage boundary.
#[derive(Clone)]
pub struct PeerPortResolver {
    store: Arc<dyn PeerConfigStore + Send + Sync>,
}

impl PeerPortResolver {
    pub(crate) fn new(store: Arc<dyn PeerConfigStore + Send + Sync>) -> Self {
        Self { store }
    }

    pub fn enabled_trusted_port(&self, host: &HostName) -> Result<NonZeroU16, AtmError> {
        self.store
            .trusted_peer(host)?
            .filter(|peer| peer.enabled)
            .map(|peer| peer.https_port)
            .ok_or_else(|| {
                AtmError::peer_config_validation(format!(
                    "no enabled trusted peer configured for {host}"
                ))
            })
    }
}
