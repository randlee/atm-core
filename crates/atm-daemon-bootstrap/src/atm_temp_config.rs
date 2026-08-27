//! `.atm.toml` sweep-interval/TTL threading into daemon composition
//! (ADR-055 decision (b), QM43-I5).

use atm_core::AtmConfig;
use atm_core::atm_temp::EnvSource;

/// Loads `.atm.toml`'s sweep-interval/TTL configuration (ADR-055 decision
/// (b)/QM43-I5) from the user's home directory, never from the daemon's
/// current working directory.
///
/// `assemble_daemon_runtime`'s doc comment explains why the daemon's config
/// doctor deliberately does not depend on a *workspace-relative* `.atm.toml`
/// (a LaunchAgent's `getcwd(2)` can block). The user's home directory is a
/// different, fixed OS concept -- the same one `~/.atm/transfer` and
/// `AtmConfig.local_host`'s documented setup already anchor to -- so reading
/// `$HOME/.atm.toml` here does not reintroduce that hazard. Returns `None`
/// when the home directory cannot be resolved or no `.atm.toml` exists
/// there; callers fall back to `AtmConfig::default()`'s compiled-in ADR-055
/// defaults (1 hour / 30 days), exactly as before this threading landed.
pub(crate) fn daemon_atm_config(env: &dyn EnvSource) -> Option<AtmConfig> {
    let home = atm_core::home::resolve_user_home_via(env)?;
    atm_core::load_atm_config(&home).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::daemon_atm_config;
    use std::collections::HashMap;

    struct FixedEnvSource(HashMap<&'static str, String>);

    impl atm_core::atm_temp::EnvSource for FixedEnvSource {
        fn var(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    /// QM43-I5: sweep-interval/TTL threading reads `$HOME/.atm.toml`, not a
    /// workspace-relative config -- `assemble_daemon_runtime`'s doc comment
    /// explains why the daemon must not depend on its (possibly-blocking)
    /// working directory for config discovery.
    #[test]
    fn daemon_atm_config_reads_sweep_settings_from_the_home_directory() {
        let home = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            home.path().join(".atm.toml"),
            "[atm]\nsweep_interval_seconds = 120\nsweep_ttl_days = 7\n",
        )
        .expect("write .atm.toml");
        let mut vars = HashMap::new();
        vars.insert("HOME", home.path().to_str().expect("utf8 path").to_string());
        let config = daemon_atm_config(&FixedEnvSource(vars)).expect("config resolves");
        assert_eq!(config.sweep_interval_seconds, 120);
        assert_eq!(config.sweep_ttl_days, 7);
    }

    #[test]
    fn daemon_atm_config_is_none_without_a_home_atm_toml() {
        let home = tempfile::tempdir().expect("tempdir");
        let mut vars = HashMap::new();
        vars.insert("HOME", home.path().to_str().expect("utf8 path").to_string());
        assert_eq!(daemon_atm_config(&FixedEnvSource(vars)), None);
    }

    #[test]
    fn daemon_atm_config_is_none_when_home_is_unresolvable() {
        assert_eq!(daemon_atm_config(&FixedEnvSource(HashMap::new())), None);
    }
}
