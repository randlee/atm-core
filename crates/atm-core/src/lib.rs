/// Acknowledgement workflows for ack-required mailbox messages.
pub mod ack;
/// Public agent-address parsing and normalization helpers.
pub mod address;
/// Mailbox cleanup workflows for read and acknowledged messages.
pub mod clear;
/// Internal configuration discovery and resolution helpers.
pub(crate) mod config;
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
/// Log query and filtering types for the CLI log surface.
pub mod log;
/// Internal mailbox persistence and parsing helpers.
pub(crate) mod mailbox;
/// Internal model-registry plumbing reserved for follow-on work.
pub(crate) mod model_registry;
/// Observability adapter traits and event payload types.
pub mod observability;
/// Internal atomic persistence helpers for shared mutable state files.
pub(crate) mod persistence;
/// Mailbox read/query workflows and output models.
pub mod read;
/// Public mailbox and team schema types shared with CLI tests and adapters.
pub mod schema;
/// Mailbox send workflows and request/response models.
pub mod send;
/// Retained local team discovery, roster repair, and backup/restore workflows.
pub mod team_admin;
/// Internal text-formatting helpers used by ATM core surfaces.
pub(crate) mod text;
/// Shared enums and semantic newtypes used across ATM core workflows.
pub mod types;
