//! atm-bd framework contract, split at the boundary.
//! `plugin`: what a profile plug-in may use: read-only facts, `Rule`, `Params`, the `Action`s a rule returns,
//! the task-hook types, and the `Profile` manifest. A plug-in decides every meaningful operator state change.
//! `framework`: the same for every profile: policy loading, the decision record, the view, executor outcomes,
//! ports, the hook Errors tables. The framework executes decisions; it never makes one. This crate names no profile.
mod hook;
mod wire;

pub mod framework;
pub mod plugin;
