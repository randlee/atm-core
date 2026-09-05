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
        AtmErrorCode::WarningRetainedDiagnosticsDegraded
    } else {
        AtmErrorCode::RetainedDiagnosticsHealthOk
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

#[cfg(test)]
mod tests {
    use super::append_counter_finding;
    use atm_core::doctor::DoctorSeverity;
    use atm_core::error::AtmErrorCode;
    use atm_core::observability_counters::DiagnosticCounters;

    #[test]
    fn retained_diagnostics_finding_uses_its_own_error_codes() {
        let mut findings = Vec::new();
        append_counter_finding(&mut findings, DiagnosticCounters::default());
        assert_eq!(findings[0].severity, DoctorSeverity::Info);
        assert_eq!(findings[0].code, AtmErrorCode::RetainedDiagnosticsHealthOk);

        append_counter_finding(
            &mut findings,
            DiagnosticCounters {
                timeline_dropped_queue_full_total: 1,
                ..DiagnosticCounters::default()
            },
        );
        assert_eq!(findings[1].severity, DoctorSeverity::Warning);
        assert_eq!(
            findings[1].code,
            AtmErrorCode::WarningRetainedDiagnosticsDegraded
        );
    }
}
