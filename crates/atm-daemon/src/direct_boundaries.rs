use atm_core::{
    boundary::{ConfigLoadRequest, ConfigLoadResponse},
    error::AtmError,
    load_atm_config,
};
use atm_storage_claude::compat::{
    ProjectionExportAppendMessageSetRequest, ProjectionExportAppendMessageSetResponse,
    ProjectionExportRecordRequest, ProjectionExportRecordResponse,
    ProjectionExportReexportMessageRequest, ProjectionExportReexportMessageResponse,
    SourceIngressDiagnosticsRequest, SourceIngressDiagnosticsResponse,
    SourceIngressIdentityFingerprintRequest, SourceIngressIdentityFingerprintResponse,
    SourceIngressImportRequest, SourceIngressImportResponse,
};

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    Ok(ConfigLoadResponse {
        config: load_atm_config(&request.current_dir)?,
    })
}

pub(crate) fn import_inbox_source(
    request: SourceIngressImportRequest,
) -> Result<SourceIngressImportResponse, AtmError> {
    atm_storage_claude::compat::import_inbox_source(request)
}

pub(crate) fn compute_identity_fingerprint(
    request: SourceIngressIdentityFingerprintRequest,
) -> SourceIngressIdentityFingerprintResponse {
    atm_storage_claude::compat::compute_identity_fingerprint(request)
}

pub(crate) fn report_inbox_diagnostics(
    request: SourceIngressDiagnosticsRequest,
) -> SourceIngressDiagnosticsResponse {
    atm_storage_claude::compat::report_inbox_diagnostics(request)
}

pub(crate) fn export_source_files(
    request: ProjectionExportRecordRequest,
) -> Result<ProjectionExportRecordResponse, AtmError> {
    atm_storage_claude::compat::export_source_files(request)
}

pub(crate) fn reexport_messages(
    request: ProjectionExportReexportMessageRequest,
) -> Result<ProjectionExportReexportMessageResponse, AtmError> {
    atm_storage_claude::compat::reexport_messages(request)
}

pub(crate) fn append_message_set(
    request: ProjectionExportAppendMessageSetRequest,
) -> Result<ProjectionExportAppendMessageSetResponse, AtmError> {
    atm_storage_claude::compat::append_message_set(request)
}
