use std::net::SocketAddr;

use atm_core::{
    doctor::{
        self, CrossHostAllowedHostDoctorRow, CrossHostAllowlistDoctorReport, CrossHostDoctorReport,
        CrossHostInterfaceDoctorRow, CrossHostSecurityDoctorReport, CrossHostTrustedPeerDoctorRow,
        DaemonRuntimeDoctorReport, DoctorExecutionContext, DoctorFinding, DoctorQuery,
        DoctorReport, DoctorSeverity, DoctorStatus, DoctorSummary,
    },
    error::AtmError,
    error_codes::AtmErrorCode,
    observability::AtmObservabilityHealth,
    protocol::ReleaseVersion,
};

use super::{DaemonRequestDispatcher, runtime_status_finding};

impl DaemonRequestDispatcher {
    pub(super) fn project_doctor_report(
        &self,
        query: DoctorQuery,
    ) -> Result<DoctorReport, AtmError> {
        let daemon_observability_finding = match self.observability.health() {
            Ok(health) => daemon_observability_finding(&health),
            Err(error) => doctor::health::observability_finding_from_error(&error),
        };
        let daemon_runtime = DaemonRuntimeDoctorReport {
            findings: vec![daemon_observability_finding],
        };
        let mut report = doctor::run_doctor_with_runtime_ports(
            query,
            self.observability.as_ref(),
            &self.service_runtime,
            &self.doctor_ports,
            Some(daemon_runtime),
        )?;
        let (cross_host, cross_host_findings) = self.project_cross_host_report()?;
        report.cross_host = Some(cross_host);
        let runtime_status = match &report.member_roster {
            Some(roster) => self.status_cache.snapshot_for_members(
                roster
                    .members
                    .iter()
                    .map(|member| (roster.team.clone(), member.name.clone())),
            ),
            None => self.status_cache.snapshot(),
        };
        report.findings.extend(cross_host_findings.clone());
        let runtime_status_finding = runtime_status_finding(&runtime_status);
        report.findings.push(runtime_status_finding.clone());
        if let Some(daemon_runtime) = report.daemon_runtime.as_mut() {
            daemon_runtime.findings.extend(cross_host_findings);
            daemon_runtime.findings.push(runtime_status_finding);
        } else {
            report.daemon_runtime = Some(DaemonRuntimeDoctorReport {
                findings: vec![runtime_status_finding],
            });
        }
        rebuild_doctor_summary(&mut report);
        report.runtime_status = Some(runtime_status);
        report.daemon_context = Some(self.build_daemon_execution_context());
        Ok(report)
    }

    fn project_cross_host_report(
        &self,
    ) -> Result<(CrossHostDoctorReport, Vec<DoctorFinding>), AtmError> {
        let interface_rows = self.peer_interface_config_store.list_interfaces()?;
        let host_rows = self.allowed_host_store.list_hosts()?;
        let security_settings = self.peer_security_store.load_security_settings()?;
        let local_identity = self.peer_security_store.load_local_identity()?;
        let trusted_peers = self.peer_security_store.list_trusted_peers()?;
        let live_bound_addr = self.peer_transport_runtime.bound_addr()?;
        let has_enabled_interface_rows = interface_rows.iter().any(|row| row.enabled);
        let legacy_fallback_active = !has_enabled_interface_rows && live_bound_addr.is_some();

        let interfaces = build_cross_host_interfaces(&interface_rows);
        let bound_endpoints = collect_bound_endpoints(&interfaces, live_bound_addr);
        let allowlist_hosts = build_cross_host_allowlist(&host_rows);
        let enabled_allowlist_count = host_rows.iter().filter(|row| row.enabled).count();
        let security = CrossHostSecurityDoctorReport {
            mode: security_settings.mode,
            updated_by: security_settings.updated_by,
            updated_at: security_settings.updated_at,
            local_identity_present: local_identity.is_some(),
            local_identity_fingerprint_sha256: local_identity
                .as_ref()
                .map(|row| row.fingerprint_sha256().to_string()),
            trusted_peers: trusted_peers
                .iter()
                .map(|row| CrossHostTrustedPeerDoctorRow {
                    host_name: row.host_name().to_string(),
                    fingerprint_sha256: row.fingerprint_sha256().to_string(),
                    display_name: row.display_name().map(ToString::to_string),
                })
                .collect(),
        };

        let findings = build_cross_host_findings(
            legacy_fallback_active,
            has_enabled_interface_rows,
            live_bound_addr,
            &interfaces,
            enabled_allowlist_count,
        );

        Ok((
            CrossHostDoctorReport {
                legacy_fallback_active,
                bound_endpoints,
                interfaces,
                allowlist: CrossHostAllowlistDoctorReport {
                    enforced: true,
                    empty: enabled_allowlist_count == 0,
                    hosts: allowlist_hosts,
                },
                security,
            },
            findings,
        ))
    }

    fn build_daemon_execution_context(&self) -> DoctorExecutionContext {
        DoctorExecutionContext {
            team: None,
            identity: None,
            version: Some(ReleaseVersion::current()),
        }
    }
}

pub(super) fn rebuild_doctor_summary(report: &mut DoctorReport) {
    report.recommendations = report
        .findings
        .iter()
        .filter_map(|finding| finding.remediation.clone())
        .collect();
    let status = doctor::health::status_from_findings(&report.findings);
    let (info_count, warning_count, error_count) = doctor_summary_counts(&report.findings);
    let message = match status {
        DoctorStatus::Healthy => "ATM doctor completed with healthy findings only",
        DoctorStatus::Warning => "ATM doctor completed with warnings",
        DoctorStatus::Error => "ATM doctor found critical issues",
    };
    report.summary = DoctorSummary {
        status,
        message: message.to_string(),
        info_count,
        warning_count,
        error_count,
    };
}

fn doctor_summary_counts(findings: &[DoctorFinding]) -> (usize, usize, usize) {
    findings.iter().fold(
        (0usize, 0usize, 0usize),
        |(info, warning, error), finding| match finding.severity {
            DoctorSeverity::Info => (info + 1, warning, error),
            DoctorSeverity::Warning => (info, warning + 1, error),
            DoctorSeverity::Error => (info, warning, error + 1),
        },
    )
}

fn build_cross_host_interfaces(
    interface_rows: &[atm_storage::PeerInterfaceRow],
) -> Vec<CrossHostInterfaceDoctorRow> {
    interface_rows
        .iter()
        .map(|row| CrossHostInterfaceDoctorRow {
            interface_name: row.interface_name.clone(),
            bind_addr: row.bind_addr.to_string(),
            advertise_addr: row.advertise_addr.to_string(),
            port: row.port,
            enabled: row.enabled,
            listener_bound: row.enabled
                && row.last_bound_at.is_some()
                && row.last_bind_error.is_none(),
            last_bound_at: row.last_bound_at,
            last_bind_error: row.last_bind_error.clone(),
            stale_at: row.stale_at,
        })
        .collect()
}

fn collect_bound_endpoints(
    interfaces: &[CrossHostInterfaceDoctorRow],
    live_bound_addr: Option<SocketAddr>,
) -> Vec<String> {
    let mut bound_endpoints = interfaces
        .iter()
        .filter(|row| row.listener_bound)
        .map(|row| format!("{}:{}", row.bind_addr, row.port))
        .collect::<Vec<_>>();
    if bound_endpoints.is_empty()
        && let Some(bound_addr) = live_bound_addr
    {
        bound_endpoints.push(bound_addr.to_string());
    }
    bound_endpoints
}

fn build_cross_host_allowlist(
    host_rows: &[atm_storage::AllowedHostRow],
) -> Vec<CrossHostAllowedHostDoctorRow> {
    host_rows
        .iter()
        .map(|row| CrossHostAllowedHostDoctorRow {
            host_name: row.host_name.to_string(),
            enabled: row.enabled,
            disabled_at: row.disabled_at,
            note: row.note.clone(),
        })
        .collect()
}

fn build_cross_host_findings(
    legacy_fallback_active: bool,
    has_enabled_interface_rows: bool,
    live_bound_addr: Option<SocketAddr>,
    interfaces: &[CrossHostInterfaceDoctorRow],
    enabled_allowlist_count: usize,
) -> Vec<DoctorFinding> {
    let mut findings = cross_host_listener_findings(
        legacy_fallback_active,
        has_enabled_interface_rows,
        live_bound_addr,
    );
    findings.extend(cross_host_degraded_bind_findings(interfaces));
    if enabled_allowlist_count == 0 {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningCrossHostAllowlistEmpty,
            message: "cross-host host authorization is enforced but no enabled daemon allowed-host rows exist".to_string(),
            remediation: Some(
                "Run `atm daemon hosts allow <host>` for each remote peer that should be admitted, then rerun `atm doctor`."
                    .to_string(),
            ),
        });
    }
    findings
}

fn cross_host_listener_findings(
    legacy_fallback_active: bool,
    has_enabled_interface_rows: bool,
    live_bound_addr: Option<SocketAddr>,
) -> Vec<DoctorFinding> {
    if legacy_fallback_active {
        let bound_addr = live_bound_addr.expect("checked above");
        return vec![DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningCrossHostLegacyFallbackActive,
            message: format!(
                "daemon cross-host listener is currently bound at {bound_addr} via legacy config fallback because no enabled durable interface rows exist"
            ),
            remediation: Some(
                "Run `atm daemon interfaces add <interface-name> --bind-addr <ip> --advertise-addr <ip> --port 43101 --kind <lan|vpn|loopback|other>`, restart atm-daemon, and rerun `atm doctor` so the durable interface rows become authoritative."
                    .to_string(),
            ),
        }];
    }
    if has_enabled_interface_rows {
        return Vec::new();
    }
    vec![DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: AtmErrorCode::WarningCrossHostListenerUnconfigured,
        message: "no enabled daemon interface rows are configured for cross-host listener binding"
            .to_string(),
        remediation: Some(
            "Run `atm daemon interfaces add <interface-name> --bind-addr <ip> --advertise-addr <ip> --port 43101 --kind <lan|vpn|loopback|other>`, restart atm-daemon, and rerun `atm doctor`."
                .to_string(),
        ),
    }]
}

fn cross_host_degraded_bind_findings(
    interfaces: &[CrossHostInterfaceDoctorRow],
) -> Vec<DoctorFinding> {
    interfaces
        .iter()
        .filter(|row| row.enabled && row.last_bind_error.is_some())
        .map(|row| DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningCrossHostListenerDegraded,
            message: format!(
                "daemon interface {} at {}:{} failed to bind for cross-host transport: {}",
                row.interface_name,
                row.bind_addr,
                row.port,
                row.last_bind_error.as_deref().unwrap_or("unknown bind failure")
            ),
            remediation: Some(
                "Run `atm daemon interfaces list` to inspect the row, correct the bind address or port, restart atm-daemon, and rerun `atm doctor`."
                    .to_string(),
            ),
        })
        .collect()
}

fn daemon_observability_finding(health: &AtmObservabilityHealth) -> DoctorFinding {
    let path = health
        .active_log_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unavailable>".to_string());
    let detail = health
        .detail
        .as_ref()
        .map(|detail| format!(" Detail: {detail}"))
        .unwrap_or_default();
    match health.logging_state {
        atm_core::observability::AtmObservabilityHealthState::Healthy => DoctorFinding {
            severity: DoctorSeverity::Info,
            code: AtmErrorCode::ObservabilityHealthOk,
            message: format!(
                "daemon retained observability sink is healthy at {path}; daemon query/follow remain deferred to the CLI-owned log surface.{detail}"
            ),
            remediation: None,
        },
        atm_core::observability::AtmObservabilityHealthState::Degraded => DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningObservabilityHealthDegraded,
            message: format!(
                "daemon retained observability sink is degraded at {path}; daemon query/follow remain deferred to the CLI-owned log surface.{detail}"
            ),
            remediation: Some(
                "Inspect the daemon retained log path and sink errors, then re-run `atm doctor`."
                    .to_string(),
            ),
        },
        atm_core::observability::AtmObservabilityHealthState::Unavailable => DoctorFinding {
            severity: DoctorSeverity::Error,
            code: AtmErrorCode::ObservabilityHealthFailed,
            message: format!(
                "daemon retained observability sink is unavailable at {path}; daemon query/follow remain deferred to the CLI-owned log surface.{detail}"
            ),
            remediation: Some(
                "Restore the daemon retained-log path and confirm it is writable before re-running `atm doctor`."
                    .to_string(),
            ),
        },
    }
}
