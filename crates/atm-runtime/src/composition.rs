use std::fmt;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use atm_core::boundary::{
    self, ConfigDoctor, ConfigDoctorReport, NonClaudeOutbound, TemplateComposer,
};
use atm_core::doctor::{DoctorFinding, DoctorSeverity};
use atm_core::doctor::{ReaderLaneDoctorReport, ReaderLanesDoctorReport, RuntimeDoctorPorts};
use atm_core::error::AtmError;
use atm_core::home::HostRuntimeScope;
use atm_core::{LocalServiceRuntime, load_atm_config};
use atm_storage::{
    DiagnosticTimelineStore, MessageStore as SharedMessageStore, PeerConfigStore,
    RosterStore as SharedRosterStore, StorageFactory,
};

use crate::legacy_storage_adapters::{
    StorageBackends, boundary_mail_store_view, boundary_roster_store_view, runtime_doctor_ports,
};
use crate::mailbox_runtime::StorageAsyncMailboxRuntime;
use crate::workflow_telemetry::{
    WorkflowTelemetryDiagnostics, WorkflowTelemetryRuntime, WorkflowTelemetrySetup,
};

#[derive(Clone)]
pub struct RuntimeAssemblyInputs {
    pub host_runtime_scope: HostRuntimeScope,
    pub storage_factory: Arc<dyn StorageFactory>,
    pub config_current_dir: PathBuf,
    pub non_claude_outbound: Arc<dyn NonClaudeOutbound + Send + Sync>,
    /// Optional application port supplied by the bootstrap composition root.
    /// Runtime owns no template-adapter dependency or implementation detail.
    pub template_composer: Option<Arc<dyn TemplateComposer>>,
    /// Optional bootstrap-owned telemetry exporter. `None` selects the core
    /// no-op sink; invalid supplied limits degrade doctor only.
    pub workflow_telemetry: Option<WorkflowTelemetrySetup>,
}

impl fmt::Debug for RuntimeAssemblyInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeAssemblyInputs")
            .field("host_runtime_scope", &self.host_runtime_scope)
            .field("storage_factory", &"dyn StorageFactory")
            .field("config_current_dir", &self.config_current_dir)
            .field("non_claude_outbound", &"dyn NonClaudeOutbound")
            .field(
                "template_composer",
                &self
                    .template_composer
                    .as_ref()
                    .map(|_| "dyn TemplateComposer"),
            )
            .field(
                "workflow_telemetry",
                &self
                    .workflow_telemetry
                    .as_ref()
                    .map(|_| "configured exporter"),
            )
            .finish()
    }
}

#[derive(Clone)]
pub struct RuntimeAssembly {
    pub service_runtime: LocalServiceRuntime,
    /// Runtime-inert AV.1a port. AV.1b is the only sprint authorized to wire
    /// an HTTP handler through it.
    pub async_mailbox_runtime: StorageAsyncMailboxRuntime,
    pub(crate) storage_backends: StorageBackends<
        Arc<dyn SharedMessageStore + Send + Sync>,
        Arc<dyn SharedRosterStore + Send + Sync>,
    >,
    pub nudge_template_override_store: Arc<dyn boundary::NudgeTemplateOverrideStore + Send + Sync>,
    pub peer_config_store: Arc<dyn PeerConfigStore + Send + Sync>,
    /// Read-only retained diagnostic timeline supplied by the selected storage backend.
    pub diagnostic_timeline: Arc<dyn DiagnosticTimelineStore + Send + Sync>,
    pub doctor_ports: RuntimeDoctorPorts,
    pub reader_lanes: Option<ReaderLanesDoctorReport>,
    pub workflow_telemetry: WorkflowTelemetryRuntime,
    template_composer: Option<Arc<dyn TemplateComposer>>,
}

impl fmt::Debug for RuntimeAssembly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeAssembly")
            .field("service_runtime", &self.service_runtime)
            .field("async_mailbox_runtime", &"StorageAsyncMailboxRuntime")
            .field("storage_backends", &self.storage_backends)
            .field(
                "nudge_template_override_store",
                &"dyn NudgeTemplateOverrideStore",
            )
            .field("peer_config_store", &"dyn PeerConfigStore")
            .field("diagnostic_timeline", &"dyn DiagnosticTimelineStore")
            .field("doctor_ports", &self.doctor_ports)
            .field("reader_lanes", &self.reader_lanes)
            .field("workflow_telemetry", &"WorkflowTelemetryRuntime")
            .field(
                "template_composer",
                &self
                    .template_composer
                    .as_ref()
                    .map(|_| "dyn TemplateComposer"),
            )
            .finish()
    }
}

#[derive(Debug)]
struct RuntimeConfigDoctor {
    // `None` is the daemon-owned doctor.  A system daemon has no caller
    // workspace and therefore must not read `.atm.toml` while answering
    // doctor requests.
    config_current_dir: Option<PathBuf>,
    workflow_telemetry: Arc<WorkflowTelemetryDiagnostics>,
}

impl boundary::sealed::Sealed for RuntimeConfigDoctor {}

impl ConfigDoctor for RuntimeConfigDoctor {
    fn inspect_config(&self) -> Result<ConfigDoctorReport, AtmError> {
        if let Some(config_current_dir) = &self.config_current_dir {
            let _ = load_atm_config(config_current_dir)?;
        }
        let mut findings = Vec::new();
        if self
            .workflow_telemetry
            .config_invalid
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            findings.push(DoctorFinding {
                severity: DoctorSeverity::Warning,
                code: atm_storage::AtmErrorCode::WorkflowTelemetryConfigInvalid,
                message: "workflow telemetry configuration is invalid; telemetry is disabled"
                    .to_owned(),
                remediation: Some(
                    "Repair the configured telemetry queue or timeout limits, then restart the daemon."
                        .to_owned(),
                ),
            });
        }
        Ok(ConfigDoctorReport { findings })
    }
}

pub fn assemble_runtime(inputs: RuntimeAssemblyInputs) -> Result<RuntimeAssembly, AtmError> {
    let template_composer = inputs.template_composer;
    let workflow_telemetry = inputs
        .workflow_telemetry
        .map_or_else(WorkflowTelemetryRuntime::disabled, |setup| {
            WorkflowTelemetryRuntime::start(setup.config, setup.sink)
        });
    let storage = inputs
        .storage_factory
        .open(inputs.host_runtime_scope.durable_state_root.as_ref())?;
    let reader_lanes = storage
        .effective_reader_lanes()
        .map(|lanes| ReaderLanesDoctorReport {
            mailbox: ReaderLaneDoctorReport {
                pool_size: lanes.mailbox.pool_size,
                queue_depth: lanes.mailbox.queue_depth,
            },
            search: ReaderLaneDoctorReport {
                pool_size: lanes.search.pool_size,
                queue_depth: lanes.search.queue_depth,
            },
        });
    let storage_backends = StorageBackends {
        messages: storage.message_store(),
        rosters: storage.roster_store(),
    };
    let async_message_store = storage.async_message_store();
    let async_mailbox_reader = storage.async_mailbox_reader();
    let async_message_search_store = storage.async_message_search_store();
    let template_catalog_store = storage.template_catalog_store();
    let nudge_template_override_store = storage.nudge_template_override_store();
    let pending_nudge_store = storage.pending_nudge_store();
    let graft_receiver_endpoint_store = storage.graft_receiver_endpoint_store();
    let peer_config_store = storage.peer_config_store();
    let diagnostic_timeline = storage.diagnostic_timeline();
    let service_runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        storage_backends.messages.clone(),
        storage_backends.rosters.clone(),
        Arc::clone(&nudge_template_override_store),
        inputs.non_claude_outbound,
    )
    .with_async_message_store(Arc::clone(&async_message_store))
    .with_async_mailbox_reader(Arc::clone(&async_mailbox_reader))
    .with_async_message_search_store(async_message_search_store)
    .with_pending_nudge_store(pending_nudge_store)
    .with_graft_receiver_endpoint_store(graft_receiver_endpoint_store)
    .with_template_rendering(template_catalog_store, template_composer.clone());
    let doctor_ports = runtime_doctor_ports(Arc::new(RuntimeConfigDoctor {
        config_current_dir: Some(inputs.config_current_dir),
        workflow_telemetry: Arc::clone(workflow_telemetry.diagnostics()),
    }));
    Ok(RuntimeAssembly {
        service_runtime,
        async_mailbox_runtime: StorageAsyncMailboxRuntime::new(
            async_mailbox_reader,
            async_message_store,
        ),
        storage_backends,
        nudge_template_override_store,
        peer_config_store,
        diagnostic_timeline,
        doctor_ports,
        reader_lanes,
        workflow_telemetry,
        template_composer,
    })
}

/// Validate all enabled HTTPS configuration before the daemon publishes any
/// HTTPS service. AI.8 performs a bind preflight only; AI.9 owns the actual
/// listener lifetime and request handling.
/// Validate the peer-listener configuration immediately before daemon startup.
/// Ordinary CLI commands intentionally do not call this: they use the shared
/// runtime assembly only to reach durable configuration and mailbox boundaries.
pub fn validate_enabled_peer_configuration(
    store: &(dyn PeerConfigStore + Send + Sync),
) -> Result<(), AtmError> {
    validate_enabled_peer_configuration_for_reload(store)?;
    let enabled = store
        .list_interfaces()?
        .into_iter()
        .filter(|interface| interface.enabled)
        .collect::<Vec<_>>();
    for interface in enabled {
        TcpListener::bind(interface.bind_addr).map_err(|error| {
            AtmError::bind_preflight(format!(
                "HTTPS bind preflight failed for {}: {error}",
                interface.bind_addr
            ))
        })?;
    }
    Ok(())
}

pub fn validate_enabled_peer_configuration_for_reload(
    store: &(dyn PeerConfigStore + Send + Sync),
) -> Result<(), AtmError> {
    for peer in store.list_trusted_peers()? {
        if peer.enabled && peer.fingerprint.as_str().trim().is_empty() {
            return Err(AtmError::peer_config_validation(
                "enabled trusted peers require a non-empty pinned fingerprint",
            ));
        }
    }
    let enabled = store
        .list_interfaces()?
        .into_iter()
        .filter(|interface| interface.enabled)
        .collect::<Vec<_>>();
    if enabled.is_empty() {
        return Ok(());
    }
    let certificate = store.local_certificate()?.ok_or_else(|| {
        AtmError::peer_config_validation(
            "enabled HTTPS interfaces require a configured local certificate reference",
        )
    })?;
    if certificate.fingerprint.as_str().trim().is_empty()
        || certificate.private_key_ref.as_str().trim().is_empty()
    {
        return Err(AtmError::peer_config_validation(
            "enabled HTTPS interfaces require a non-empty certificate fingerprint and key reference",
        ));
    }
    Ok(())
}

impl RuntimeAssembly {
    /// Select the system-daemon view of the assembled runtime.
    ///
    /// IPC and peer requests carry caller context but must never cause the
    /// daemon to read a caller workspace's `.atm.toml` file.
    pub fn for_daemon(mut self) -> Self {
        self.service_runtime = self.service_runtime.without_workspace_config();
        self.doctor_ports = runtime_doctor_ports(Arc::new(RuntimeConfigDoctor {
            config_current_dir: None,
            workflow_telemetry: Arc::clone(self.workflow_telemetry.diagnostics()),
        }));
        self
    }

    pub fn message_store_arc(&self) -> Arc<dyn SharedMessageStore + Send + Sync> {
        self.storage_backends.messages.clone()
    }

    pub fn mail_store_arc(&self) -> Arc<dyn boundary::MailStore + Send + Sync> {
        boundary_mail_store_view(self.storage_backends.messages.clone())
    }

    pub fn roster_store_arc(&self) -> Arc<dyn boundary::RosterStore + Send + Sync> {
        boundary_roster_store_view(self.storage_backends.rosters.clone())
    }

    pub fn shared_roster_store_arc(&self) -> Arc<dyn SharedRosterStore + Send + Sync> {
        self.storage_backends.rosters.clone()
    }

    pub fn peer_config_store(&self) -> Arc<dyn PeerConfigStore + Send + Sync> {
        Arc::clone(&self.peer_config_store)
    }
    /// Returns the bootstrap-provided template-composition port, if enabled.
    pub fn template_composer(&self) -> Option<Arc<dyn TemplateComposer>> {
        self.service_runtime.template_composer()
    }
}

/// Invoke the retained roster boundary through the runtime selected by
/// atm-core. This preserves fixture-scoped runtime installation in tests.
pub fn with_installed_roster_store<T>(
    f: impl FnOnce(&(dyn boundary::RosterStore + Send + Sync)) -> Result<T, AtmError>,
) -> Result<T, AtmError> {
    atm_core::with_default_local_service_runtime(|runtime| {
        let roster_store = boundary_roster_store_view(runtime.shared_roster_store_arc());
        f(roster_store.as_ref())
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use atm_core::boundary::ConfigDoctor;
    use atm_storage::{
        CertificateFingerprint, HostName, HttpsInterface, LocalCertificate, PeerConfigStore,
        PrivateKeyRef, TrustedPeer,
    };

    use super::{RuntimeConfigDoctor, validate_enabled_peer_configuration};
    use crate::workflow_telemetry::WorkflowTelemetryDiagnostics;

    #[test]
    fn daemon_config_doctor_does_not_read_a_caller_workspace() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!(
            "atm-runtime-daemon-doctor-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&workspace).expect("workspace fixture");
        std::fs::write(workspace.join(".atm.toml"), "not = [valid")
            .expect("write invalid workspace config");

        let caller_doctor = RuntimeConfigDoctor {
            config_current_dir: Some(PathBuf::from(&workspace)),
            workflow_telemetry: Arc::new(WorkflowTelemetryDiagnostics::default()),
        };
        assert!(caller_doctor.inspect_config().is_err());

        RuntimeConfigDoctor {
            config_current_dir: None,
            workflow_telemetry: Arc::new(WorkflowTelemetryDiagnostics::default()),
        }
        .inspect_config()
        .expect("daemon doctor must ignore caller workspace config");
        std::fs::remove_dir_all(workspace).expect("remove workspace fixture");
    }

    #[test]
    fn invalid_telemetry_configuration_is_a_doctor_warning_not_a_runtime_failure() {
        let diagnostics = Arc::new(WorkflowTelemetryDiagnostics::default());
        diagnostics
            .config_invalid
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let report = RuntimeConfigDoctor {
            config_current_dir: None,
            workflow_telemetry: diagnostics,
        }
        .inspect_config()
        .expect("doctor report");
        assert_eq!(report.findings.len(), 1);
        assert_eq!(
            report.findings[0].code,
            atm_storage::AtmErrorCode::WorkflowTelemetryConfigInvalid
        );
        assert_eq!(
            report.findings[0].severity,
            atm_core::doctor::DoctorSeverity::Warning
        );
    }

    #[derive(Default)]
    struct TestPeerConfigStore {
        interfaces: Vec<HttpsInterface>,
        certificate: Option<LocalCertificate>,
        peers: Vec<TrustedPeer>,
    }

    impl atm_storage::contract::sealed::Sealed for TestPeerConfigStore {}

    impl PeerConfigStore for TestPeerConfigStore {
        fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, atm_storage::AtmError> {
            Ok(self.interfaces.clone())
        }

        fn save_interface(&self, _interface: &HttpsInterface) -> Result<(), atm_storage::AtmError> {
            unreachable!("validation fixture is read-only")
        }

        fn remove_interface(
            &self,
            _bind_addr: std::net::SocketAddr,
        ) -> Result<bool, atm_storage::AtmError> {
            unreachable!("validation fixture is read-only")
        }

        fn local_certificate(&self) -> Result<Option<LocalCertificate>, atm_storage::AtmError> {
            Ok(self.certificate.clone())
        }

        fn save_local_certificate(
            &self,
            _certificate: &LocalCertificate,
        ) -> Result<(), atm_storage::AtmError> {
            unreachable!("validation fixture is read-only")
        }

        fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, atm_storage::AtmError> {
            Ok(self.peers.clone())
        }

        fn trusted_peer(
            &self,
            _host: &HostName,
        ) -> Result<Option<TrustedPeer>, atm_storage::AtmError> {
            unreachable!("validation fixture is read-only")
        }

        fn save_trusted_peer(&self, _peer: &TrustedPeer) -> Result<(), atm_storage::AtmError> {
            unreachable!("validation fixture is read-only")
        }

        fn remove_trusted_peer(&self, _host: &HostName) -> Result<bool, atm_storage::AtmError> {
            unreachable!("validation fixture is read-only")
        }
    }

    fn enabled_interface() -> HttpsInterface {
        HttpsInterface {
            bind_addr: "127.0.0.1:0".parse().expect("bind address"),
            advertise_host: "localhost".parse().expect("host"),
            enabled: true,
        }
    }

    fn certificate() -> LocalCertificate {
        LocalCertificate {
            fingerprint: "sha256:test"
                .parse::<CertificateFingerprint>()
                .expect("fingerprint"),
            private_key_ref: "keychain://atm/test"
                .parse::<PrivateKeyRef>()
                .expect("key ref"),
        }
    }

    #[test]
    fn enabled_peer_configuration_accepts_complete_configuration() {
        let store = TestPeerConfigStore {
            interfaces: vec![enabled_interface()],
            certificate: Some(certificate()),
            peers: Vec::new(),
        };
        validate_enabled_peer_configuration(&store).expect("complete peer config");
    }

    #[test]
    fn enabled_peer_configuration_rejects_missing_certificate() {
        let store = TestPeerConfigStore {
            interfaces: vec![enabled_interface()],
            certificate: None,
            peers: Vec::new(),
        };
        let error = validate_enabled_peer_configuration(&store).expect_err("missing cert");
        assert!(error.message().contains("configured local certificate"));
    }

    #[test]
    fn disabled_peer_configuration_requires_no_certificate() {
        validate_enabled_peer_configuration(&TestPeerConfigStore::default())
            .expect("disabled peer configuration");
    }
}
