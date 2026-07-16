use std::net::SocketAddr;

use atm_core::doctor::{
    CrossHostAllowedHostDoctorRow, CrossHostInterfaceDoctorRow, DoctorFinding, DoctorSeverity,
    DoctorStatus, DoctorSummary,
};
use atm_core::error_codes::AtmErrorCode;

pub(super) fn build_doctor_summary(findings: &[DoctorFinding]) -> DoctorSummary {
    let status = atm_core::doctor::health::status_from_findings(findings);
    let (info_count, warning_count, error_count) = findings.iter().fold(
        (0usize, 0usize, 0usize),
        |(info, warning, error), finding| match finding.severity {
            DoctorSeverity::Info => (info + 1, warning, error),
            DoctorSeverity::Warning => (info, warning + 1, error),
            DoctorSeverity::Error => (info, warning, error + 1),
        },
    );
    let message = match status {
        DoctorStatus::Healthy => "ATM doctor completed with healthy findings only",
        DoctorStatus::Warning => "ATM doctor completed with warnings",
        DoctorStatus::Error => "ATM doctor found critical issues",
    };
    DoctorSummary {
        status,
        message: message.to_string(),
        info_count,
        warning_count,
        error_count,
    }
}

pub(super) fn build_cross_host_interface_row(
    row: &atm_storage::PeerInterfaceRow,
) -> CrossHostInterfaceDoctorRow {
    CrossHostInterfaceDoctorRow {
        interface_name: row.interface_name.clone(),
        bind_addr: row.bind_addr.to_string(),
        advertise_addr: row.advertise_addr.to_string(),
        port: row.port,
        enabled: row.enabled,
        listener_bound: row.enabled && row.last_bound_at.is_some() && row.last_bind_error.is_none(),
        last_bound_at: row.last_bound_at,
        last_bind_error: row.last_bind_error.clone(),
        stale_at: row.stale_at,
    }
}

pub(super) fn build_cross_host_allowlist_row(
    row: &atm_storage::AllowedHostRow,
) -> CrossHostAllowedHostDoctorRow {
    CrossHostAllowedHostDoctorRow {
        host_name: row.host_name.to_string(),
        enabled: row.enabled,
        disabled_at: row.disabled_at,
        note: row.note.clone(),
    }
}

pub(super) fn build_cross_host_bound_endpoints(
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

pub(super) fn build_cross_host_findings(
    interfaces: &[CrossHostInterfaceDoctorRow],
    live_bound_addr: Option<SocketAddr>,
    has_enabled_interface_rows: bool,
    legacy_fallback_active: bool,
    enabled_allowlist_count: usize,
) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();
    if legacy_fallback_active {
        findings.push(legacy_cross_host_fallback_finding(
            live_bound_addr.expect("legacy fallback requires a live bound address"),
        ));
    }
    findings.extend(
        interfaces
            .iter()
            .filter(|row| row.enabled && row.last_bind_error.is_some())
            .map(degraded_cross_host_listener_finding),
    );
    if has_enabled_interface_rows && enabled_allowlist_count == 0 {
        findings.push(empty_cross_host_allowlist_finding());
    }
    findings
}

fn legacy_cross_host_fallback_finding(bound_addr: SocketAddr) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: AtmErrorCode::WarningCrossHostLegacyFallbackActive,
        message: format!(
            "daemon cross-host listener is currently bound at {bound_addr} via legacy config fallback because no enabled durable interface rows exist"
        ),
        remediation: Some(
            "Run `atm daemon interfaces add <interface-name> --bind-addr <ip> --advertise-addr <ip> --port 43101 --kind <lan|vpn|loopback|other>`, restart atm-daemon, and rerun `atm doctor` so the durable interface rows become authoritative."
                .to_string(),
        ),
    }
}

fn degraded_cross_host_listener_finding(row: &CrossHostInterfaceDoctorRow) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: AtmErrorCode::WarningCrossHostListenerDegraded,
        message: format!(
            "daemon interface {} at {}:{} failed to bind for cross-host transport: {}",
            row.interface_name,
            row.bind_addr,
            row.port,
            row.last_bind_error
                .as_deref()
                .unwrap_or("unknown bind failure")
        ),
        remediation: Some(
            "Run `atm daemon interfaces list` to inspect the row, correct the bind address or port, restart atm-daemon, and rerun `atm doctor`."
                .to_string(),
        ),
    }
}

fn empty_cross_host_allowlist_finding() -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: AtmErrorCode::WarningCrossHostAllowlistEmpty,
        message: "cross-host host authorization is enforced but no enabled daemon allowed-host rows exist"
            .to_string(),
        remediation: Some(
            "Run `atm daemon hosts allow <host>` for each remote peer that should be admitted, then rerun `atm doctor`."
                .to_string(),
        ),
    }
}
