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
/// Internal atomic persistence helpers for shared mutable state files.
pub(crate) mod persistence;
/// Internal process-liveness helpers shared across lock implementations.
pub(crate) mod process;
/// Mailbox read/query workflows and output models.
pub mod read;
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
/// Internal text-formatting helpers used by ATM core surfaces.
pub(crate) mod text;
/// Shared enums and semantic newtypes used across ATM core workflows.
pub mod types;
/// File-watch and reconcile boundary contracts for daemon/runtime layers.
pub mod watcher_reconcile;
/// Internal ATM-owned workflow-state helpers shared across mailbox services.
pub(crate) mod workflow;
