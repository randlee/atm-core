//! Reloadable daemon-owned dependencies for synchronous local admission.

use std::sync::Arc;

use arc_swap::ArcSwap;
use atm_core::send::{
    PreparedWrite, WriteRequest, prepare_peer_writes_atomically, prepare_write_with_runtime,
};
use atm_core::{LocalServiceRuntime, error::AtmError};
use atm_storage::{HostName, PeerAliasKey, PeerDirectory, PeerEndpoint};

use crate::daemon_runtime_observability::DaemonRuntimeObservability;

/// Immutable dependencies read by one local admission.
///
/// The local request path reads this atomically-swapped in-memory view. It
/// must not read a caller workspace or resolve post-send hooks while an IPC
/// response is pending. Reload replaces the whole view as one control-plane
/// operation; it is deliberately not another persistence store.
#[derive(Clone)]
pub(crate) struct AdmissionRuntimeView {
    state: Arc<ArcSwap<AdmissionRuntimeViewState>>,
}

#[derive(Clone)]
struct AdmissionRuntimeViewState {
    runtime: LocalServiceRuntime,
    peer_directory: PeerDirectory,
}

impl AdmissionRuntimeView {
    pub(crate) fn new(runtime: LocalServiceRuntime, peer_directory: PeerDirectory) -> Self {
        Self {
            state: Arc::new(ArcSwap::from_pointee(AdmissionRuntimeViewState {
                runtime,
                peer_directory,
            })),
        }
    }

    pub(crate) fn prepare_write(
        &self,
        mut request: WriteRequest,
        observability: &dyn DaemonRuntimeObservability,
    ) -> Result<PreparedWrite, AtmError> {
        let view = self.state.load();
        if let Some(address) = request.to.as_ref()
            && let Some(host) = address.host()
        {
            let endpoint = view
                .peer_directory
                .normalize(&PeerAliasKey::parse(host.as_str())?)?;
            request.to = Some(address.with_host(endpoint.canonical_host)?);
        }
        prepare_write_with_runtime(request, observability, &view.runtime)
    }

    pub(crate) fn prepare_peer_writes_atomically(
        &self,
        mut requests: Vec<WriteRequest>,
        observability: &dyn DaemonRuntimeObservability,
    ) -> Result<Vec<PreparedWrite>, AtmError> {
        let view = self.state.load();
        for request in &mut requests {
            if let Some(address) = request.to.as_ref()
                && let Some(host) = address.host()
            {
                let endpoint = view
                    .peer_directory
                    .normalize(&PeerAliasKey::parse(host.as_str())?)?;
                request.to = Some(address.with_host(endpoint.canonical_host)?);
            }
        }
        prepare_peer_writes_atomically(requests, observability, &view.runtime)
    }

    pub(crate) fn reload(&self, runtime: LocalServiceRuntime, peer_directory: PeerDirectory) {
        runtime.clear_roster_cache();
        self.state.store(Arc::new(AdmissionRuntimeViewState {
            runtime,
            peer_directory,
        }));
    }

    /// Resolves one already-canonical host from the immutable configuration
    /// snapshot published with local admission. This is an in-memory lookup,
    /// never a per-send storage read or hostname-resolution step.
    pub(crate) fn endpoint_for_canonical_host(&self, host: &HostName) -> Option<PeerEndpoint> {
        self.state
            .load()
            .peer_directory
            .endpoint_for_canonical_host(host)
    }
}
