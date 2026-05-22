use crate::boundary::{
    ConfigLoadRequest, ConfigLoadResponse, InboxExportAppendMessageSetRequest,
    InboxExportAppendMessageSetResponse, InboxExportRecordRequest, InboxExportRecordResponse,
    InboxExportReexportMessageRequest, InboxExportReexportMessageResponse,
    InboxIngressDiagnosticsRequest, InboxIngressDiagnosticsResponse,
    InboxIngressIdentityFingerprintRequest, InboxIngressIdentityFingerprintResponse,
    InboxIngressImportRequest, InboxIngressImportResponse, RosterStore,
};
use crate::error::AtmError;
use crate::types::TeamName;
use std::path::Path;

pub fn load_workspace_config(request: ConfigLoadRequest) -> Result<ConfigLoadResponse, AtmError> {
    crate::boundary_support::load_workspace_config(request)
}

pub fn hydrate_roster_from_team_config_once_at_startup_if_empty(
    home_dir: &Path,
    team: &TeamName,
    roster_store: &dyn RosterStore,
) -> Result<bool, AtmError> {
    crate::boundary_support::hydrate_roster_from_team_config_once_at_startup_if_empty(
        home_dir,
        team,
        roster_store,
    )
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
