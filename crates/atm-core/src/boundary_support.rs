#![allow(
    dead_code,
    reason = "AC.2 preserves these crate-private Claude compatibility helpers until later internal consumers are removed."
)]

//! Hidden daemon-side ingress/export helper layer used by concrete boundary
//! adapters.

use crate::boundary::{
    ConfigLoadRequest, ConfigLoadResponse, ProjectionAppendMode,
    ProjectionExportAppendMessageSetRequest, ProjectionExportAppendMessageSetResponse,
    ProjectionExportRecordRequest, ProjectionExportRecordResponse,
    ProjectionExportReexportMessageRequest, ProjectionExportReexportMessageResponse,
    SourceIngressDiagnosticsRequest, SourceIngressDiagnosticsResponse,
    SourceIngressIdentityFingerprintRequest, SourceIngressIdentityFingerprintResponse,
    SourceIngressImportRequest, SourceIngressImportResponse,
};
use crate::config;
use crate::error::AtmError;

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    Ok(ConfigLoadResponse {
        config: config::load_config(&request.current_dir).map_err(|error| {
            AtmError::config(format!(
                "daemon ConfigIngress could not load workspace config from {}",
                request.current_dir.display()
            ))
            .with_recovery(
                "Fix the workspace ATM configuration or current-directory selection before retrying daemon config ingress.",
            )
            .with_source(error)
        })?,
    })
}

pub(crate) fn import_inbox_source(
    request: SourceIngressImportRequest,
) -> Result<SourceIngressImportResponse, AtmError> {
    let team = request.team.clone();
    let agent = request.agent.clone();
    let home_dir = request.home_dir.display().to_string();
    atm_storage_claude::compat::import_inbox_source(request).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon inbox ingress could not import source projections for {}@{} from {}",
            agent, team, home_dir
        ))
        .with_recovery(
            "Fix the team inbox source files or ATM home selection before retrying daemon inbox ingestion.",
        )
        .with_source(error)
    })
}

pub(crate) fn compute_identity_fingerprint(
    request: SourceIngressIdentityFingerprintRequest,
) -> SourceIngressIdentityFingerprintResponse {
    let fingerprint = atm_storage_claude::compat::compute_identity_fingerprint(
        atm_storage_claude::compat::SourceIngressIdentityFingerprintRequest {
            message: request.message,
        },
    )
    .fingerprint
    .map(crate::boundary::MessageFingerprint::from);
    SourceIngressIdentityFingerprintResponse { fingerprint }
}

pub(crate) fn report_inbox_diagnostics(
    request: SourceIngressDiagnosticsRequest,
) -> SourceIngressDiagnosticsResponse {
    atm_storage_claude::compat::report_inbox_diagnostics(request)
}

pub(crate) fn export_source_files(
    request: ProjectionExportRecordRequest,
) -> Result<ProjectionExportRecordResponse, AtmError> {
    atm_storage_claude::compat::export_source_files(request).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon inbox export could not commit source projection path(s)"
        ))
        .with_recovery(
            "Fix the destination inbox projection files or ATM home permissions before retrying daemon inbox export.",
        )
        .with_source(error)
    })
}

pub(crate) fn reexport_messages(
    request: ProjectionExportReexportMessageRequest,
) -> Result<ProjectionExportReexportMessageResponse, AtmError> {
    let path = request.path.display().to_string();
    atm_storage_claude::compat::reexport_messages(request).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon inbox export could not rewrite mailbox projection {}",
            path
        ))
        .with_recovery(
            "Fix the destination mailbox projection path or file permissions before retrying daemon message re-export.",
        )
        .with_source(error)
    })
}

pub(crate) fn append_message_set(
    request: ProjectionExportAppendMessageSetRequest,
) -> Result<ProjectionExportAppendMessageSetResponse, AtmError> {
    let path = request.path.display().to_string();
    let _ = match request.mode {
        ProjectionAppendMode::RecoveredLogicalMessageSet => (),
    };
    atm_storage_claude::compat::append_message_set(request).map_err(|error| {
        AtmError::daemon_unavailable(format!(
            "daemon inbox export could not materialize recovered logical message set for {}",
            path
        ))
        .with_recovery(
            "Fix the destination mailbox projection path or file permissions before retrying recovered Claude compatibility delivery.",
        )
        .with_source(error)
    })
}
