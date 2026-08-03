use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use atm_core::error::AtmError;
use interprocess::local_socket::Stream as LocalSocketStream;

use crate::SubsystemObservability;
use crate::active_connection_registry::ActiveConnectionRegistry;
use crate::daemon_worker_join::{
    CompletionTrackedJoinHandle, JoinTimeoutPolicy, LOCAL_WORKER_JOIN_DEADLINE, join_with_timeout,
};
use crate::lifecycle_control::LifecycleControlSourceAdapter;
use crate::local_admission::{BOUNDED_ADMISSION_RETRY_INTERVAL, send_with_bounded_admission};
use crate::request_worker::{DispatchWorkerPool, handle_connection};
use crate::shutdown_beacon::ShutdownBeacon;

use super::MAX_CONCURRENT_CONNECTIONS;

const CONNECTION_ADMISSION_QUEUE: usize = MAX_CONCURRENT_CONNECTIONS;
const LOCAL_IPC_WORKER_JOIN_POLICY: JoinTimeoutPolicy = JoinTimeoutPolicy {
    subsystem: "local_ipc_transport",
    worker_kind: "local IPC connection worker",
    panic_message: "local IPC connection worker panicked during shutdown",
    timeout_message: "local IPC connection worker exceeded the shutdown join deadline",
};

const CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE: &str =
    "daemon local IPC connection worker panicked; transport thread recovered";

/// Fixed same-host connection workers with one bounded admission queue.
///
/// The queue absorbs a short accept burst without serializing every handoff;
/// when it is full, the listener retries with lifecycle checks and the OS
/// listener backlog supplies further backpressure.
pub(super) struct ConnectionWorkerPool {
    sender: std::sync::mpsc::SyncSender<LocalSocketStream>,
    workers: Mutex<Vec<CompletionTrackedJoinHandle<()>>>,
    #[cfg(test)]
    saturated_admission_signal: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
    #[cfg(test)]
    shutdown_join_signal: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
}

impl ConnectionWorkerPool {
    pub(super) fn start(
        force_shutdown: Arc<AtomicBool>,
        registry: Arc<ActiveConnectionRegistry>,
        observability: SubsystemObservability,
        dispatch_workers: Arc<DispatchWorkerPool>,
    ) -> Result<Self, AtmError> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(CONNECTION_ADMISSION_QUEUE);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(MAX_CONCURRENT_CONNECTIONS);
        for worker_index in 0..MAX_CONCURRENT_CONNECTIONS {
            let receiver = Arc::clone(&receiver);
            let force_shutdown = Arc::clone(&force_shutdown);
            let registry = Arc::clone(&registry);
            let observability = observability.clone();
            let dispatch_workers = Arc::clone(&dispatch_workers);
            let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name(format!("local-ipc-connection-{worker_index}"))
                .spawn(move || {
                    let _completion_tx = completion_tx;
                    loop {
                        let stream = match receiver.lock() {
                            Ok(receiver) => receiver.recv(),
                            Err(_) => return,
                        };
                        let Ok(stream) = stream else {
                            return;
                        };
                        let _active = registry.register();
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            handle_connection(
                                stream,
                                force_shutdown.as_ref(),
                                dispatch_workers.as_ref(),
                                &observability,
                            )
                        }));
                        match result {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => tracing::warn!(
                                subsystem = "local_ipc_transport",
                                action = "connection_worker",
                                outcome = "classified_failure",
                                %error,
                                "daemon local IPC connection handling failed"
                            ),
                            Err(_) => observability.emit_or_warn(
                                "connection_worker",
                                "panic",
                                CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE,
                            ),
                        }
                    }
                })
                .map_err(|source| {
                    AtmError::daemon_unavailable_with_cause(
                        "failed to start local IPC connection worker",
                        source,
                    )
                })?;
            workers.push(CompletionTrackedJoinHandle {
                completion_rx,
                join_handle: worker,
            });
        }
        Ok(Self {
            sender,
            workers: Mutex::new(workers),
            #[cfg(test)]
            saturated_admission_signal: Mutex::new(None),
            #[cfg(test)]
            shutdown_join_signal: Mutex::new(None),
        })
    }

    pub(super) fn dispatch(
        &self,
        stream: LocalSocketStream,
        lifecycle_control: &LifecycleControlSourceAdapter,
        shutdown_beacon: &ShutdownBeacon,
    ) -> Result<(), AtmError> {
        Self::ensure_admission_open(lifecycle_control, shutdown_beacon)?;
        send_with_bounded_admission(
            &self.sender,
            stream,
            || {
                #[cfg(test)]
                self.signal_saturated_admission_for_test();
                Self::ensure_admission_open(lifecycle_control, shutdown_beacon)?;
                Ok(BOUNDED_ADMISSION_RETRY_INTERVAL)
            },
            "local IPC connection workers stopped accepting work",
        )
    }

    fn ensure_admission_open(
        lifecycle_control: &LifecycleControlSourceAdapter,
        shutdown_beacon: &ShutdownBeacon,
    ) -> Result<(), AtmError> {
        if lifecycle_control.terminate_requested() || shutdown_beacon.is_tripped() {
            return Err(AtmError::daemon_unavailable(
                "daemon local IPC connection admission stopped during shutdown",
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn install_saturated_admission_signal_for_test(
        &self,
        signal: std::sync::mpsc::SyncSender<()>,
    ) {
        *self
            .saturated_admission_signal
            .lock()
            .expect("lock saturated-admission test signal") = Some(signal);
    }

    #[cfg(test)]
    fn signal_saturated_admission_for_test(&self) {
        if let Some(signal) = self
            .saturated_admission_signal
            .lock()
            .expect("lock saturated-admission test signal")
            .take()
        {
            let _ = signal.send(());
        }
    }

    #[cfg(test)]
    pub(super) fn install_shutdown_join_signal_for_test(
        &self,
        signal: std::sync::mpsc::SyncSender<()>,
    ) {
        *self
            .shutdown_join_signal
            .lock()
            .expect("lock connection-worker shutdown-join test signal") = Some(signal);
    }

    #[cfg(test)]
    fn signal_shutdown_join_for_test(signal_slot: &Mutex<Option<std::sync::mpsc::SyncSender<()>>>) {
        if let Some(signal) = signal_slot
            .lock()
            .expect("lock connection-worker shutdown-join test signal")
            .take()
        {
            let _ = signal.send(());
        }
    }

    pub(super) fn shutdown(self) -> Result<(), AtmError> {
        let Self {
            sender,
            workers,
            #[cfg(test)]
            shutdown_join_signal,
            ..
        } = self;
        drop(sender);
        // The sender must be gone before joining: otherwise idle workers can
        // remain in `recv` and turn ordinary shutdown into a deadlock.
        #[cfg(test)]
        Self::signal_shutdown_join_for_test(&shutdown_join_signal);
        for worker in workers.into_inner().map_err(|_| {
            AtmError::daemon_unavailable("local IPC connection worker lock poisoned")
        })? {
            join_with_timeout(
                worker,
                LOCAL_WORKER_JOIN_DEADLINE,
                LOCAL_IPC_WORKER_JOIN_POLICY,
            )?;
        }
        Ok(())
    }
}
