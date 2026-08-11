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
    if let Some(message) = configuration_guidance(code) {
        return message;
    }
    if let Some(message) = identity_guidance(code) {
        return message;
    }
    if let Some(message) = daemon_guidance(code) {
        return message;
    }
    if let Some(message) = local_http_guidance(code) {
        return message;
    }
    if let Some(message) = mailbox_guidance(code) {
        return message;
    }
    if let Some(message) = request_guidance(code) {
        return message;
    }
    if let Some(message) = observability_guidance(code) {
        return message;
    }
    if let Some(message) = warning_guidance(code) {
        return message;
    }
    if let Some(message) = post_send_guidance(code) {
        return message;
    }

    match code {
        AtmErrorCode::InternalError => "Inspect the logged diagnostic context before retrying.",
        AtmErrorCode::SerializationFailed => {
            "Repair the invalid serialized ATM data before retrying."
        }
        AtmErrorCode::FilePolicyRejected | AtmErrorCode::FileReferenceRewriteFailed => {
            "Correct the referenced path or file-policy input before retrying."
        }
        AtmErrorCode::WaitTimeout => "Retry after the bounded operation becomes available.",
        AtmErrorCode::TestFakeTransportInjectionFailed => {
            "Repair the test transport fixture before retrying."
        }
        AtmErrorCode::HelpTopicNotFound => "Use `atm help --list` to inspect available topics.",
        _ => "Inspect the logged diagnostic context before retrying.",
    }
}

const fn configuration_guidance(code: AtmErrorCode) -> Option<&'static str> {
    match code {
        AtmErrorCode::ConfigHomeUnavailable
        | AtmErrorCode::AtmHomeUnresolved
        | AtmErrorCode::ConfigParseFailed
        | AtmErrorCode::ConfigRetiredHookMembersKey
        | AtmErrorCode::ConfigRetiredLegacyHookKeys
        | AtmErrorCode::ConfigTeamParseFailed
        | AtmErrorCode::ConfigTeamMissing => Some("Repair the active ATM configuration and retry."),
        AtmErrorCode::IdentityUnavailable
        | AtmErrorCode::IdentityInvalid
        | AtmErrorCode::IdentityConflict => Some("Set a valid ATM identity before retrying."),
        AtmErrorCode::MemberAlreadyExists | AtmErrorCode::MemberNotFound => {
            Some("Confirm the target team and member before retrying.")
        }
        _ => None,
    }
}

const fn identity_guidance(code: AtmErrorCode) -> Option<&'static str> {
    match code {
        AtmErrorCode::AddressParseFailed
        | AtmErrorCode::TeamUnavailable
        | AtmErrorCode::TeamInvalid
        | AtmErrorCode::TeamNotFound
        | AtmErrorCode::AgentNotFound => {
            Some("Correct the ATM address or team selection and retry.")
        }
        _ => None,
    }
}

const fn daemon_guidance(code: AtmErrorCode) -> Option<&'static str> {
    match code {
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
        | AtmErrorCode::PeerConfigValidationFailed
        | AtmErrorCode::CertificateOperationFailed
        | AtmErrorCode::BindPreflightFailed
        | AtmErrorCode::ClientDaemonVersionIncompatible
        | AtmErrorCode::DaemonAdvisorySessionAlreadyRegistered
        | AtmErrorCode::DaemonAdvisorySessionNotRegistered
        | AtmErrorCode::DaemonAdvisorySessionCleanupFailed => Some(
            "Ensure atm-daemon binary is installed, then restore the single local daemon to a healthy serving state and retry.",
        ),
        AtmErrorCode::DaemonConnectionSaturated => {
            Some("Wait for the daemon to finish an in-flight request, then retry.")
        }
        _ => None,
    }
}

const fn mailbox_guidance(code: AtmErrorCode) -> Option<&'static str> {
    match code {
        AtmErrorCode::MailboxReadFailed
        | AtmErrorCode::MailboxWriteFailed
        | AtmErrorCode::MailboxLockFailed
        | AtmErrorCode::MailboxLockReadOnlyFilesystem
        | AtmErrorCode::MailboxLockTimeout => {
            Some("Repair mailbox access or wait for the competing operation, then retry.")
        }
        _ => None,
    }
}

const fn request_guidance(code: AtmErrorCode) -> Option<&'static str> {
    match code {
        AtmErrorCode::MessageValidationFailed
        | AtmErrorCode::LocalHttpCapabilityInvalid
        | AtmErrorCode::LocalHttpEndpointSchemaUnsupported
        | AtmErrorCode::LocalHttpEndpointMissing
        | AtmErrorCode::LocalHttpEndpointNonLoopback
        | AtmErrorCode::LocalHttpRuntimeDirectoryMissing
        | AtmErrorCode::MessageIdConflict
        | AtmErrorCode::SelfAddressedSendInvalid
        | AtmErrorCode::EmptyNudgeTemplateBody
        | AtmErrorCode::CallerContextRequestInvalid
        | AtmErrorCode::AckInvalidState
        | AtmErrorCode::ClearInvalidState => {
            Some("Correct the invalid ATM request or state before retrying.")
        }
        AtmErrorCode::DecomposedTemplateIncludeForbidden => Some(
            "Remove template dependencies or use the confined render operation before retrying.",
        ),
        _ => None,
    }
}

const fn local_http_guidance(code: AtmErrorCode) -> Option<&'static str> {
    match code {
        AtmErrorCode::LocalHttpCapabilityRevoked => Some(
            "Re-read local endpoint metadata and authenticate with the newly published capability.",
        ),
        _ => None,
    }
}

const fn observability_guidance(code: AtmErrorCode) -> Option<&'static str> {
    match code {
        AtmErrorCode::ObservabilityEmitFailed
        | AtmErrorCode::ObservabilityQueryFailed
        | AtmErrorCode::ObservabilityFollowFailed
        | AtmErrorCode::ObservabilityHealthFailed
        | AtmErrorCode::ObservabilityBootstrapFailed => {
            Some("Repair the observability backend or retry the operation later.")
        }
        AtmErrorCode::ObservabilityHealthOk => Some("No operator action is required."),
        _ => None,
    }
}

const fn warning_guidance(code: AtmErrorCode) -> Option<&'static str> {
    match code {
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
            Some("Inspect the warning context and correct the reported condition.")
        }
        _ => None,
    }
}

const fn post_send_guidance(code: AtmErrorCode) -> Option<&'static str> {
    match code {
        AtmErrorCode::PostSendPaneMissing
        | AtmErrorCode::PostSendTmuxSendFailed
        | AtmErrorCode::PostSendGraftUnavailable
        | AtmErrorCode::GraftReceiverAlreadyActive
        | AtmErrorCode::PostSendAdvisoryDeliveryFailed => {
            Some("Repair the configured post-send target and retry if delivery is required.")
        }
        _ => None,
    }
}
