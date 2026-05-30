use atm_core::{
    boundary::{
        ConfigLoadRequest, ConfigLoadResponse, InboxExportAppendMessageSetRequest,
        InboxExportAppendMessageSetResponse, InboxExportRecordRequest, InboxExportRecordResponse,
        InboxExportReexportMessageRequest, InboxExportReexportMessageResponse,
        InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
        InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
        InboxIngressImportRequest, InboxIngressImportResponse,
    },
    error::AtmError,
};

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    atm_core::direct_boundaries::load_workspace_config(request)
}

pub(crate) fn import_inbox_source(
    request: InboxIngressImportRequest,
) -> Result<InboxIngressImportResponse, AtmError> {
    atm_core::direct_boundaries::import_inbox_source(request)
}

pub(crate) fn compute_identity_fingerprint(
    request: InboxIngressIdentityFingerprintRequest,
) -> InboxIngressIdentityFingerprintResponse {
    atm_core::direct_boundaries::compute_identity_fingerprint(request)
}

pub(crate) fn report_inbox_diagnostics(
    request: InboxIngressDiagnosticsRequest,
) -> InboxIngressDiagnosticsResponse {
    atm_core::direct_boundaries::report_inbox_diagnostics(request)
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

pub(crate) fn append_message_set(
    request: InboxExportAppendMessageSetRequest,
) -> Result<InboxExportAppendMessageSetResponse, AtmError> {
    atm_core::direct_boundaries::append_message_set(request)
}
