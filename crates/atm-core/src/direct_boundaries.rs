use crate::boundary::{
    ConfigLoadRequest, ConfigLoadResponse, ProjectionAppendMessageSetRequest,
    ProjectionAppendMessageSetResponse, ProjectionRecordRequest, ProjectionRecordResponse,
    ProjectionReexportMessageRequest, ProjectionReexportMessageResponse,
    SourceDiagnosticsRequest, SourceDiagnosticsResponse, SourceIdentityFingerprintRequest,
    SourceIdentityFingerprintResponse, SourceImportRequest, SourceImportResponse,
};
use crate::error::AtmError;

pub fn load_workspace_config(request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError> {
    crate::boundary_support::load_workspace_config(request)
}

pub fn import_inbox_source(
    request: SourceImportRequest,
) -> Result<SourceImportResponse, AtmError> {
    crate::boundary_support::import_inbox_source(request)
}

pub fn compute_identity_fingerprint(
    request: SourceIdentityFingerprintRequest,
) -> SourceIdentityFingerprintResponse {
    crate::boundary_support::compute_identity_fingerprint(request)
}

pub fn report_inbox_diagnostics(
    request: SourceDiagnosticsRequest,
) -> SourceDiagnosticsResponse {
    crate::boundary_support::report_inbox_diagnostics(request)
}

pub fn export_source_files(
    request: ProjectionRecordRequest,
) -> Result<ProjectionRecordResponse, AtmError> {
    crate::boundary_support::export_source_files(request)
}

pub fn reexport_messages(
    request: ProjectionReexportMessageRequest,
) -> Result<ProjectionReexportMessageResponse, AtmError> {
    // Yb Y.10 constrains full mailbox re-export to explicit repair/rebuild
    // seams. Normal runtime delivery must use append-only Claude execution or
    // the typed non-Claude outbound boundary instead.
    crate::boundary_support::reexport_messages(request)
}

pub fn append_message_set(
    request: ProjectionAppendMessageSetRequest,
) -> Result<ProjectionAppendMessageSetResponse, AtmError> {
    crate::boundary_support::append_message_set(request)
}
