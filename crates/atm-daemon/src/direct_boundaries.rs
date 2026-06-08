use atm_core::{
    boundary::{
        ConfigLoadRequest, ConfigLoadResponse, ProjectionAppendMessageSetRequest,
        ProjectionAppendMessageSetResponse, ProjectionRecordRequest, ProjectionRecordResponse,
        ProjectionReexportMessageRequest, ProjectionReexportMessageResponse,
        SourceDiagnosticsRequest, SourceDiagnosticsResponse, SourceIdentityFingerprintRequest,
        SourceIdentityFingerprintResponse, SourceImportRequest, SourceImportResponse,
    },
    error::AtmError,
};

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    atm_core::direct_boundaries::load_workspace_config(request)
}

pub(crate) fn import_inbox_source(
    request: SourceImportRequest,
) -> Result<SourceImportResponse, AtmError> {
    atm_core::direct_boundaries::import_inbox_source(request)
}

pub(crate) fn compute_identity_fingerprint(
    request: SourceIdentityFingerprintRequest,
) -> SourceIdentityFingerprintResponse {
    atm_core::direct_boundaries::compute_identity_fingerprint(request)
}

pub(crate) fn report_inbox_diagnostics(
    request: SourceDiagnosticsRequest,
) -> SourceDiagnosticsResponse {
    atm_core::direct_boundaries::report_inbox_diagnostics(request)
}

pub(crate) fn export_source_files(
    request: ProjectionRecordRequest,
) -> Result<ProjectionRecordResponse, AtmError> {
    atm_core::direct_boundaries::export_source_files(request)
}

pub(crate) fn reexport_messages(
    request: ProjectionReexportMessageRequest,
) -> Result<ProjectionReexportMessageResponse, AtmError> {
    atm_core::direct_boundaries::reexport_messages(request)
}

pub(crate) fn append_message_set(
    request: ProjectionAppendMessageSetRequest,
) -> Result<ProjectionAppendMessageSetResponse, AtmError> {
    atm_core::direct_boundaries::append_message_set(request)
}
