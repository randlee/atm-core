#![allow(
    dead_code,
    reason = "AC.2 keeps these internal compatibility helpers available while later consumer cutovers complete."
)]

use crate::boundary::{
    ConfigLoadRequest, ConfigLoadResponse, ProjectionExportAppendMessageSetRequest,
    ProjectionExportAppendMessageSetResponse, ProjectionExportRecordRequest,
    ProjectionExportRecordResponse, ProjectionExportReexportMessageRequest,
    ProjectionExportReexportMessageResponse, SourceIngressDiagnosticsRequest,
    SourceIngressDiagnosticsResponse, SourceIngressIdentityFingerprintRequest,
    SourceIngressIdentityFingerprintResponse, SourceIngressImportRequest,
    SourceIngressImportResponse,
};
use crate::error::AtmError;

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    crate::boundary_support::load_workspace_config(request)
}

pub(crate) fn import_inbox_source(
    request: SourceIngressImportRequest,
) -> Result<SourceIngressImportResponse, AtmError> {
    crate::boundary_support::import_inbox_source(request)
}

pub(crate) fn compute_identity_fingerprint(
    request: SourceIngressIdentityFingerprintRequest,
) -> SourceIngressIdentityFingerprintResponse {
    crate::boundary_support::compute_identity_fingerprint(request)
}

pub(crate) fn report_inbox_diagnostics(
    request: SourceIngressDiagnosticsRequest,
) -> SourceIngressDiagnosticsResponse {
    crate::boundary_support::report_inbox_diagnostics(request)
}

pub(crate) fn export_source_files(
    request: ProjectionExportRecordRequest,
) -> Result<ProjectionExportRecordResponse, AtmError> {
    crate::boundary_support::export_source_files(request)
}

pub(crate) fn reexport_messages(
    request: ProjectionExportReexportMessageRequest,
) -> Result<ProjectionExportReexportMessageResponse, AtmError> {
    // Yb Y.10 constrains full mailbox re-export to explicit repair/rebuild
    // seams. Normal runtime delivery must use append-only Claude execution or
    // the typed non-Claude outbound boundary instead.
    crate::boundary_support::reexport_messages(request)
}

pub(crate) fn append_message_set(
    request: ProjectionExportAppendMessageSetRequest,
) -> Result<ProjectionExportAppendMessageSetResponse, AtmError> {
    crate::boundary_support::append_message_set(request)
}
