use std::env;

use crate::error::AtmError;
use crate::types::{AgentIdentity, AgentName, ChatId, TeamName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerContext {
    pub caller_identity: AgentName,
    pub caller_chat_id: Option<ChatId>,
    pub caller_team: TeamName,
}

#[derive(Debug, Clone, Copy)]
pub struct CallerIdentityOverride<'a>(pub &'a str);

#[derive(Debug, Clone, Copy)]
pub struct CallerTeamOverride<'a>(pub &'a str);

#[derive(Debug, Clone, Copy)]
pub struct CallerChatIdOverride<'a>(pub &'a str);

#[derive(Debug, Clone, Copy, Default)]
pub struct CallerContextOverrides<'a> {
    pub identity_override: Option<CallerIdentityOverride<'a>>,
    pub chat_id_override: Option<CallerChatIdOverride<'a>>,
    pub team_override: Option<CallerTeamOverride<'a>>,
}

pub fn resolve_cli_inspection_caller_context(
    overrides: CallerContextOverrides<'_>,
) -> Result<CallerContext, AtmError> {
    if overrides.identity_override.is_some() && overrides.chat_id_override.is_some() {
        return Err(AtmError::validation(
            "--as and --chat-id are mutually exclusive",
        ));
    }
    let ambient_identity = resolve_identity_component(None)?;
    let caller_identity =
        resolve_identity_component(overrides.identity_override.map(|value| value.0))?;
    let caller_chat_id = resolve_caller_chat_id(
        overrides.identity_override.map(|_| &caller_identity),
        overrides
            .chat_id_override
            .map(|value| value.0.parse())
            .transpose()?
            .as_ref(),
        read_env_raw("ATM_CHAT_ID")?.as_deref(),
        &ambient_identity,
    )?;
    let caller_team = resolve_team_component(overrides.team_override.map(|value| value.0))?;
    Ok(CallerContext {
        caller_identity: caller_identity.agent,
        caller_chat_id,
        caller_team,
    })
}

/// Resolve the one ambient chat identity with CLI overrides taking precedence.
pub fn resolve_caller_chat_id(
    explicit_as: Option<&AgentIdentity>,
    explicit_chat_id: Option<&ChatId>,
    ambient_chat_id: Option<&str>,
    ambient_identity: &AgentIdentity,
) -> Result<Option<ChatId>, AtmError> {
    if let Some(identity) = explicit_as {
        return Ok(identity.chat_id.clone());
    }
    if let Some(chat_id) = explicit_chat_id {
        return Ok(Some(chat_id.clone()));
    }
    if let Some(value) = ambient_chat_id {
        let trimmed = value.trim();
        return if trimmed.is_empty() {
            Ok(None)
        } else {
            trimmed.parse().map(Some)
        };
    }
    Ok(ambient_identity.chat_id.clone())
}

pub fn resolve_cli_mutation_caller_context(
    team_override: Option<CallerTeamOverride<'_>>,
) -> Result<CallerContext, AtmError> {
    resolve_cli_mutation_caller_context_with_overrides(CallerContextOverrides {
        identity_override: None,
        chat_id_override: None,
        team_override,
    })
}

/// Resolves a caller for a mutating command without allowing impersonation.
pub fn resolve_cli_mutation_caller_context_with_overrides(
    overrides: CallerContextOverrides<'_>,
) -> Result<CallerContext, AtmError> {
    let ambient = read_cli_identity_from_env()?.ok_or_else(AtmError::identity_unavailable)?;
    let caller = resolve_cli_inspection_caller_context(overrides)?;
    if overrides.identity_override.is_some() && caller.caller_identity != ambient {
        return Err(AtmError::validation(
            "--as must use the same base agent as ATM_IDENTITY for mutating commands",
        ));
    }
    Ok(caller)
}

pub fn resolve_cli_caller_context(
    overrides: CallerContextOverrides<'_>,
) -> Result<CallerContext, AtmError> {
    resolve_cli_inspection_caller_context(overrides)
}

pub fn read_cli_identity_from_env() -> Result<Option<AgentName>, AtmError> {
    read_env_raw("ATM_IDENTITY")?
        .map(parse_identity)
        .map(|result| result.map(|identity| identity.agent))
        .transpose()
}

pub fn read_cli_team_from_env() -> Result<Option<TeamName>, AtmError> {
    read_env_raw("ATM_TEAM")?.map(parse_team).transpose()
}

pub fn read_cli_identity_from_env_or_warn(warning_site: &'static str) -> Option<AgentName> {
    match read_cli_identity_from_env() {
        Ok(identity) => identity,
        Err(error) => {
            tracing::warn!(
                %error,
                warning_site,
                env_var = "ATM_IDENTITY",
                "ignoring malformed caller identity environment variable during doctor context capture"
            );
            None
        }
    }
}

pub fn read_cli_team_from_env_or_warn(warning_site: &'static str) -> Option<TeamName> {
    match read_cli_team_from_env() {
        Ok(team) => team,
        Err(error) => {
            tracing::warn!(
                %error,
                warning_site,
                env_var = "ATM_TEAM",
                "ignoring malformed caller team environment variable during doctor context capture"
            );
            None
        }
    }
}

fn resolve_identity_component(explicit: Option<&str>) -> Result<AgentIdentity, AtmError> {
    let raw = match explicit {
        Some(value) => value.to_string(),
        None => match read_cli_identity_from_env()? {
            Some(value) => return Ok(AgentIdentity::new(value, None)),
            None => return Err(AtmError::identity_unavailable()),
        },
    };
    parse_identity(raw)
}

fn resolve_team_component(explicit: Option<&str>) -> Result<TeamName, AtmError> {
    let raw = match explicit {
        Some(value) => value.to_string(),
        None => match read_cli_team_from_env()? {
            Some(value) => return Ok(value),
            None => return Err(AtmError::team_unavailable()),
        },
    };
    parse_team(raw)
}

fn read_env_raw(key: &str) -> Result<Option<String>, AtmError> {
    match env::var_os(key) {
        None => Ok(None),
        Some(value) => value.into_string().map(Some).map_err(|value| match key {
            "ATM_IDENTITY" => AtmError::identity_invalid(format!(
                "{key} must be valid UTF-8 text, got {:?}",
                value
            )),
            "ATM_TEAM" => {
                AtmError::team_invalid(format!("{key} must be valid UTF-8 text, got {:?}", value))
            }
            "ATM_CHAT_ID" => {
                AtmError::validation(format!("{key} must be valid UTF-8 text, got {:?}", value))
            }
            _ => unreachable!("caller context only reads ATM-owned keys"),
        }),
    }
}

fn parse_identity(raw: String) -> Result<AgentIdentity, AtmError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AtmError::identity_invalid(
            "caller identity must not be blank".to_string(),
        ));
    }

    trimmed
        .parse::<AgentIdentity>()
        .map_err(|error| AtmError::identity_invalid(error.detail()))
}

fn parse_team(raw: String) -> Result<TeamName, AtmError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AtmError::team_invalid(
            "caller team must not be blank".to_string(),
        ));
    }

    trimmed
        .parse::<TeamName>()
        .map_err(|error| AtmError::team_invalid(error.detail()))
}

#[cfg(test)]
mod tests {
    use crate::error_codes::AtmErrorCode;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};

    use super::{
        CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
        read_cli_identity_from_env, read_cli_identity_from_env_or_warn, read_cli_team_from_env,
        read_cli_team_from_env_or_warn, resolve_cli_inspection_caller_context,
        resolve_cli_mutation_caller_context,
    };

    #[test]
    #[serial_test::serial(env)]
    fn explicit_overrides_win_over_environment() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(ROLE_TEAM_LEAD)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let override_team = format!("{TEST_TEAM}-alt");

        let context = resolve_cli_inspection_caller_context(CallerContextOverrides {
            identity_override: Some(CallerIdentityOverride(TEST_SENDER)),
            chat_id_override: None,
            team_override: Some(CallerTeamOverride(override_team.as_str())),
        })
        .expect("caller context");

        assert_eq!(context.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(context.caller_team.as_str(), override_team);
    }

    #[test]
    #[serial_test::serial(env)]
    fn environment_supplies_context_when_overrides_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);

        let context = resolve_cli_inspection_caller_context(CallerContextOverrides::default())
            .expect("caller context");

        assert_eq!(context.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(context.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial_test::serial(env)]
    fn missing_identity_fails_before_dispatch() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", None), ("ATM_TEAM", Some(TEST_TEAM))]);

        let error = resolve_cli_mutation_caller_context(Some(CallerTeamOverride(TEST_TEAM)))
            .expect_err("missing identity");

        assert_eq!(error.code(), AtmErrorCode::IdentityUnavailable);
    }

    #[test]
    #[serial_test::serial(env)]
    fn invalid_explicit_team_uses_team_invalid_contract() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);

        let error = resolve_cli_inspection_caller_context(CallerContextOverrides {
            identity_override: None,
            chat_id_override: None,
            team_override: Some(CallerTeamOverride("../bad")),
        })
        .expect_err("invalid team");

        assert_eq!(error.code(), AtmErrorCode::TeamInvalid);
    }

    #[test]
    #[serial_test::serial(env)]
    fn optional_env_reads_return_none_when_missing() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", None), ("ATM_TEAM", None)]);

        assert_eq!(read_cli_identity_from_env().expect("identity"), None);
        assert_eq!(read_cli_team_from_env().expect("team"), None);
    }

    #[test]
    #[serial_test::serial(env)]
    fn mutating_context_ignores_identity_override_surface() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);

        let context = resolve_cli_mutation_caller_context(Some(CallerTeamOverride(TEST_TEAM)))
            .expect("caller context");

        assert_eq!(context.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(context.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial_test::serial(env)]
    fn lossy_identity_env_read_warns_and_falls_back_to_none() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some("   ")), ("ATM_TEAM", None)]);

        assert_eq!(
            read_cli_identity_from_env_or_warn("caller_context test"),
            None
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn lossy_team_env_read_warns_and_falls_back_to_none() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", None), ("ATM_TEAM", Some("   "))]);

        assert_eq!(read_cli_team_from_env_or_warn("caller_context test"), None);
    }
}
