#![allow(
    dead_code,
    reason = "Phase AD obsolete: caller-owned context fallback is forbidden in production, but the retired helper module remains test-visible until the later deletion sprint removes it entirely."
)]

pub mod hook;

use crate::caller_context::read_cli_identity_from_env;
use crate::config::AtmConfig;
use crate::error::AtmError;
use crate::types::AgentName;

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
) -> Result<AgentName, AtmError> {
    if let Some(actor) = actor_override.filter(|value| !value.trim().is_empty()) {
        return resolve_aliased_agent(actor, config);
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
) -> Result<AgentName, AtmError> {
    if let Some(sender) = sender_override.filter(|value| !value.trim().is_empty()) {
        return resolve_aliased_agent(sender.trim(), config);
    }

    if let Some(identity) = hook::read_hook_identity()? {
        return resolve_aliased_agent(identity.as_str(), config);
    }

    resolve_runtime_sender_identity(config)
}

/// Resolve the canonical runtime sender identity for the current ATM process.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`] when
/// `ATM_IDENTITY` is not set in the current environment.
pub(crate) fn resolve_runtime_sender_identity(
    _config: Option<&AtmConfig>,
) -> Result<AgentName, AtmError> {
    read_cli_identity_from_env()?
        .map(|identity| identity.agent)
        .ok_or_else(AtmError::identity_unavailable)
}

fn resolve_aliased_agent(value: &str, config: Option<&AtmConfig>) -> Result<AgentName, AtmError> {
    crate::config::aliases::resolve_agent_name(value, config)
}

#[cfg(test)]
pub(crate) fn resolve_hook_identity(
    team_override: Option<&str>,
    config: Option<&AtmConfig>,
) -> Result<(AgentName, crate::types::TeamName), AtmError> {
    let agent = resolve_runtime_sender_identity(config)?;
    let team = crate::config::resolve_team(team_override, config)
        .ok_or_else(AtmError::team_unavailable)?;
    Ok((agent, team))
}

#[cfg(test)]
mod tests {
    use std::env;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::AtmConfig;
    use crate::test_support::{TEST_SENDER, TEST_TEAM, lock_env, remove_env_var, set_env_var};
    use crate::types::AgentName;

    #[cfg(unix)]
    use super::resolve_sender_identity;
    use super::{resolve_hook_identity, resolve_runtime_sender_identity};
    #[cfg(unix)]
    use crate::roles::ROLE_TEAM_LEAD;

    #[test]
    #[serial_test::serial(env)]
    fn resolves_sender_identity_from_environment() {
        let _env_lock = lock_env();
        let original_identity = env::var_os("ATM_IDENTITY");
        set_env_var("ATM_IDENTITY", TEST_SENDER);

        let config = AtmConfig {
            obsolete_identity: Some("config-agent".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_runtime_sender_identity(Some(&config)).expect("identity"),
            AgentName::from_validated(TEST_SENDER)
        );

        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    #[serial_test::serial(env)]
    fn sender_identity_does_not_fall_back_to_config_when_env_missing() {
        let _env_lock = lock_env();
        let original_identity = env::var_os("ATM_IDENTITY");
        remove_env_var("ATM_IDENTITY");

        let config = AtmConfig {
            obsolete_identity: Some("config-agent".into()),
            ..Default::default()
        };

        let error = resolve_runtime_sender_identity(Some(&config)).expect_err("identity error");
        assert!(matches!(
            error.code(),
            crate::error_codes::AtmErrorCode::IdentityUnavailable
                | crate::error_codes::AtmErrorCode::IdentityInvalid
                | crate::error_codes::AtmErrorCode::IdentityConflict
        ));

        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    #[serial_test::serial(env)]
    fn resolves_hook_identity_from_environment() {
        let _env_lock = lock_env();
        let original_identity = env::var_os("ATM_IDENTITY");
        let original_team = env::var_os("ATM_TEAM");
        set_env_var("ATM_IDENTITY", TEST_SENDER);
        set_env_var("ATM_TEAM", TEST_TEAM);

        let (agent, team) = resolve_hook_identity(None, None).expect("hook identity");
        assert_eq!(agent.as_str(), TEST_SENDER);
        assert_eq!(team.as_str(), TEST_TEAM);

        restore("ATM_IDENTITY", original_identity);
        restore("ATM_TEAM", original_team);
    }

    #[test]
    #[serial_test::serial(env)]
    fn hook_identity_requires_runtime_identity_when_env_missing() {
        let _env_lock = lock_env();
        let original_identity = env::var_os("ATM_IDENTITY");
        let original_team = env::var_os("ATM_TEAM");
        remove_env_var("ATM_IDENTITY");
        set_env_var("ATM_TEAM", "");

        let config = AtmConfig {
            obsolete_identity: Some("config-agent".into()),
            default_team: Some("config-team".parse().expect("team")),
            ..Default::default()
        };

        let error = resolve_hook_identity(None, Some(&config)).expect_err("hook identity error");
        assert!(matches!(
            error.code(),
            crate::error_codes::AtmErrorCode::IdentityUnavailable
                | crate::error_codes::AtmErrorCode::IdentityInvalid
                | crate::error_codes::AtmErrorCode::IdentityConflict
        ));

        restore("ATM_IDENTITY", original_identity);
        restore("ATM_TEAM", original_team);
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn send_sender_identity_applies_alias_to_hook_identity() {
        let _env_lock = lock_env();
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
        aliases.insert("lead".to_string(), ROLE_TEAM_LEAD.to_string());
        let config = AtmConfig {
            aliases,
            ..Default::default()
        };

        assert_eq!(
            resolve_sender_identity(None, Some(&config)).expect("send identity"),
            AgentName::from_validated(ROLE_TEAM_LEAD)
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
}
