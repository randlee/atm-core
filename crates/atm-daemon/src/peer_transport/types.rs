use std::sync::mpsc;
use std::thread::JoinHandle;

use super::PeerTransportRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReplayResumeSummary {
    pub(crate) delivered: usize,
    pub(crate) retained: usize,
    pub(crate) purged_expired: usize,
    pub(crate) receipt_updates: usize,
}

#[derive(Debug)]
pub(crate) struct ReplayResumeWorkerHandle {
    pub(crate) stop_tx: mpsc::Sender<()>,
    pub(crate) join_handle: JoinHandle<()>,
}

impl Default for PeerTransportRuntime {
    fn default() -> Self {
        Self::new(None)
    }
}
