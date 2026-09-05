//! Retained-observability projection used by the daemon doctor response.

use atm_core::doctor::{DoctorFinding, DoctorSeverity};
use atm_core::error::AtmErrorCode;
use atm_core::observability::AtmObservabilityHealth;
use atm_core::observability_counters::{DiagnosticCounters, RetainedObservabilityHealth};

pub(crate) fn project_counter_health(
    observability: &mut AtmObservabilityHealth,
    counters: DiagnosticCounters,
) {
    observability.apply_retained_diagnostics(RetainedObservabilityHealth::from(counters));
}

pub(crate) fn append_counter_finding(
    findings: &mut Vec<DoctorFinding>,
    counters: DiagnosticCounters,
) {
    let degraded = !RetainedObservabilityHealth::from(counters)
        .degraded
        .is_empty();
    let severity = if degraded {
        DoctorSeverity::Warning
    } else {
        DoctorSeverity::Info
    };
    let code = if degraded {
        AtmErrorCode::WarningObservabilityHealthDegraded
    } else {
        AtmErrorCode::ObservabilityHealthOk
    };
    findings.push(DoctorFinding {
        severity,
        code,
        message: format!(
            "retained diagnostics: jsonl forwarded={} queue_full_dropped={} reentrant_dropped={}; timeline written={} queue_full_dropped={} persist_error_dropped={}",
            counters.jsonl_forwarded_total,
            counters.jsonl_dropped_queue_full_total,
            counters.jsonl_dropped_reentrant_total,
            counters.timeline_written_total,
            counters.timeline_dropped_queue_full_total,
            counters.timeline_dropped_persist_error_total,
        ),
        remediation: degraded.then(|| {
            "Inspect retained-log queue pressure and timeline persistence; query /v1/health for the current counters."
                .to_owned()
        }),
    });
}
