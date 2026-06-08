use atm_core::{
    boundary::{
        ConfigLoadRequest, ConfigLoadResponse, ProjectionExportAppendMessageSetRequest,
        ProjectionExportAppendMessageSetResponse, ProjectionExportRecordRequest,
        ProjectionExportRecordResponse, ProjectionExportReexportMessageRequest,
        ProjectionExportReexportMessageResponse, SourceIngressDiagnosticsRequest,
        SourceIngressDiagnosticsResponse, SourceIngressIdentityFingerprintRequest,
        SourceIngressIdentityFingerprintResponse, SourceIngressImportRequest,
        SourceIngressImportResponse,
    },
    error::AtmError,
};

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    atm_core::direct_boundaries::load_workspace_config(request)
}

pub(crate) fn import_inbox_source(
    request: SourceIngressImportRequest,
) -> Result<SourceIngressImportResponse, AtmError> {
    atm_core::direct_boundaries::import_inbox_source(request)
}

pub(crate) fn compute_identity_fingerprint(
    request: SourceIngressIdentityFingerprintRequest,
) -> SourceIngressIdentityFingerprintResponse {
    atm_core::direct_boundaries::compute_identity_fingerprint(request)
}

pub(crate) fn report_inbox_diagnostics(
    request: SourceIngressDiagnosticsRequest,
) -> SourceIngressDiagnosticsResponse {
    atm_core::direct_boundaries::report_inbox_diagnostics(request)
}

pub(crate) fn export_source_files(
    request: ProjectionExportRecordRequest,
) -> Result<ProjectionExportRecordResponse, AtmError> {
    atm_core::direct_boundaries::export_source_files(request)
}

pub(crate) fn reexport_messages(
    request: ProjectionExportReexportMessageRequest,
) -> Result<ProjectionExportReexportMessageResponse, AtmError> {
    atm_core::direct_boundaries::reexport_messages(request)
}

pub(crate) fn append_message_set(
    request: ProjectionExportAppendMessageSetRequest,
) -> Result<ProjectionExportAppendMessageSetResponse, AtmError> {
    atm_core::direct_boundaries::append_message_set(request)
}
