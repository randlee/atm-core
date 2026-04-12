pub mod hook;

#[cfg(test)]
use hook::HookIdentity;

use crate::config::AtmConfig;
use crate::error::AtmError;

/// Resolve the active actor identity for commands that allow an explicit override.
///
/// # Errors
///
/// Returns [`AtmError`] with [`crate::error_codes::AtmErrorCode::IdentityUnavailable`]
/// when neither the explicit override, hook identity, nor `ATM_IDENTITY`
/// environment variable provides a sender identity.
pub(crate) fn resolve_actor_identity(
    actor_override: Option<&str>,
    config: Option<&AtmConfig>,
) -> Result<String, AtmError> {
    if let Some(actor) = actor_override.filter(|value| !value.trim().is_empty()) {
        return Ok(crate::config::aliases::resolve_agent(actor, config));
    }

    if let Some(identity) = hook::read_hook_identity()? {
        return Ok(identity);
    }

    resolve_runtime_sender_identity(config)
}

/// Resolve the sender identity for `send`, preserving sender-override and
/// alias-after-hook behavior.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`] when neither the
/// explicit override, hook identity, nor `ATM_IDENTITY` provides a sender.
pub(crate) fn resolve_sender_identity(
    sender_override: Option<&str>,
    config: Option<&AtmConfig>,
) -> Result<String, AtmError> {
    if let Some(sender) = sender_override.filter(|value| !value.trim().is_empty()) {
        return Ok(crate::config::aliases::resolve_agent(sender.trim(), config));
    }

    if let Some(identity) = hook::read_hook_identity()? {
        return Ok(crate::config::aliases::resolve_agent(&identity, config));
    }

    resolve_runtime_sender_identity(config)
        .map(|identity| crate::config::aliases::resolve_agent(&identity, config))
}

/// Resolve the canonical runtime sender identity for the current ATM process.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`] when
/// `ATM_IDENTITY` is not set in the current environment.
pub fn resolve_runtime_sender_identity(config: Option<&AtmConfig>) -> Result<String, AtmError> {
    crate::config::resolve_identity(config).ok_or_else(AtmError::identity_unavailable)
}

#[cfg(test)]
pub fn resolve_hook_identity(
    team_override: Option<&str>,
    config: Option<&AtmConfig>,
) -> Result<HookIdentity, AtmError> {
    let agent = resolve_runtime_sender_identity(config)?;
    let team = crate::config::resolve_team(team_override, config)
        .ok_or_else(AtmError::team_unavailable)?;
    Ok(HookIdentity { agent, team })
}

#[cfg(test)]
mod tests {
    use std::env;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::AtmConfig;

    use super::{resolve_hook_identity, resolve_runtime_sender_identity, resolve_sender_identity};

    #[test]
    #[serial_test::serial]
    fn resolves_sender_identity_from_environment() {
        let original_identity = env::var_os("ATM_IDENTITY");
        set_env_var("ATM_IDENTITY", "arch-ctm");

        let config = AtmConfig {
            identity: Some("config-agent".into()),
            default_team: None,
            team_members: Vec::new(),
            aliases: Default::default(),
            post_send_hook: None,
            post_send_hook_senders: Vec::new(),
            post_send_hook_recipients: Vec::new(),
            config_root: std::path::PathBuf::new(),
            obsolete_identity_present: true,
        };
        assert_eq!(
            resolve_runtime_sender_identity(Some(&config)).expect("identity"),
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
            team_members: Vec::new(),
            aliases: Default::default(),
            post_send_hook: None,
            post_send_hook_senders: Vec::new(),
            post_send_hook_recipients: Vec::new(),
            config_root: std::path::PathBuf::new(),
            obsolete_identity_present: true,
        };

        let error = resolve_runtime_sender_identity(Some(&config)).expect_err("identity error");
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
            team_members: Vec::new(),
            aliases: Default::default(),
            post_send_hook: None,
            post_send_hook_senders: Vec::new(),
            post_send_hook_recipients: Vec::new(),
            config_root: std::path::PathBuf::new(),
            obsolete_identity_present: true,
        };

        let error = resolve_hook_identity(None, Some(&config)).expect_err("hook identity error");
        assert!(error.is_identity());

        restore("ATM_IDENTITY", original_identity);
        restore("ATM_TEAM", original_team);
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn send_sender_identity_applies_alias_to_hook_identity() {
        let original_identity = env::var_os("ATM_IDENTITY");
        remove_env_var("ATM_IDENTITY");

        let hook_path =
            std::env::temp_dir().join(format!("atm-hook-{}.json", unsafe { libc::getppid() }));
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_secs_f64();
        fs::write(
            &hook_path,
            format!(r#"{{"agent_name":"lead","created_at":{created_at}}}"#),
        )
        .expect("hook file");

        let mut aliases = std::collections::BTreeMap::new();
        aliases.insert("lead".to_string(), "team-lead".to_string());
        let config = AtmConfig {
            identity: None,
            default_team: None,
            team_members: Vec::new(),
            aliases,
            post_send_hook: None,
            post_send_hook_senders: Vec::new(),
            post_send_hook_recipients: Vec::new(),
            config_root: std::path::PathBuf::new(),
            obsolete_identity_present: false,
        };

        assert_eq!(
            resolve_sender_identity(None, Some(&config)).expect("send identity"),
            "team-lead"
        );

        let _ = fs::remove_file(hook_path);
        restore("ATM_IDENTITY", original_identity);
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
