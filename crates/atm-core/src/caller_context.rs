use std::env;

use serde::{Deserialize, Serialize};

use crate::error::AtmError;
use crate::types::{AgentIdentity, AgentName, ChatId, SessionId, TeamName};

/// Environment-attested, transient metadata about the caller's local activity.
///
/// This is deliberately distinct from command identity: it is present only
/// when the ambient identity and team attest to the resolved caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivityObservation {
    pub team: TeamName,
    pub member: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerContext {
    pub caller_identity: AgentName,
    pub caller_chat_id: Option<ChatId>,
    pub caller_team: TeamName,
    pub activity_observation: Option<ActivityObservation>,
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
        return Err(AtmError::caller_context_request_invalid(
            "--as and --chat-id are mutually exclusive",
        ));
    }
    let caller_identity =
        resolve_identity_component(overrides.identity_override.map(|value| value.0))?;
    let caller_chat_id = resolve_caller_chat_id(
        overrides.identity_override.map(|_| &caller_identity),
        overrides
            .chat_id_override
            .map(|value| value.0.parse())
            .transpose()?
            .as_ref(),
        read_cli_chat_id_from_env()?.as_deref(),
        &caller_identity,
    )?;
    let caller_team = resolve_team_component(overrides.team_override.map(|value| value.0))?;
    let activity_observation =
        activity_observation_for_resolved_caller(&caller_identity.agent, &caller_team);
    Ok(CallerContext {
        caller_identity: caller_identity.agent,
        caller_chat_id,
        caller_team,
        activity_observation,
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
    if overrides.identity_override.is_some() && caller.caller_identity != ambient.agent {
        return Err(AtmError::caller_context_request_invalid(
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

/// Reads the complete ambient identity so callers retain an optional chat id.
pub fn read_cli_identity_from_env() -> Result<Option<AgentIdentity>, AtmError> {
    read_env_raw("ATM_IDENTITY")?
        .map(parse_identity)
        .transpose()
}

/// Reads only the ambient base agent for legacy callers that cannot carry chat identity.
pub fn read_cli_agent_name_from_env() -> Result<Option<AgentName>, AtmError> {
    Ok(read_cli_identity_from_env()?.map(|identity| identity.agent))
}

pub fn read_cli_team_from_env() -> Result<Option<TeamName>, AtmError> {
    read_env_raw("ATM_TEAM")?.map(parse_team).transpose()
}

/// Reads optional, environment-attested session telemetry for the CLI caller.
pub fn read_cli_session_id_from_env() -> Option<SessionId> {
    let value = env::var_os("ATM_SESSION_ID")?.into_string().ok()?;
    match SessionId::new(value) {
        Ok(session_id) => session_id,
        Err(error) => {
            tracing::info!(
                %error,
                env_var = "ATM_SESSION_ID",
                "suppressing invalid optional caller session telemetry"
            );
            None
        }
    }
}

/// Reads optional, environment-attested process telemetry for the CLI caller.
pub fn read_cli_pid_from_env() -> Option<u32> {
    env::var_os("ATM_PID")?
        .into_string()
        .ok()?
        .trim()
        .parse()
        .ok()
        .filter(|pid| *pid != 0)
}

/// Build transient telemetry only when the environment attests to this caller.
pub fn activity_observation_for_resolved_caller(
    member: &AgentName,
    team: &TeamName,
) -> Option<ActivityObservation> {
    let env_identity = read_cli_identity_from_env().ok().flatten()?;
    let env_team = read_cli_team_from_env().ok().flatten()?;
    if env_identity.agent != *member || env_team != *team {
        return None;
    }

    Some(ActivityObservation {
        team: team.clone(),
        member: member.clone(),
        session_id: read_cli_session_id_from_env(),
        pid: read_cli_pid_from_env(),
    })
}

fn read_cli_chat_id_from_env() -> Result<Option<String>, AtmError> {
    read_env_raw("ATM_CHAT_ID")
}

pub fn read_cli_identity_from_env_or_warn(warning_site: &'static str) -> Option<AgentName> {
    match read_cli_agent_name_from_env() {
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
            Some(value) => return Ok(value),
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
            "ATM_CHAT_ID" => AtmError::caller_context_request_invalid(format!(
                "{key} must be valid UTF-8 text, got {:?}",
                value
            )),
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
    use crate::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM, set_env_var};
    use crate::types::{AgentIdentity, ChatId, SESSION_ID_MAX_BYTES};

    use super::{
        CallerChatIdOverride, CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
        activity_observation_for_resolved_caller, read_cli_agent_name_from_env,
        read_cli_identity_from_env_or_warn, read_cli_pid_from_env, read_cli_session_id_from_env,
        read_cli_team_from_env, read_cli_team_from_env_or_warn, resolve_caller_chat_id,
        resolve_cli_inspection_caller_context, resolve_cli_mutation_caller_context,
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
        assert_eq!(context.activity_observation, None);
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
        assert_eq!(
            context.activity_observation,
            Some(super::ActivityObservation {
                team: TEST_TEAM.parse().expect("team"),
                member: TEST_SENDER.parse().expect("member"),
                session_id: None,
                pid: None,
            })
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn inspection_as_override_does_not_require_ambient_identity() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", None),
            ("ATM_CHAT_ID", None),
            ("ATM_TEAM", None),
        ]);

        let context = resolve_cli_inspection_caller_context(CallerContextOverrides {
            identity_override: Some(CallerIdentityOverride(TEST_SENDER)),
            chat_id_override: None,
            team_override: Some(CallerTeamOverride(TEST_TEAM)),
        })
        .expect("--as inspection context without ATM_IDENTITY");

        assert_eq!(context.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(context.caller_team.as_str(), TEST_TEAM);
        assert_eq!(context.caller_chat_id, None);
        assert_eq!(context.activity_observation, None);
    }

    #[test]
    #[serial_test::serial(env)]
    fn inspection_context_with_all_identity_inputs_absent_is_unavailable() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", None),
            ("ATM_CHAT_ID", None),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);

        let error = resolve_cli_inspection_caller_context(CallerContextOverrides::default())
            .expect_err("missing inspection identity");

        assert_eq!(error.code(), AtmErrorCode::IdentityUnavailable);
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
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", None),
            ("ATM_TEAM", None),
            ("ATM_SESSION_ID", None),
            ("ATM_PID", None),
        ]);

        assert_eq!(read_cli_agent_name_from_env().expect("identity"), None);
        assert_eq!(read_cli_team_from_env().expect("team"), None);
        assert_eq!(read_cli_session_id_from_env(), None);
        assert_eq!(read_cli_pid_from_env(), None);
    }

    #[test]
    #[serial_test::serial(env)]
    fn optional_activity_metadata_readers_normalize_invalid_values() {
        let oversized = "s".repeat(SESSION_ID_MAX_BYTES + 1);
        let _env = EnvGuard::set_many([("ATM_SESSION_ID", Some("  ")), ("ATM_PID", Some("0"))]);

        assert_eq!(read_cli_session_id_from_env(), None);
        assert_eq!(read_cli_pid_from_env(), None);

        drop(_env);
        let _env = EnvGuard::set_many([
            ("ATM_SESSION_ID", Some(oversized.as_str())),
            ("ATM_PID", Some("not-a-pid")),
        ]);

        assert_eq!(read_cli_session_id_from_env(), None);
        assert_eq!(read_cli_pid_from_env(), None);
    }

    #[test]
    #[serial_test::serial(env)]
    fn matching_environment_attests_activity_observation() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
            ("ATM_SESSION_ID", Some("session-17")),
            ("ATM_PID", Some("17")),
        ]);

        let context = resolve_cli_inspection_caller_context(CallerContextOverrides::default())
            .expect("caller context");
        let observation = context.activity_observation.expect("attested observation");

        assert_eq!(observation.member.as_str(), TEST_SENDER);
        assert_eq!(observation.team.as_str(), TEST_TEAM);
        assert_eq!(
            observation.session_id.as_ref().map(AsRef::as_ref),
            Some("session-17")
        );
        assert_eq!(observation.pid, Some(17));
    }

    #[test]
    #[serial_test::serial(env)]
    fn malformed_ambient_identity_suppresses_telemetry_without_breaking_overrides() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("   ")),
            ("ATM_TEAM", Some(TEST_TEAM)),
            ("ATM_SESSION_ID", Some("session-17")),
            ("ATM_PID", Some("17")),
        ]);

        let context = resolve_cli_inspection_caller_context(CallerContextOverrides {
            identity_override: Some(CallerIdentityOverride(TEST_SENDER)),
            chat_id_override: None,
            team_override: Some(CallerTeamOverride(TEST_TEAM)),
        })
        .expect("overrides remain valid");

        assert_eq!(context.activity_observation, None);
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn non_unicode_activity_metadata_is_suppressed() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
            ("ATM_SESSION_ID", None),
            ("ATM_PID", None),
        ]);
        set_env_var("ATM_SESSION_ID", OsString::from_vec(vec![0xff]));
        set_env_var("ATM_PID", OsString::from_vec(vec![0xff]));

        let member = TEST_SENDER.parse().expect("member");
        let team = TEST_TEAM.parse().expect("team");
        let observation = activity_observation_for_resolved_caller(&member, &team)
            .expect("identity and team attest");

        assert_eq!(observation.session_id, None);
        assert_eq!(observation.pid, None);
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

    #[test]
    fn chat_id_precedence_is_explicit_as_then_flag_then_environment_then_identity() {
        let explicit_as: AgentIdentity = "agent:as-chat".parse().expect("qualified --as");
        let explicit_chat_id: ChatId = "flag-chat".parse().expect("--chat-id");
        let ambient_identity: AgentIdentity = "agent:identity-chat".parse().expect("identity");

        assert_eq!(
            resolve_caller_chat_id(
                Some(&explicit_as),
                Some(&explicit_chat_id),
                Some("environment-chat"),
                &ambient_identity,
            )
            .expect("explicit --as"),
            Some("as-chat".parse().expect("chat id"))
        );
        assert_eq!(
            resolve_caller_chat_id(
                None,
                Some(&explicit_chat_id),
                Some("environment-chat"),
                &ambient_identity,
            )
            .expect("--chat-id"),
            Some("flag-chat".parse().expect("chat id"))
        );
        assert_eq!(
            resolve_caller_chat_id(None, None, Some("environment-chat"), &ambient_identity)
                .expect("ATM_CHAT_ID"),
            Some("environment-chat".parse().expect("chat id"))
        );
        assert_eq!(
            resolve_caller_chat_id(None, None, None, &ambient_identity).expect("identity chat"),
            Some("identity-chat".parse().expect("chat id"))
        );
    }

    #[test]
    fn unqualified_as_and_empty_ambient_chat_explicitly_select_no_chat() {
        let unqualified_as: AgentIdentity = "agent".parse().expect("unqualified --as");
        let ambient_identity: AgentIdentity = "agent:identity-chat".parse().expect("identity");

        assert_eq!(
            resolve_caller_chat_id(
                Some(&unqualified_as),
                None,
                Some("environment-chat"),
                &ambient_identity
            )
            .expect("unqualified --as"),
            None
        );
        assert_eq!(
            resolve_caller_chat_id(None, None, Some("  "), &ambient_identity)
                .expect("empty ATM_CHAT_ID"),
            None
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn qualified_identity_and_atm_chat_id_preserve_distinct_precedence_values() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a:identity-chat")),
            ("ATM_CHAT_ID", Some("environment-chat")),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);

        let context = resolve_cli_inspection_caller_context(CallerContextOverrides::default())
            .expect("caller context");

        assert_eq!(context.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(
            context.caller_chat_id,
            Some("environment-chat".parse().expect("chat id"))
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn qualified_ambient_identity_supplies_chat_id_when_environment_override_is_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a:identity-chat")),
            ("ATM_CHAT_ID", None),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);

        let context = resolve_cli_inspection_caller_context(CallerContextOverrides::default())
            .expect("caller context");

        assert_eq!(context.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(
            context.caller_chat_id,
            Some("identity-chat".parse().expect("chat id"))
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn chat_id_flag_with_ambient_identity_matches_qualified_as() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_CHAT_ID", None),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);

        let from_flag = resolve_cli_inspection_caller_context(CallerContextOverrides {
            identity_override: None,
            chat_id_override: Some(CallerChatIdOverride("chat-17")),
            team_override: None,
        })
        .expect("--chat-id context");
        let qualified_as = format!("{TEST_SENDER}:chat-17");
        let from_qualified_as = resolve_cli_inspection_caller_context(CallerContextOverrides {
            identity_override: Some(CallerIdentityOverride(qualified_as.as_str())),
            chat_id_override: None,
            team_override: None,
        })
        .expect("qualified --as context");

        assert_eq!(from_flag.caller_identity, from_qualified_as.caller_identity);
        assert_eq!(from_flag.caller_chat_id, from_qualified_as.caller_chat_id);
    }

    #[test]
    #[serial_test::serial(env)]
    fn mutually_exclusive_chat_overrides_use_caller_context_error() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);

        let error = resolve_cli_inspection_caller_context(CallerContextOverrides {
            identity_override: Some(CallerIdentityOverride(TEST_SENDER)),
            chat_id_override: Some(CallerChatIdOverride("chat-17")),
            team_override: None,
        })
        .expect_err("mutually exclusive overrides");

        assert_eq!(error.code(), AtmErrorCode::CallerContextRequestInvalid);
    }

    #[test]
    #[serial_test::serial(env)]
    fn invalid_atm_chat_id_fails_before_dispatch() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_CHAT_ID", Some("invalid:delimiter")),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);

        let error = resolve_cli_inspection_caller_context(CallerContextOverrides::default())
            .expect_err("invalid ATM_CHAT_ID");

        assert_eq!(error.code(), AtmErrorCode::AddressParseFailed);
    }
}
