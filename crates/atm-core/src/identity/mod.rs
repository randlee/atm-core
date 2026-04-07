pub mod hook;

#[cfg(test)]
use hook::HookIdentity;

use crate::config::AtmConfig;
use crate::error::AtmError;

pub fn resolve_sender_identity(config: Option<&AtmConfig>) -> Result<String, AtmError> {
    crate::config::resolve_identity(config).ok_or_else(AtmError::identity_unavailable)
}

#[cfg(test)]
pub fn resolve_hook_identity(
    team_override: Option<&str>,
    config: Option<&AtmConfig>,
) -> Result<HookIdentity, AtmError> {
    let agent = resolve_sender_identity(config)?;
    let team = crate::config::resolve_team(team_override, config)
        .ok_or_else(AtmError::team_unavailable)?;
    Ok(HookIdentity { agent, team })
}

#[cfg(test)]
mod tests {
    use std::env;

    use crate::config::AtmConfig;

    use super::{resolve_hook_identity, resolve_sender_identity};

    #[test]
    #[serial_test::serial]
    fn resolves_sender_identity_from_environment() {
        let original_identity = env::var_os("ATM_IDENTITY");
        set_env_var("ATM_IDENTITY", "arch-ctm");

        let config = AtmConfig {
            identity: Some("config-agent".into()),
            default_team: None,
            obsolete_identity_present: true,
        };
        assert_eq!(
            resolve_sender_identity(Some(&config)).expect("identity"),
            "arch-ctm"
        );

        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    #[serial_test::serial]
    fn sender_identity_does_not_fall_back_to_config_when_env_missing() {
        let original_identity = env::var_os("ATM_IDENTITY");
        remove_env_var("ATM_IDENTITY");

        let config = AtmConfig {
            identity: Some("config-agent".into()),
            default_team: None,
            obsolete_identity_present: true,
        };

        let error = resolve_sender_identity(Some(&config)).expect_err("identity error");
        assert!(error.is_identity());

        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    #[serial_test::serial]
    fn resolves_hook_identity_from_environment() {
        let original_identity = env::var_os("ATM_IDENTITY");
        let original_team = env::var_os("ATM_TEAM");
        set_env_var("ATM_IDENTITY", "arch-ctm");
        set_env_var("ATM_TEAM", "atm-dev");

        let identity = resolve_hook_identity(None, None).expect("hook identity");
        assert_eq!(identity.agent, "arch-ctm");
        assert_eq!(identity.team, "atm-dev");

        restore("ATM_IDENTITY", original_identity);
        restore("ATM_TEAM", original_team);
    }

    #[test]
    #[serial_test::serial]
    fn hook_identity_requires_runtime_identity_when_env_missing() {
        let original_identity = env::var_os("ATM_IDENTITY");
        let original_team = env::var_os("ATM_TEAM");
        remove_env_var("ATM_IDENTITY");
        set_env_var("ATM_TEAM", "");

        let config = AtmConfig {
            identity: Some("config-agent".into()),
            default_team: Some("config-team".into()),
            obsolete_identity_present: true,
        };

        let error = resolve_hook_identity(None, Some(&config)).expect_err("hook identity error");
        assert!(error.is_identity());

        restore("ATM_IDENTITY", original_identity);
        restore("ATM_TEAM", original_team);
    }

    fn restore(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => set_env_var(key, value),
            None => remove_env_var(key),
        }
    }

    fn set_env_var<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(key: K, value: V) {
        // SAFETY: these tests use serial_test to ensure process environment
        // mutations are not performed concurrently.
        unsafe { env::set_var(key, value) }
    }

    fn remove_env_var<K: AsRef<std::ffi::OsStr>>(key: K) {
        // SAFETY: these tests use serial_test to ensure process environment
        // mutations are not performed concurrently.
        unsafe { env::remove_var(key) }
    }
}
