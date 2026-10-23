//! Local signed installer admission for virtual tool senders.
//!
//! The HTTP boundary checks method, connector provenance, and operation shape;
//! signature, expiry, host policy, generation, and nonce replay are verified
//! atomically by the installed registry adapter.

use std::sync::Arc;

use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::error::AtmError;
use atm_core::protocol::{ResponseEnvelope, ToolInstallerRequest};
use atm_core::send::{ToolInstallerOperation, ToolSenderRegistry, WriteRequest};
use atm_core::send::{ToolRuntimeProof, ToolSenderAttestation};
use atm_core::types::IsoTimestamp;

use super::StorageAndNudgeRouter;

impl StorageAndNudgeRouter {
    pub(super) async fn admit_tool_write(
        &self,
        request: &WriteRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<Option<atm_core::boundary::ToolSenderCapability>, AtmError> {
        match (
            &request.tool_runtime_proof,
            &request.tool_sender_attestation,
        ) {
            (None, None) => Ok(None),
            (Some(_), Some(_)) => Err(AtmError::validation(
                "a write cannot combine local runtime proof and peer sender attestation",
            )),
            (Some(proof), None) => self
                .admit_local_tool_write(request, proof, ingress, deadline)
                .await
                .map(Some),
            (None, Some(attestation)) => self
                .admit_peer_tool_write(request, attestation, ingress, deadline)
                .await
                .map(Some),
        }
    }

    async fn admit_local_tool_write(
        &self,
        request: &WriteRequest,
        proof: &ToolRuntimeProof,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<atm_core::boundary::ToolSenderCapability, AtmError> {
        require_local(ingress)?;
        if request.origin_message_id.is_some()
            || request.origin_timestamp.is_some()
            || request.authenticated_source_host.is_some()
        {
            return Err(AtmError::validation(
                "local tool runtime proof cannot override peer origin metadata",
            ));
        }
        let registry = self.hooks.tool_senders.clone().ok_or_else(|| {
            AtmError::daemon_unavailable_with_recovery(
                "tool sender registry is unavailable for runtime proof admission",
                "Install the protected tool sender registry during daemon startup before accepting virtual sender writes.",
            )
        })?;
        let proof = proof.clone();
        let admission = self
            .control_path_sync_bridge
            .run(deadline, move || {
                let message_id = proof.message_id;
                let digest = proof.message_digest.clone();
                registry.admit_signed_message(proof, message_id, digest, IsoTimestamp::now())
            })
            .await?;
        match atm_core::boundary::claim_verified_tool_runtime(admission) {
            atm_core::boundary::SenderClaim::DaemonTool { proof, .. } => Ok(proof),
            atm_core::boundary::SenderClaim::Ordinary { .. } => Err(AtmError::new(
                atm_core::error::AtmErrorCode::InternalError,
                "verified runtime admission produced an ordinary sender claim",
            )),
        }
    }

    async fn admit_peer_tool_write(
        &self,
        request: &WriteRequest,
        attestation: &ToolSenderAttestation,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<atm_core::boundary::ToolSenderCapability, AtmError> {
        if ingress != AuthenticatedIngress::Peer {
            return Err(AtmError::validation_with_recovery(
                "tool sender attestation is accepted only on authenticated peer ingress",
                "forward the attested message through its authenticated origin-host connection",
            ));
        }
        let source_host = request.authenticated_source_host.clone().ok_or_else(|| {
            AtmError::validation("peer tool sender proof has no authenticated source host")
        })?;
        let message_id = request.origin_message_id.ok_or_else(|| {
            AtmError::validation("peer tool sender proof has no stable origin message id")
        })?;
        if request.origin_timestamp.is_none() {
            return Err(AtmError::validation(
                "peer tool sender proof has no immutable origin timestamp",
            ));
        }
        let store = self.hooks.tool_sender_peers.clone().ok_or_else(|| {
            AtmError::daemon_unavailable_with_recovery(
                "peer tool sender admission is unavailable because no origin policy was installed",
                "Install protected origin-host attestation policy during daemon startup before accepting peer virtual sender writes.",
            )
        })?;
        let attestation = attestation.clone();
        let digest = attestation.message_digest.clone();
        let admission = self
            .control_path_sync_bridge
            .run(deadline, move || {
                store.admit_attestation(
                    attestation,
                    message_id,
                    digest,
                    &source_host,
                    IsoTimestamp::now(),
                )
            })
            .await?;
        match atm_core::boundary::claim_verified_tool_runtime(admission) {
            atm_core::boundary::SenderClaim::DaemonTool { proof, .. } => Ok(proof),
            atm_core::boundary::SenderClaim::Ordinary { .. } => Err(AtmError::new(
                atm_core::error::AtmErrorCode::InternalError,
                "verified peer admission produced an ordinary sender claim",
            )),
        }
    }

    pub(super) async fn tool_sender_apply(
        &self,
        request: ToolInstallerRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        if !matches!(
            &request.operation,
            ToolInstallerOperation::Register | ToolInstallerOperation::Rotate
        ) {
            return Err(AtmError::validation(
                "POST /v1/atm/tool-senders accepts register or rotate operations",
            ));
        }
        let registry = self.tool_sender_registry()?;
        let receipt = self
            .control_path_sync_bridge
            .run(deadline, move || {
                registry.apply_signed(request, IsoTimestamp::now())
            })
            .await?;
        Ok(ApiResponse::new(ResponseEnvelope::ToolSenderApplied(
            receipt,
        )))
    }

    pub(super) async fn tool_sender_unregister(
        &self,
        request: ToolInstallerRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        if request.operation != ToolInstallerOperation::Unregister {
            return Err(AtmError::validation(
                "DELETE /v1/atm/tool-senders accepts only unregister operations",
            ));
        }
        let registry = self.tool_sender_registry()?;
        let receipt = self
            .control_path_sync_bridge
            .run(deadline, move || {
                registry.apply_signed(request, IsoTimestamp::now())
            })
            .await?;
        Ok(ApiResponse::new(ResponseEnvelope::ToolSenderApplied(
            receipt,
        )))
    }

    fn tool_sender_registry(&self) -> Result<Arc<dyn ToolSenderRegistry>, AtmError> {
        self.hooks.tool_senders.clone().ok_or_else(|| {
            AtmError::daemon_unavailable_with_recovery(
                "tool sender registry is unavailable because daemon startup did not install it",
                "Install the protected tool sender registry during daemon startup before serving installer routes, then retry.",
            )
            .with_cause("StorageAndNudgeRouter::with_tool_sender_registry was not called")
        })
    }
}

fn require_local(ingress: AuthenticatedIngress) -> Result<(), AtmError> {
    if ingress == AuthenticatedIngress::Local {
        Ok(())
    } else {
        Err(AtmError::validation_with_recovery(
            format!(
                "tool sender installer route rejects {ingress:?} ingress; it requires an authenticated local HTTP adapter"
            ),
            "Send the request through the authenticated local HTTP adapter.",
        )
        .with_cause(format!("request arrived through {ingress:?} ingress")))
    }
}

#[cfg(test)]
mod tests {
    use atm_core::api::AuthenticatedIngress;
    use atm_core::protocol::ToolInstallerRequest;
    use atm_core::send::ToolInstallerOperation;

    use crate::hook_filter::test_support::deadline;
    use crate::storage_and_nudge_router::tests::fixture;

    use super::StorageAndNudgeRouter;

    fn installer_request(operation: ToolInstallerOperation) -> ToolInstallerRequest {
        serde_json::from_value(serde_json::json!({
            "schema_version": "1",
            "operation": match operation {
                ToolInstallerOperation::Register => "register",
                ToolInstallerOperation::Rotate => "rotate",
                ToolInstallerOperation::Unregister => "unregister",
            },
            "installer_id": "installer",
            "installer_key_id": "installer-key",
            "sender": "plugin",
            "package_digest": "package-digest",
            "registration_generation": 1,
            "host_id": "host.example",
            "runtime_key_id": "runtime-key",
            "runtime_public_key": "public-key",
            "nonce": "nonce",
            "issued_at": "2026-01-02T04:03:05Z",
            "expires_at": "2026-01-02T04:04:05Z",
            "signature": "opaque-signature"
        }))
        .expect("installer request")
    }

    fn router() -> StorageAndNudgeRouter {
        fixture(true, None, None).router
    }

    #[tokio::test]
    async fn peer_cannot_use_the_local_installer_endpoint() {
        let error = router()
            .tool_sender_apply(
                installer_request(ToolInstallerOperation::Register),
                AuthenticatedIngress::Peer,
                deadline(),
            )
            .await
            .expect_err("peer ingress is refused before registry access");

        assert!(error.message().contains("rejects Peer ingress"));
    }

    #[tokio::test]
    async fn post_rejects_unregister_and_delete_rejects_rotation() {
        let router = router();
        let post_error = router
            .tool_sender_apply(
                installer_request(ToolInstallerOperation::Unregister),
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect_err("POST only accepts register or rotate");
        assert!(post_error.message().contains("accepts register or rotate"));

        let delete_error = router
            .tool_sender_unregister(
                installer_request(ToolInstallerOperation::Rotate),
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect_err("DELETE only accepts unregister");
        assert!(delete_error.message().contains("accepts only unregister"));
    }

    #[tokio::test]
    async fn accepted_local_operation_fails_closed_without_registry() {
        let error = router()
            .tool_sender_apply(
                installer_request(ToolInstallerOperation::Register),
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect_err("an unconfigured daemon has no installer admission port");

        assert_eq!(
            error.code(),
            atm_core::error::AtmErrorCode::DaemonUnavailable
        );
        assert!(error.message().contains("registry is unavailable"));
    }
}
