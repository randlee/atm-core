use crate::boundary::{
    ConfigLoadRequest, ConfigLoadResponse, InboxExportAppendMessageSetRequest,
    InboxExportAppendMessageSetResponse, InboxExportRecordRequest, InboxExportRecordResponse,
    InboxExportReexportMessageRequest, InboxExportReexportMessageResponse,
    InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
    InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
    InboxIngressImportRequest, InboxIngressImportResponse,
};
use crate::error::AtmError;

pub fn load_workspace_config(request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError> {
    crate::boundary_support::load_workspace_config(request)
}

pub fn import_inbox_source(
    request: InboxIngressImportRequest,
) -> Result<InboxIngressImportResponse, AtmError> {
    crate::boundary_support::import_inbox_source(request)
}

pub fn compute_identity_fingerprint(
    request: InboxIngressIdentityFingerprintRequest,
) -> InboxIngressIdentityFingerprintResponse {
    crate::boundary_support::compute_identity_fingerprint(request)
}

pub fn report_inbox_diagnostics(
    request: InboxIngressDiagnosticsRequest,
) -> InboxIngressDiagnosticsResponse {
    crate::boundary_support::report_inbox_diagnostics(request)
}

pub fn export_source_files(
    request: InboxExportRecordRequest,
) -> Result<InboxExportRecordResponse, AtmError> {
    crate::boundary_support::export_source_files(request)
}

pub fn reexport_messages(
    request: InboxExportReexportMessageRequest,
) -> Result<InboxExportReexportMessageResponse, AtmError> {
    // Yb Y.10 constrains full mailbox re-export to explicit repair/rebuild
    // seams. Normal runtime delivery must use append-only Claude execution or
    // the typed non-Claude outbound boundary instead.
    crate::boundary_support::reexport_messages(request)
}

pub fn append_message_set(
    request: InboxExportAppendMessageSetRequest,
) -> Result<InboxExportAppendMessageSetResponse, AtmError> {
    crate::boundary_support::append_message_set(request)
}
