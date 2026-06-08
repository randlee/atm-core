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

pub fn load_workspace_config(request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError> {
    crate::boundary_support::load_workspace_config(request)
}

pub fn import_inbox_source(
    request: SourceIngressImportRequest,
) -> Result<SourceIngressImportResponse, AtmError> {
    crate::boundary_support::import_inbox_source(request)
}

pub fn compute_identity_fingerprint(
    request: SourceIngressIdentityFingerprintRequest,
) -> SourceIngressIdentityFingerprintResponse {
    crate::boundary_support::compute_identity_fingerprint(request)
}

pub fn report_inbox_diagnostics(
    request: SourceIngressDiagnosticsRequest,
) -> SourceIngressDiagnosticsResponse {
    crate::boundary_support::report_inbox_diagnostics(request)
}

pub fn export_source_files(
    request: ProjectionExportRecordRequest,
) -> Result<ProjectionExportRecordResponse, AtmError> {
    crate::boundary_support::export_source_files(request)
}

pub fn reexport_messages(
    request: ProjectionExportReexportMessageRequest,
) -> Result<ProjectionExportReexportMessageResponse, AtmError> {
    // Yb Y.10 constrains full mailbox re-export to explicit repair/rebuild
    // seams. Normal runtime delivery must use append-only Claude execution or
    // the typed non-Claude outbound boundary instead.
    crate::boundary_support::reexport_messages(request)
}

pub fn append_message_set(
    request: ProjectionExportAppendMessageSetRequest,
) -> Result<ProjectionExportAppendMessageSetResponse, AtmError> {
    crate::boundary_support::append_message_set(request)
}
