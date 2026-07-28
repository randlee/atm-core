//! Reloadable daemon-owned dependencies for synchronous local admission.

use std::sync::Arc;

use arc_swap::ArcSwap;
use atm_core::send::{PreparedWrite, WriteRequest, prepare_write_with_runtime};
use atm_core::{LocalServiceRuntime, error::AtmError};

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
}

impl AdmissionRuntimeView {
    pub(crate) fn new(runtime: LocalServiceRuntime) -> Self {
        Self {
            state: Arc::new(ArcSwap::from_pointee(AdmissionRuntimeViewState { runtime })),
        }
    }

    pub(crate) fn prepare_write(
        &self,
        request: WriteRequest,
        observability: &dyn DaemonRuntimeObservability,
    ) -> Result<PreparedWrite, AtmError> {
        let view = self.state.load();
        prepare_write_with_runtime(request, observability, &view.runtime)
    }

    pub(crate) fn reload(&self, runtime: LocalServiceRuntime) {
        self.state
            .store(Arc::new(AdmissionRuntimeViewState { runtime }));
    }
}
