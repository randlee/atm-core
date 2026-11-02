//! atm-bd framework contract. `hook`: what task hooks read, decide and write. `monitor`: collect ->
//! decide -> execute. `wire`: ATM's hook request and reply JSON. Plug-ins add hooks, templates and a
//! policy; this crate names no profile.
pub mod hook;
pub mod monitor;
pub mod wire;

pub use hook::*;
pub use monitor::*;
pub use wire::*;
