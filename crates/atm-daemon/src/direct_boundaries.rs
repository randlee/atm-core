use atm_core::{
    boundary::{
        ConfigLoadRequest, ConfigLoadResponse, ConfigTeamLoadRequest, ConfigTeamLoadResponse,
        InboxExportRecordRequest, InboxExportRecordResponse, InboxExportReexportMessageRequest,
        InboxExportReexportMessageResponse, InboxIngressDiagnosticsRequest,
        InboxIngressDiagnosticsResponse, InboxIngressIdentityFingerprintRequest,
        InboxIngressIdentityFingerprintResponse, InboxIngressImportRequest,
        InboxIngressImportResponse,
    },
    error::AtmError,
};

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    atm_core::direct_boundaries::load_workspace_config(request)
}

pub(crate) fn load_team_config(
    request: ConfigTeamLoadRequest,
) -> Result<ConfigTeamLoadResponse, AtmError> {
    atm_core::direct_boundaries::load_team_config(request)
}

pub(crate) fn import_inbox_source(
    request: InboxIngressImportRequest,
) -> Result<InboxIngressImportResponse, AtmError> {
    atm_core::direct_boundaries::import_inbox_source(request)
}

pub(crate) fn compute_identity_fingerprint(
    request: InboxIngressIdentityFingerprintRequest,
) -> Result<InboxIngressIdentityFingerprintResponse, AtmError> {
    Ok(atm_core::direct_boundaries::compute_identity_fingerprint(
        request,
    ))
}

pub(crate) fn report_inbox_diagnostics(
    request: InboxIngressDiagnosticsRequest,
) -> Result<InboxIngressDiagnosticsResponse, AtmError> {
    Ok(atm_core::direct_boundaries::report_inbox_diagnostics(
        request,
    ))
}

pub(crate) fn export_source_files(
    request: InboxExportRecordRequest,
) -> Result<InboxExportRecordResponse, AtmError> {
    atm_core::direct_boundaries::export_source_files(request)
}

pub(crate) fn reexport_messages(
    request: InboxExportReexportMessageRequest,
) -> Result<InboxExportReexportMessageResponse, AtmError> {
    atm_core::direct_boundaries::reexport_messages(request)
}
