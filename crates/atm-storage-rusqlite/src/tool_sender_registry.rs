//! SQLite inspection adapter for persisted plug-in sender registrations.
//!
//! Signature/trust-root verification is deliberately owned by the runtime
//! admission layer. This adapter only projects a previously persisted active
//! registration through the sealed inspection issuer.

use std::sync::Arc;

use atm_storage::contract::sealed;
use atm_storage::{
    AgentAddress, AtmError, AtmErrorCode, AtmMessageId, IsoTimestamp, MessageDigest,
    PluginSenderId, RegisteredToolSender, RegisteredToolSenderIssuer, ToolInstallReceipt,
    ToolInstallerRequest, ToolPrincipal, ToolRuntimeAdmission, ToolRuntimeProof, ToolSenderFailure,
    ToolSenderRegistry,
};
use rusqlite::{OptionalExtension, params};

use crate::shared_db::SharedDb;
use crate::store_support::internal;

struct SqliteToolSenderRegistry {
    db: Arc<SharedDb>,
}

/// Private read-only row record. Its seal grants inspection projection only;
/// it cannot mint a runtime admission.
struct SqliteRegisteredToolSenderIssuer {
    principal: ToolPrincipal,
    address: AgentAddress,
}

impl sealed::Sealed for SqliteRegisteredToolSenderIssuer {}

impl RegisteredToolSenderIssuer for SqliteRegisteredToolSenderIssuer {
    fn principal(&self) -> &ToolPrincipal {
        &self.principal
    }

    fn address(&self) -> &AgentAddress {
        &self.address
    }
}

pub(crate) fn tool_sender_registry(db: Arc<SharedDb>) -> Arc<dyn ToolSenderRegistry + Send + Sync> {
    Arc::new(SqliteToolSenderRegistry { db })
}

impl sealed::Sealed for SqliteToolSenderRegistry {}

fn trust_root_required() -> AtmError {
    ToolSenderFailure::SignatureInvalid
        .into_error("tool sender verification requires the runtime-owned installer trust root")
}

impl ToolSenderRegistry for SqliteToolSenderRegistry {
    fn apply_signed(
        &self,
        _request: ToolInstallerRequest,
        _now: IsoTimestamp,
    ) -> Result<ToolInstallReceipt, AtmError> {
        // Persisting an unverified installer envelope would manufacture a
        // trusted sender. BK10 has no trust-root input, so fail closed.
        Err(trust_root_required())
    }

    fn admit_signed_message(
        &self,
        _proof: ToolRuntimeProof,
        _message_id: AtmMessageId,
        _digest: MessageDigest,
        _now: IsoTimestamp,
    ) -> Result<ToolRuntimeAdmission, AtmError> {
        Err(trust_root_required())
    }

    fn resolve(&self, sender: &PluginSenderId) -> Result<RegisteredToolSender, AtmError> {
        let sender = sender.clone();
        self.db.read(move |connection| {
            let active: Option<i64> = connection
                .query_row(
                    "SELECT active FROM tool_sender_registry WHERE sender = ?1",
                    params![sender.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| {
                    AtmError::new(
                        AtmErrorCode::StoreQueryFailed,
                        format!("failed to read tool sender registration: {error}"),
                    )
                })?;
            if active != Some(1) {
                return Err(ToolSenderFailure::SenderUnregistered.into_error(format!(
                    "tool sender {sender} is not registered by an authenticated installer"
                )));
            }
            let address: AgentAddress = format!("{}@tool", sender.as_str())
                .parse()
                .map_err(|error| internal("tool_sender_registry", error))?;
            let issuer = SqliteRegisteredToolSenderIssuer {
                principal: ToolPrincipal::RegisteredPlugin { sender },
                address,
            };
            Ok(RegisteredToolSender::from_issuer(&issuer))
        })
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    use super::*;
    use crate::SqliteStorageBackend;

    #[test]
    fn resolve_projects_only_an_active_persisted_registration() {
        let root = tempdir().expect("tempdir");
        let path = root.path().join("mail.db");
        let backend = SqliteStorageBackend::new(&path).expect("backend");
        let connection = Connection::open(&path).expect("connection");
        connection.execute(
            "INSERT INTO tool_sender_registry (sender, installer_id, installer_key_id, host_id, runtime_key_id, runtime_public_key, registration_generation, active, installed_at) VALUES (?1, 'installer', 'installer-key', 'host.example', 'runtime-key', 'public', 1, 1, '2026-10-01T00:00:00Z')",
            params!["nightly"],
        ).expect("registration");
        drop(connection);

        let sender = PluginSenderId::parse("nightly").expect("sender");
        let resolved = backend
            .tool_sender_registry()
            .resolve(&sender)
            .expect("resolve");
        assert_eq!(
            resolved.principal(),
            &ToolPrincipal::RegisteredPlugin { sender }
        );
        assert_eq!(resolved.address().to_string(), "nightly@tool");
    }

    #[test]
    fn inactive_registration_is_not_an_inspection_sender() {
        let root = tempdir().expect("tempdir");
        let backend = SqliteStorageBackend::new(root.path().join("mail.db")).expect("backend");
        let missing = PluginSenderId::parse("missing").expect("sender");
        let error = backend
            .tool_sender_registry()
            .resolve(&missing)
            .expect_err("missing sender");
        assert_eq!(error.code(), AtmErrorCode::ToolSenderUnregistered);
    }
}
