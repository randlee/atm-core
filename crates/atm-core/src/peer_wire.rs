//! Transport-neutral peer-wire policy vocabulary.
//!
//! This module owns the typed mode selected at the daemon launch boundary. It
//! deliberately carries no TLS adapter, certificate, environment, or runtime
//! availability detail: those are implementation concerns of later adapters.

/// The security policy selected for the daemon's peer-wire adapter.
///
/// `Mtls` is the normal process mode. `PlaintextTest` is an explicit,
/// non-durable diagnostic and benchmark mode; it never represents peer
/// authentication or authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeerWireSecurity {
    /// Normal cross-host transport: mutual TLS and exact peer authorization.
    #[default]
    Mtls,
    /// Explicit untrusted diagnostic mode using the preserved direct TCP path.
    PlaintextTest,
}

impl PeerWireSecurity {
    /// Stable daemon-launch spelling for this security mode.
    #[must_use]
    pub const fn as_launch_value(self) -> &'static str {
        match self {
            Self::Mtls => "mutual-tls",
            Self::PlaintextTest => "plaintext-test",
        }
    }
}

/// The complete transport-neutral peer-wire policy chosen for one daemon run.
///
/// AO.3 owns parsing this value from `atm-daemon --peer-wire-security`; this
/// contract intentionally has no environment or configuration constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeerWireMode {
    /// The selected wire-security policy.
    pub security: PeerWireSecurity,
}

impl PeerWireMode {
    /// Returns the normal daemon policy for an invocation with no explicit
    /// launch-mode override.
    #[must_use]
    pub const fn mtls() -> Self {
        Self {
            security: PeerWireSecurity::Mtls,
        }
    }

    /// Returns the explicit untrusted diagnostic/benchmark policy.
    #[must_use]
    pub const fn plaintext_test() -> Self {
        Self {
            security: PeerWireSecurity::PlaintextTest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PeerWireMode, PeerWireSecurity};

    #[test]
    fn normal_mode_defaults_to_mutual_tls() {
        assert_eq!(PeerWireMode::default(), PeerWireMode::mtls());
        assert_eq!(PeerWireSecurity::default(), PeerWireSecurity::Mtls);
    }

    #[test]
    fn mode_vocabulary_has_stable_daemon_launch_spellings() {
        assert_eq!(PeerWireSecurity::Mtls.as_launch_value(), "mutual-tls");
        assert_eq!(
            PeerWireSecurity::PlaintextTest.as_launch_value(),
            "plaintext-test"
        );
    }

    #[test]
    fn plaintext_test_remains_an_explicit_distinct_mode() {
        assert_ne!(PeerWireMode::plaintext_test(), PeerWireMode::mtls());
    }
}
