pub mod hook;

pub use hook::HookIdentity;

use crate::config::AtmConfig;
use crate::error::Error;

pub fn resolve_sender_identity(config: Option<&AtmConfig>) -> Result<String, Error> {
    crate::config::resolve_identity(config).ok_or(Error::IdentityUnavailable)
}

pub fn resolve_hook_identity(
    team_override: Option<&str>,
    config: Option<&AtmConfig>,
) -> Result<HookIdentity, Error> {
    let agent = resolve_sender_identity(config)?;
    let team = crate::config::resolve_team(team_override, config).ok_or(Error::TeamUnavailable)?;
    Ok(HookIdentity { agent, team })
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::sync::{Mutex, OnceLock};

    use crate::config::AtmConfig;

    use super::{resolve_hook_identity, resolve_sender_identity};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn resolves_sender_identity_from_environment() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let original_identity = env::var_os("ATM_IDENTITY");
        env::set_var("ATM_IDENTITY", "arch-ctm");

        let config = AtmConfig {
            identity: Some("config-agent".into()),
            default_team: None,
        };
        assert_eq!(
            resolve_sender_identity(Some(&config)).expect("identity"),
            "arch-ctm"
        );

        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    fn resolves_sender_identity_from_config_when_env_missing() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let original_identity = env::var_os("ATM_IDENTITY");
        env::set_var("ATM_IDENTITY", "");

        let config = AtmConfig {
            identity: Some("config-agent".into()),
            default_team: None,
        };
        assert_eq!(
            resolve_sender_identity(Some(&config)).expect("identity"),
            "config-agent"
        );

        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    fn resolves_hook_identity_from_environment() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let original_identity = env::var_os("ATM_IDENTITY");
        let original_team = env::var_os("ATM_TEAM");
        env::set_var("ATM_IDENTITY", "arch-ctm");
        env::set_var("ATM_TEAM", "atm-dev");

        let identity = resolve_hook_identity(None, None).expect("hook identity");
        assert_eq!(identity.agent, "arch-ctm");
        assert_eq!(identity.team, "atm-dev");

        restore("ATM_IDENTITY", original_identity);
        restore("ATM_TEAM", original_team);
    }

    #[test]
    fn resolves_hook_identity_from_config_when_env_missing() {
        let _guard = env_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let original_identity = env::var_os("ATM_IDENTITY");
        let original_team = env::var_os("ATM_TEAM");
        env::set_var("ATM_IDENTITY", "");
        env::set_var("ATM_TEAM", "");

        let config = AtmConfig {
            identity: Some("config-agent".into()),
            default_team: Some("config-team".into()),
        };

        let identity = resolve_hook_identity(None, Some(&config)).expect("hook identity");
        assert_eq!(identity.agent, "config-agent");
        assert_eq!(identity.team, "config-team");

        restore("ATM_IDENTITY", original_identity);
        restore("ATM_TEAM", original_team);
    }

    fn restore(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
        }
    }
}
