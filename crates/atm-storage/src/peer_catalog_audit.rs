//! Pure audit of a trusted-peer catalog against durable-hostname enforcement.
//!
//! This module owns only the read-only classification of trusted-peer rows.
//! It performs no storage I/O, TLS work, or CLI mutation; callers use the
//! resulting audit to decide startup policy or render operator remediation.

use crate::contract::TrustedPeer;
use crate::types::HostName;

/// Partitions a trusted-peer catalog by durable-hostname status and
/// enablement, isolating legacy literal-IP authority rows that predate
/// [`HostName::is_durable_hostname`] enforcement.
///
/// # Examples
/// ```
/// use atm_storage::TrustedPeerCatalogAudit;
///
/// let audit = TrustedPeerCatalogAudit::from_peers(&[]);
/// assert!(!audit.has_legacy_literal_ip_rows());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedPeerCatalogAudit {
    durable_enabled: Vec<HostName>,
    durable_disabled: Vec<HostName>,
    legacy_literal_enabled: Vec<HostName>,
    legacy_literal_disabled: Vec<HostName>,
}

impl TrustedPeerCatalogAudit {
    /// Classifies every row in `peers` without mutating or reordering them.
    #[must_use]
    pub fn from_peers(peers: &[TrustedPeer]) -> Self {
        let mut audit = Self::default();
        for peer in peers {
            let durable = peer.host.is_durable_hostname();
            let bucket = match (durable, peer.enabled) {
                (true, true) => &mut audit.durable_enabled,
                (true, false) => &mut audit.durable_disabled,
                (false, true) => &mut audit.legacy_literal_enabled,
                (false, false) => &mut audit.legacy_literal_disabled,
            };
            bucket.push(peer.host.clone());
        }
        audit
    }

    /// Durable-hostname peers currently enabled for outbound/inbound mTLS.
    #[must_use]
    pub fn durable_enabled_hosts(&self) -> &[HostName] {
        &self.durable_enabled
    }

    /// Legacy literal-IP rows that are enabled and would otherwise be used
    /// as a live mTLS peer authority.
    #[must_use]
    pub fn legacy_literal_enabled_hosts(&self) -> &[HostName] {
        &self.legacy_literal_enabled
    }

    /// Legacy literal-IP rows that are disabled and therefore historical
    /// only; they must never block an otherwise-valid configuration.
    #[must_use]
    pub fn legacy_literal_disabled_hosts(&self) -> &[HostName] {
        &self.legacy_literal_disabled
    }

    /// True when the catalog contains any legacy literal-IP row, enabled or
    /// disabled.
    #[must_use]
    pub fn has_legacy_literal_ip_rows(&self) -> bool {
        !self.legacy_literal_enabled.is_empty() || !self.legacy_literal_disabled.is_empty()
    }

    /// The exact, safe command that converts `host` (a legacy literal-IP
    /// authority) to a durable `target_hostname`, preserving its fingerprint
    /// and port. `target_hostname` may be a literal placeholder such as
    /// `<hostname>` when rendering guidance rather than an executable command.
    #[must_use]
    pub fn migrate_command(host: &HostName, target_hostname: &str) -> String {
        format!("atm peer trust migrate --map {host}={target_hostname} --yes")
    }

    /// The exact, safe command that retires `host` from the trusted-peer
    /// catalog without touching any other row.
    #[must_use]
    pub fn revoke_command(host: &HostName) -> String {
        format!("atm peer trust revoke --host {host} --yes")
    }

    /// Renders one remediation line per legacy literal-IP row (enabled and
    /// disabled), naming the host and its exact safe migrate/revoke commands.
    /// Returns an empty string when no legacy literal-IP rows are present.
    #[must_use]
    pub fn remediation_text(&self) -> String {
        let mut lines = Vec::new();
        for host in &self.legacy_literal_enabled {
            lines.push(format!(
                "{host} (enabled): migrate with `{}` or retire with `{}`",
                Self::migrate_command(host, "<hostname>"),
                Self::revoke_command(host)
            ));
        }
        for host in &self.legacy_literal_disabled {
            lines.push(format!(
                "{host} (disabled): safe to prune with `{}`",
                Self::revoke_command(host)
            ));
        }
        lines.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU16;

    fn peer(host: &str, enabled: bool) -> TrustedPeer {
        TrustedPeer {
            host: host.parse().expect("host"),
            fingerprint: "sha256:test".parse().expect("fingerprint"),
            enabled,
            https_port: NonZeroU16::new(443).expect("port"),
        }
    }

    #[test]
    fn empty_catalog_has_no_legacy_rows() {
        let audit = TrustedPeerCatalogAudit::from_peers(&[]);
        assert!(!audit.has_legacy_literal_ip_rows());
        assert_eq!(audit.remediation_text(), "");
    }

    #[test]
    fn mixed_catalog_partitions_every_bucket() {
        let peers = vec![
            peer("rand-m5.local", true),
            peer("rand-m4.local", false),
            peer("192.168.128.29", true),
            peer("10.0.0.5", false),
        ];
        let audit = TrustedPeerCatalogAudit::from_peers(&peers);
        assert_eq!(
            audit.durable_enabled_hosts(),
            &["rand-m5.local".parse::<HostName>().expect("host")]
        );
        assert_eq!(audit.durable_disabled.len(), 1);
        assert_eq!(
            audit.legacy_literal_enabled_hosts(),
            &["192.168.128.29".parse::<HostName>().expect("host")]
        );
        assert_eq!(
            audit.legacy_literal_disabled_hosts(),
            &["10.0.0.5".parse::<HostName>().expect("host")]
        );
        assert!(audit.has_legacy_literal_ip_rows());
    }

    #[test]
    fn remediation_text_names_every_offending_host_with_exact_commands() {
        let peers = vec![peer("192.168.128.29", true), peer("10.0.0.5", false)];
        let audit = TrustedPeerCatalogAudit::from_peers(&peers);
        let text = audit.remediation_text();
        assert!(text.contains("192.168.128.29"));
        assert!(text.contains("atm peer trust migrate --map 192.168.128.29=<hostname> --yes"));
        assert!(text.contains("atm peer trust revoke --host 192.168.128.29 --yes"));
        assert!(text.contains("10.0.0.5"));
        assert!(text.contains("atm peer trust revoke --host 10.0.0.5 --yes"));
    }

    #[test]
    fn migrate_and_revoke_commands_render_exact_cli_invocations() {
        let host: HostName = "192.168.128.29".parse().expect("host");
        assert_eq!(
            TrustedPeerCatalogAudit::migrate_command(&host, "rand-m5.local"),
            "atm peer trust migrate --map 192.168.128.29=rand-m5.local --yes"
        );
        assert_eq!(
            TrustedPeerCatalogAudit::revoke_command(&host),
            "atm peer trust revoke --host 192.168.128.29 --yes"
        );
    }
}
