use std::env;

use atm_core::error::AtmError;
use atm_core::types::{AgentName, TeamName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CallerContext {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CallerIdentityOverride<'a>(pub &'a str);

#[derive(Debug, Clone, Copy)]
pub(crate) struct CallerTeamOverride<'a>(pub &'a str);

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CallerContextOverrides<'a> {
    pub identity_override: Option<CallerIdentityOverride<'a>>,
    pub team_override: Option<CallerTeamOverride<'a>>,
}

pub(crate) fn resolve_cli_caller_context(
    overrides: CallerContextOverrides<'_>,
) -> Result<CallerContext, AtmError> {
    let caller_identity =
        resolve_identity_component(overrides.identity_override.map(|value| value.0))?;
    let caller_team = resolve_team_component(overrides.team_override.map(|value| value.0))?;
    Ok(CallerContext {
        caller_identity,
        caller_team,
    })
}

fn resolve_identity_component(explicit: Option<&str>) -> Result<AgentName, AtmError> {
    let raw = match explicit {
        Some(value) => value.to_string(),
        None => match read_env_raw("ATM_IDENTITY")? {
            Some(value) => value,
            None => return Err(AtmError::identity_unavailable()),
        },
    };
    parse_identity(raw)
}

fn resolve_team_component(explicit: Option<&str>) -> Result<TeamName, AtmError> {
    let raw = match explicit {
        Some(value) => value.to_string(),
        None => match read_env_raw("ATM_TEAM")? {
            Some(value) => value,
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
            _ => unreachable!("caller context only reads ATM-owned keys"),
        }),
    }
}

fn parse_identity(raw: String) -> Result<AgentName, AtmError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AtmError::identity_invalid(
            "caller identity must not be blank".to_string(),
        ));
    }

    trimmed
        .parse::<AgentName>()
        .map_err(|error| AtmError::identity_invalid(error.message))
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
        .map_err(|error| AtmError::team_invalid(error.message))
}

#[cfg(test)]
mod tests {
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::roles::ROLE_TEAM_LEAD;
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
    use serial_test::serial;

    use super::{
        CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
        resolve_cli_caller_context,
    };

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var_os(key);
            match value {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    #[test]
    #[serial(env)]
    fn explicit_overrides_win_over_environment() {
        let _identity = EnvGuard::set("ATM_IDENTITY", Some(ROLE_TEAM_LEAD));
        let _team = EnvGuard::set("ATM_TEAM", Some(TEST_TEAM));
        let override_team = format!("{TEST_TEAM}-alt");

        let context = resolve_cli_caller_context(CallerContextOverrides {
            identity_override: Some(CallerIdentityOverride(TEST_SENDER)),
            team_override: Some(CallerTeamOverride(override_team.as_str())),
        })
        .expect("caller context");

        assert_eq!(context.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(context.caller_team.as_str(), override_team);
    }

    #[test]
    #[serial(env)]
    fn environment_supplies_context_when_overrides_absent() {
        let _identity = EnvGuard::set("ATM_IDENTITY", Some(TEST_SENDER));
        let _team = EnvGuard::set("ATM_TEAM", Some(TEST_TEAM));

        let context =
            resolve_cli_caller_context(CallerContextOverrides::default()).expect("caller context");

        assert_eq!(context.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(context.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial(env)]
    fn missing_identity_fails_before_dispatch() {
        let _identity = EnvGuard::set("ATM_IDENTITY", None);
        let _team = EnvGuard::set("ATM_TEAM", Some(TEST_TEAM));

        let error = resolve_cli_caller_context(CallerContextOverrides::default())
            .expect_err("missing identity");

        assert_eq!(error.code, AtmErrorCode::IdentityUnavailable);
    }

    #[test]
    #[serial(env)]
    fn invalid_explicit_team_uses_team_invalid_contract() {
        let _identity = EnvGuard::set("ATM_IDENTITY", Some(TEST_SENDER));
        let _team = EnvGuard::set("ATM_TEAM", Some(TEST_TEAM));

        let error = resolve_cli_caller_context(CallerContextOverrides {
            identity_override: None,
            team_override: Some(CallerTeamOverride("../bad")),
        })
        .expect_err("invalid team");

        assert_eq!(error.code, AtmErrorCode::TeamInvalid);
    }
}
