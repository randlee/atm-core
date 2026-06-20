#![allow(
    deprecated,
    reason = "Phase AC keeps the shared storage traits as a transitional contract while Claude storage remains the first backend implementation"
)]

mod backend;
pub mod compat;
mod mailbox;
mod paths;
mod roster;
