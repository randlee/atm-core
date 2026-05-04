/// Acknowledgement workflows for ack-required mailbox messages.
pub mod ack;
/// Public agent-address parsing and normalization helpers.
pub mod address;
/// Mailbox cleanup workflows for read and acknowledged messages.
pub mod clear;
/// Internal configuration discovery and resolution helpers.
pub(crate) mod config;
/// Daemon request/response dispatch contracts shared across transports.
pub mod dispatcher;
/// Doctor-report types and health checks for the CLI surface.
pub mod doctor;
/// Shared ATM error types and recovery-oriented error helpers.
pub mod error;
/// Stable ATM-owned error-code registry used by core and CLI layers.
pub mod error_codes;
/// Public ATM home and team-path resolution helpers.
pub mod home;
/// Internal identity resolution and hook lookup helpers.
pub(crate) mod identity;
/// Inbox-export helpers projecting ATM-authored rows back to Claude inbox files.
pub mod inbox_export;
/// Inbox-ingest helpers importing shared inbox rows into SQLite truth.
pub mod inbox_ingress;
/// Log query and filtering types for the CLI log surface.
pub mod log;
/// Durable message-store contracts and records.
pub mod mail_store;
/// Internal mailbox persistence and parsing helpers.
pub(crate) mod mailbox;
pub use mailbox::{read_messages, write_messages};
/// Internal model-registry plumbing reserved for follow-on work.
pub(crate) mod model_registry;
/// Observability adapter traits and event payload types.
pub mod observability;
/// Shared observability envelope types used by CLI and daemon adapters.
pub use sc_observability_types;
/// Internal atomic persistence helpers for shared mutable state files.
pub(crate) mod persistence;
/// Internal process-liveness helpers shared across lock implementations.
pub(crate) mod process;
/// Mailbox read/query workflows and output models.
pub mod read;
/// Reserved production role constants shared across runtime and tests.
pub mod roles;
/// Durable roster-store contracts and records.
pub mod roster_store;
/// Public mailbox and team schema types shared with CLI tests and adapters.
pub mod schema;
/// Mailbox send workflows and request/response models.
pub mod send;
/// Shared store newtypes, typed errors, and bootstrap/health contracts.
pub mod store;
/// Durable task-store contracts and records.
pub mod task_store;
/// Retained local team discovery, roster repair, and backup/restore workflows.
pub mod team_admin;
/// Team-config ingress helpers projecting roster state into SQLite truth.
pub mod team_ingress;
/// Shared synthetic test identities and role constants used across crate tests.
#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
/// Internal text-formatting helpers used by ATM core surfaces.
pub(crate) mod text;
/// Shared enums and semantic newtypes used across ATM core workflows.
pub mod types;
/// File-watch and reconcile boundary contracts for daemon/runtime layers.
pub mod watcher_reconcile;
/// Internal ATM-owned workflow-state helpers shared across mailbox services.
pub(crate) mod workflow;

#[cfg(test)]
#[doc(hidden)]
pub mod internal_test_hooks {
    use crate::ack::{ScopedReplyAtmMessageIdOverride, ScopedReplyMessageIdOverride};
    use crate::mailbox::lock::ScopedDebugTimeoutOverride;
    use crate::mailbox::lock::ScopedNonContentionLockErrorOverride;
    use crate::mailbox::source::ScopedSourceDiscoveryFaultOverride;
    use crate::schema::{AtmMessageId, LegacyMessageId};
    use crate::team_admin::{
        ScopedRestoreInboxStageFailureOverride, ScopedRestoreMarkerRemoveFailureOverride,
        ScopedTeamConfigWriteFailureOverride,
    };

    pub struct NonContentionLockErrorGuard {
        _inner: ScopedNonContentionLockErrorOverride,
    }

    impl NonContentionLockErrorGuard {
        pub fn enable() -> Self {
            Self {
                _inner: ScopedNonContentionLockErrorOverride::enable(),
            }
        }
    }

    pub struct SourceDiscoveryFaultGuard {
        _inner: ScopedSourceDiscoveryFaultOverride,
    }

    impl SourceDiscoveryFaultGuard {
        pub fn enable() -> Self {
            Self {
                _inner: ScopedSourceDiscoveryFaultOverride::enable(),
            }
        }
    }

    pub struct DebugMailboxLockTimeoutOverrideGuard {
        _inner: ScopedDebugTimeoutOverride,
    }

    impl DebugMailboxLockTimeoutOverrideGuard {
        pub fn set(timeout_ms: u64) -> Self {
            Self {
                _inner: ScopedDebugTimeoutOverride::set(timeout_ms),
            }
        }
    }

    pub struct ReplyMessageIdOverrideGuard {
        _inner: ScopedReplyMessageIdOverride,
    }

    impl ReplyMessageIdOverrideGuard {
        pub fn set(message_id: LegacyMessageId) -> Self {
            Self {
                _inner: ScopedReplyMessageIdOverride::set(message_id),
            }
        }
    }

    pub struct ReplyAtmMessageIdOverrideGuard {
        _inner: ScopedReplyAtmMessageIdOverride,
    }

    impl ReplyAtmMessageIdOverrideGuard {
        pub fn set(message_id: AtmMessageId) -> Self {
            Self {
                _inner: ScopedReplyAtmMessageIdOverride::set(message_id),
            }
        }
    }

    pub struct TeamConfigWriteFailureGuard {
        _inner: ScopedTeamConfigWriteFailureOverride,
    }

    impl TeamConfigWriteFailureGuard {
        pub fn enable() -> Self {
            Self {
                _inner: ScopedTeamConfigWriteFailureOverride::enable(),
            }
        }
    }

    pub struct RestoreInboxStageFailureGuard {
        _inner: ScopedRestoreInboxStageFailureOverride,
    }

    impl RestoreInboxStageFailureGuard {
        pub fn enable() -> Self {
            Self {
                _inner: ScopedRestoreInboxStageFailureOverride::enable(),
            }
        }
    }

    pub struct RestoreMarkerRemoveFailureGuard {
        _inner: ScopedRestoreMarkerRemoveFailureOverride,
    }

    impl RestoreMarkerRemoveFailureGuard {
        pub fn enable() -> Self {
            Self {
                _inner: ScopedRestoreMarkerRemoveFailureOverride::enable(),
            }
        }
    }
}
