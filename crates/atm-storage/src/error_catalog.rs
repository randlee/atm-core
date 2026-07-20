//! Canonical rendering for ATM-owned errors.

use crate::error_codes::AtmErrorCode;

/// Renders the sole user-visible error string for an ATM error code.
///
/// Adapters may provide bounded, non-secret detail, but the catalog owns the
/// recovery guidance so no protocol or adapter creates another error shape.
pub(crate) fn render(code: AtmErrorCode, detail: impl Into<String>) -> String {
    format!("{}\n  Recovery: {}", detail.into(), guidance(code))
}

pub(crate) fn render_code(code: AtmErrorCode) -> String {
    guidance(code).to_string()
}

const fn guidance(code: AtmErrorCode) -> &'static str {
    match code {
        AtmErrorCode::ConfigHomeUnavailable
        | AtmErrorCode::AtmHomeUnresolved
        | AtmErrorCode::ConfigParseFailed
        | AtmErrorCode::ConfigRetiredHookMembersKey
        | AtmErrorCode::ConfigRetiredLegacyHookKeys
        | AtmErrorCode::ConfigTeamParseFailed
        | AtmErrorCode::ConfigTeamMissing => "Repair the active ATM configuration and retry.",
        AtmErrorCode::IdentityUnavailable
        | AtmErrorCode::IdentityInvalid
        | AtmErrorCode::IdentityConflict => "Set a valid ATM identity before retrying.",
        AtmErrorCode::MemberAlreadyExists | AtmErrorCode::MemberNotFound => {
            "Confirm the target team and member before retrying."
        }
        AtmErrorCode::DaemonUnavailable
        | AtmErrorCode::RuntimeRootInvalid
        | AtmErrorCode::RuntimeBootstrapRefused
        | AtmErrorCode::SocketOverrideForbidden
        | AtmErrorCode::DaemonMayHaveExecuted
        | AtmErrorCode::DaemonLifecycleWedge
        | AtmErrorCode::DaemonLaunchGateRejected
        | AtmErrorCode::DaemonServingStateRejected
        | AtmErrorCode::DaemonStaleOwnerRecoveryFailed
        | AtmErrorCode::DaemonAutoStartFailed
        | AtmErrorCode::DaemonConnectionSaturated
        | AtmErrorCode::ClientDaemonVersionIncompatible
        | AtmErrorCode::DaemonAdvisorySessionAlreadyRegistered
        | AtmErrorCode::DaemonAdvisorySessionNotRegistered
        | AtmErrorCode::DaemonAdvisorySessionCleanupFailed => {
            "Ensure atm-daemon binary is installed, then restore the single local daemon to a healthy serving state and retry."
        }
        AtmErrorCode::AddressParseFailed
        | AtmErrorCode::TeamUnavailable
        | AtmErrorCode::TeamInvalid
        | AtmErrorCode::TeamNotFound
        | AtmErrorCode::AgentNotFound => "Correct the ATM address or team selection and retry.",
        AtmErrorCode::MailboxReadFailed
        | AtmErrorCode::MailboxWriteFailed
        | AtmErrorCode::MailboxLockFailed
        | AtmErrorCode::MailboxLockReadOnlyFilesystem
        | AtmErrorCode::MailboxLockTimeout => {
            "Repair mailbox access or wait for the competing operation, then retry."
        }
        AtmErrorCode::InternalError => "Inspect the logged diagnostic context before retrying.",
        AtmErrorCode::MessageValidationFailed
        | AtmErrorCode::SelfAddressedSendInvalid
        | AtmErrorCode::EmptyNudgeTemplateBody
        | AtmErrorCode::CallerContextRequestInvalid
        | AtmErrorCode::AckInvalidState
        | AtmErrorCode::ClearInvalidState => {
            "Correct the invalid ATM request or state before retrying."
        }
        AtmErrorCode::SerializationFailed => {
            "Repair the invalid serialized ATM data before retrying."
        }
        AtmErrorCode::FilePolicyRejected | AtmErrorCode::FileReferenceRewriteFailed => {
            "Correct the referenced path or file-policy input before retrying."
        }
        AtmErrorCode::WaitTimeout => "Retry after the bounded operation becomes available.",
        AtmErrorCode::ObservabilityEmitFailed
        | AtmErrorCode::ObservabilityQueryFailed
        | AtmErrorCode::ObservabilityFollowFailed
        | AtmErrorCode::ObservabilityHealthFailed
        | AtmErrorCode::ObservabilityBootstrapFailed => {
            "Repair the observability backend or retry the operation later."
        }
        AtmErrorCode::ObservabilityHealthOk => "No operator action is required.",
        AtmErrorCode::WarningInvalidTeamMemberSkipped
        | AtmErrorCode::WarningMailboxRecordSkipped
        | AtmErrorCode::WarningMalformedAtmFieldIgnored
        | AtmErrorCode::WarningObservabilityHealthDegraded
        | AtmErrorCode::WarningSqliteHealthDegraded
        | AtmErrorCode::WarningOriginInboxEntrySkipped
        | AtmErrorCode::WarningMissingTeamConfigFallback
        | AtmErrorCode::WarningSendAlertStateDegraded
        | AtmErrorCode::WarningIdentityDrift
        | AtmErrorCode::WarningRosterDrift
        | AtmErrorCode::WarningBaselineMemberMissing
        | AtmErrorCode::WarningRestoreInProgress
        | AtmErrorCode::WarningStaleMailboxLock
        | AtmErrorCode::WarningHookSkipped
        | AtmErrorCode::WarningHookExecutionFailed => {
            "Inspect the warning context and correct the reported condition."
        }
        AtmErrorCode::PostSendPaneMissing
        | AtmErrorCode::PostSendTmuxSendFailed
        | AtmErrorCode::PostSendGraftUnavailable
        | AtmErrorCode::PostSendAdvisoryDeliveryFailed => {
            "Repair the configured post-send target and retry if delivery is required."
        }
        AtmErrorCode::TestFakeTransportInjectionFailed => {
            "Repair the test transport fixture before retrying."
        }
        AtmErrorCode::HelpTopicNotFound => "Use `atm help --list` to inspect available topics.",
    }
}
