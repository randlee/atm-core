//! Shared validation for write provenance at transport and persistence seams.

use crate::error::AtmError;
use crate::types::HostName;

/// Identifies the ingress policy that admitted a write request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteIngress {
    /// A canonical local writer or an already-normalized internal request.
    Canonical,
    /// A request received through the local IPC/TCP boundary.
    Local,
    /// A request received through authenticated mutual TLS.
    Peer,
    /// A request received through the explicitly opt-in plaintext smoke path.
    UntrustedSmoke,
    /// A plaintext diagnostic request that cannot submit writes.
    AnonymousSmoke,
}

/// The provenance fields relevant to one write admission decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteProvenance<'a> {
    pub target_host: Option<&'a HostName>,
    pub authenticated_source_host: Option<&'a HostName>,
    pub origin_message_id: bool,
    pub origin_timestamp: bool,
}

/// Validated provenance facts shared by self-send, delivery-policy, ACK, and
/// daemon ingress decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedWriteProvenance {
    target_host_qualified: bool,
    authenticated_peer: bool,
    origin_metadata: bool,
}

impl ValidatedWriteProvenance {
    #[must_use]
    pub const fn target_host_qualified(self) -> bool {
        self.target_host_qualified
    }

    #[must_use]
    pub const fn is_authenticated_peer(self) -> bool {
        self.authenticated_peer
    }

    #[must_use]
    pub const fn has_origin_metadata(self) -> bool {
        self.origin_metadata
    }

    /// A host-qualified request without authenticated peer provenance is an
    /// origin write that must be admitted as a remote target.
    #[must_use]
    pub const fn is_remote_origin(self) -> bool {
        self.target_host_qualified && !self.authenticated_peer
    }
}

/// Validate all write provenance combinations in one place.
pub fn validate_write_provenance(
    ingress: WriteIngress,
    provenance: WriteProvenance<'_>,
) -> Result<ValidatedWriteProvenance, AtmError> {
    let has_any_origin_metadata = provenance.origin_message_id || provenance.origin_timestamp;
    let has_complete_origin_metadata = provenance.origin_message_id && provenance.origin_timestamp;
    if has_any_origin_metadata
        && !has_complete_origin_metadata
        && !matches!(ingress, WriteIngress::Canonical)
    {
        return Err(AtmError::validation(
            "write provenance requires both origin_message_id and origin_timestamp; preserve the immutable origin metadata as a pair",
        ));
    }
    let authenticated_peer = provenance.authenticated_source_host.is_some();
    if authenticated_peer && !has_complete_origin_metadata {
        return Err(AtmError::validation(
            "authenticated peer writes require source host, origin_message_id, and origin_timestamp; retry through the authenticated peer adapter",
        ));
    }

    match ingress {
        WriteIngress::Canonical => {}
        WriteIngress::Local if authenticated_peer || has_any_origin_metadata => {
            return Err(AtmError::validation(
                "local write requests must not supply authenticated peer provenance or origin metadata",
            ));
        }
        WriteIngress::Peer if !authenticated_peer || !has_complete_origin_metadata => {
            return Err(AtmError::validation(
                "peer write requests require authenticated source provenance and immutable origin metadata",
            ));
        }
        WriteIngress::UntrustedSmoke if authenticated_peer || !has_complete_origin_metadata => {
            return Err(AtmError::validation(
                "plaintext smoke ingress must carry origin metadata but no authenticated peer identity",
            ));
        }
        WriteIngress::AnonymousSmoke => {
            return Err(AtmError::validation(
                "anonymous plaintext diagnostics cannot submit writes; include explicit peer provenance or use authenticated HTTPS",
            ));
        }
        WriteIngress::Local | WriteIngress::Peer | WriteIngress::UntrustedSmoke => {}
    }

    Ok(ValidatedWriteProvenance {
        target_host_qualified: provenance.target_host.is_some(),
        authenticated_peer,
        origin_metadata: has_complete_origin_metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::{WriteIngress, WriteProvenance, validate_write_provenance};
    use crate::types::HostName;

    fn provenance(
        target_host: bool,
        authenticated_source_host: bool,
        origin_message_id: bool,
        origin_timestamp: bool,
    ) -> WriteProvenance<'static> {
        let target: &'static HostName = Box::leak(Box::new(
            "peer.example.test".parse::<HostName>().expect("host"),
        ));
        WriteProvenance {
            target_host: target_host.then_some(target),
            authenticated_source_host: authenticated_source_host.then_some(target),
            origin_message_id,
            origin_timestamp,
        }
    }

    #[test]
    fn ingress_matrix_accepts_only_complete_provenance() {
        assert!(
            validate_write_provenance(WriteIngress::Local, provenance(false, false, false, false))
                .is_ok()
        );
        assert!(
            validate_write_provenance(WriteIngress::Peer, provenance(true, true, true, true))
                .is_ok()
        );
        assert!(
            validate_write_provenance(
                WriteIngress::UntrustedSmoke,
                provenance(false, false, true, true)
            )
            .is_ok()
        );
        assert!(
            validate_write_provenance(
                WriteIngress::AnonymousSmoke,
                provenance(false, false, false, false)
            )
            .is_err()
        );
    }

    #[test]
    fn partial_or_forged_provenance_is_rejected_with_actionable_error() {
        for input in [
            provenance(false, true, true, false),
            provenance(false, true, false, false),
            provenance(false, false, true, false),
        ] {
            let error = validate_write_provenance(WriteIngress::Peer, input)
                .expect_err("partial provenance must fail closed");
            assert!(!error.message().is_empty());
        }
        assert!(
            validate_write_provenance(WriteIngress::Local, provenance(false, true, true, true))
                .is_err()
        );
        assert!(
            validate_write_provenance(
                WriteIngress::UntrustedSmoke,
                provenance(false, true, true, true)
            )
            .is_err()
        );
        assert!(
            validate_write_provenance(WriteIngress::Peer, provenance(false, false, true, true))
                .is_err()
        );
    }

    #[test]
    fn remote_origin_is_host_qualified_without_authenticated_source() {
        let validated = validate_write_provenance(
            WriteIngress::Canonical,
            provenance(true, false, false, false),
        )
        .expect("origin target is valid");
        assert!(validated.is_remote_origin());
        assert!(!validated.is_authenticated_peer());
    }
}
