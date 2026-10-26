//! Construction and dependency configuration for the canonical HTTP router.

use super::*;

impl StorageAndNudgeRouter {
    pub fn new(
        service_runtime: LocalServiceRuntime,
        observability: Arc<dyn ObservabilityPort + Send + Sync>,
        received_hook_selector: Arc<dyn MessageReceivedHookSelector>,
        daemon_home: PathBuf,
    ) -> Self {
        let runtime_health = RuntimeHealth::default();
        Self {
            service_runtime,
            observability,
            received_hook_selector,
            control_path_sync_bridge: ControlPathSyncBridge::new(
                NonZeroUsize::new(1).expect("one non-storage core bridge operation"),
                runtime_health.clone(),
            ),
            write_source_preflight_bridge: WriteSourcePreflightBridge::new(runtime_health.clone()),
            async_mailbox_runtime: None,
            doctor_projection: None,
            daemon_home,
            runtime_health,
            doctor_ports: None,
            daemon_context: None,
            diagnostic_counters: None,
            peer_port_resolver: None,
            #[cfg(test)]
            direct_peer_port: crate::direct_peer_port(),
            peer_connection_pool: None,
            shared_direct_peer_client: None,
            maintenance: None,
            bare_cli_fifo: Default::default(),
            bare_cli_queue_full_drops: Default::default(),
            member_state_transition_sink: None,
            detached_received_hooks: DetachedReceivedHooks::default(),
            task_telemetry: atm_runtime::TaskTelemetryRuntime::disabled(),
            hooks: hook_wiring::HookWiring::default(),
        }
    }

    /// Installs the composition-owned Tokio mailbox port.  AV.1b routes the
    /// read family exclusively through this capability.
    #[must_use]
    pub fn with_async_mailbox_runtime(
        mut self,
        async_mailbox_runtime: Arc<dyn AsyncMailboxRuntime>,
    ) -> Self {
        self.async_mailbox_runtime = Some(async_mailbox_runtime);
        self
    }

    /// Installs the bounded, composition-owned doctor projection.  Doctor is
    /// a control-plane request and must not share the mailbox read bridge.
    #[must_use]
    pub fn with_doctor_projection(mut self, doctor_projection: Arc<dyn DoctorProjection>) -> Self {
        self.doctor_projection = Some(doctor_projection);
        self
    }

    #[cfg(test)]
    pub(super) fn with_direct_peer_port(mut self, direct_peer_port: NonZeroU16) -> Self {
        self.direct_peer_port = direct_peer_port;
        self
    }

    /// Installs the durable exact-peer authority used to select outbound ports.
    #[must_use]
    pub fn with_peer_port_resolver(mut self, resolver: PeerPortResolver) -> Self {
        self.peer_port_resolver = Some(resolver);
        self
    }

    /// Adds the existing core doctor ports and the process-owned lifecycle
    /// projection. Both stay behind core interfaces; the HTTP runtime never
    /// learns a concrete storage backend.
    #[must_use]
    pub fn with_runtime_health(
        mut self,
        runtime_health: RuntimeHealth,
        doctor_ports: atm_core::doctor::RuntimeDoctorPorts,
    ) -> Self {
        self.control_path_sync_bridge = ControlPathSyncBridge::new(
            NonZeroUsize::new(1).expect("one non-storage core bridge operation"),
            runtime_health.clone(),
        );
        self.write_source_preflight_bridge =
            WriteSourcePreflightBridge::new(runtime_health.clone());
        self.runtime_health = runtime_health;
        self.doctor_ports = Some(doctor_ports);
        self
    }

    /// Installs the process-owned retained-diagnostic counter projection for
    /// the doctor report. The runtime sees only the core snapshot contract.
    #[must_use]
    pub fn with_diagnostic_counters(mut self, counters: Arc<dyn DiagnosticCountersSource>) -> Self {
        self.diagnostic_counters = Some(counters);
        self
    }

    /// Installs the optional bootstrap diagnostic projection without forcing
    /// focused runtime fixtures to construct a tracing bridge.
    #[must_use]
    pub fn with_diagnostic_counters_option(
        mut self,
        counters: Option<Arc<dyn DiagnosticCountersSource>>,
    ) -> Self {
        self.diagnostic_counters = counters;
        self
    }

    /// Attaches the process-owned maintenance task to this composition root.
    #[must_use]
    pub fn with_maintenance(mut self, maintenance: Arc<dyn crate::RuntimeMaintenance>) -> Self {
        self.maintenance = Some(maintenance);
        self
    }

    /// Installs the daemon-lifetime bare-CLI FIFO and its overflow counter.
    #[must_use]
    pub fn with_bare_cli_fifo(
        mut self,
        fifo: BareCliFifo,
        queue_full_drops: BareCliQueueFullDrops,
    ) -> Self {
        self.bare_cli_fifo = fifo;
        self.bare_cli_queue_full_drops = queue_full_drops;
        self
    }

    /// Installs the best-effort idle-transition queue drain owned by daemon
    /// composition. The runtime only forwards the sealed notification.
    #[must_use]
    pub fn with_member_state_transition_sink(
        mut self,
        sink: Arc<dyn crate::MemberStateTransitionSink>,
    ) -> Self {
        self.member_state_transition_sink = Some(sink);
        self
    }

    /// Attaches daemon process identity captured at bootstrap to the doctor
    /// response. This is intentionally injected: request-time doctor handling
    /// must not read a mutable process environment to identify its server.
    #[must_use]
    pub fn with_daemon_context(
        mut self,
        daemon_context: atm_core::doctor::DoctorExecutionContext,
    ) -> Self {
        self.daemon_context = Some(daemon_context);
        self
    }

    /// Installs the daemon-owned outbound pool paired with the selected
    /// authenticated stream adapter. The pool is a transport implementation
    /// detail; canonical request routing remains in this router.
    #[must_use]
    pub fn with_peer_connection_pool(mut self, pool: PeerConnectionPool) -> Self {
        self.peer_connection_pool = Some(pool);
        self
    }

    /// Injects the daemon-lifetime reqwest client for direct plaintext peers.
    /// Clones share reqwest's built-in connection pool; this router never
    /// creates a second plaintext pooling mechanism.
    #[must_use]
    pub fn with_shared_direct_peer_client(mut self, client: reqwest::Client) -> Self {
        self.shared_direct_peer_client = Some(client);
        self
    }

    /// Projects committed task-ledger rows through the bounded runtime handle;
    /// the default is the disabled runtime.
    #[must_use]
    pub fn with_task_telemetry(mut self, runtime: atm_runtime::TaskTelemetryRuntime) -> Self {
        self.task_telemetry = runtime;
        self
    }

    /// Drains retained authenticated peer connections after HTTP request
    /// admission has stopped, then the receiver-hook work that peer responses
    /// deliberately did not wait for. Individual request guards remain
    /// non-blocking on drop; only this daemon lifecycle path awaits driver
    /// termination.
    /// Releases outbound peer drivers, detached received hooks, and hook
    /// chains within the one caller-owned shutdown `deadline`: each drain gets
    /// only what its predecessors left, never a fresh budget.
    pub async fn shutdown_peer_connections(&self, deadline: tokio::time::Instant) {
        if let Some(pool) = &self.peer_connection_pool {
            pool.shutdown(deadline.saturating_duration_since(tokio::time::Instant::now()))
                .await;
        }
        self.detached_received_hooks
            .drain(deadline.saturating_duration_since(tokio::time::Instant::now()))
            .await;
        self.hook_chains()
            .drain(RequestDeadline::after(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
            ))
            .await;
    }
}
