// Keep the implementation in bin_support/, but make the dependency ownership
// explicit in the in-tree shim so cargo-shear can see the package-level use.
use sc_observability as _;

#[cfg(test)]
const _: Option<fn(sc_observability::Logger)> = None;

include!("../bin_support/daemon_observability.rs");
