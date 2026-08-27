use std::path::PathBuf;

use chrono::Utc;
use serde::Serialize;
use serde_json::json;

use crate::boundary::{RosterEntry, RosterHarness, RosterMemberKind, RosterStore};
use crate::delivery_channel::{HerdrSession, LocalMessageReceivedBackend};
use crate::error::AtmError;
use crate::home;
use crate::schema::{AgentType, HOME_DIR_METADATA_KEY, HomeDirPath, WORKSPACE_ROOT_METADATA_KEY};
use crate::types::{AgentName, ModelName, PaneId, TeamName};

use super::{filesystem, projection};

/// Semantic target member for roster repair operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberName(pub AgentName);

/// Explicit local backend flags supplied by the CLI.
#[derive(Debug, Clone, Copy)]
pub struct BackendOptions<'a> {
    pub backend: Option<&'a str>,
    pub target: Option<&'a str>,
    pub session: Option<&'a str>,
}

/// Parameters for adding one member to a team roster.
#[derive(Debug, Clone)]
pub struct AddMemberRequest {
    pub atm_home_dir: HomeDirPath,
    pub team: TeamName,
    pub member: AgentName,
    pub agent_type: AgentType,
    pub model: ModelName,
    pub member_home_dir: HomeDirPath,
    pub tmux_pane_id: Option<PaneId>,
    pub local_backend: Option<LocalMessageReceivedBackend>,
    pub backend_warning: Option<String>,
}

impl AddMemberRequest {
    pub fn new(
        atm_home_dir: PathBuf,
        team: &str,
        member: &str,
        agent_type: String,
        model: String,
        member_home_dir: PathBuf,
        tmux_pane_id: Option<String>,
    ) -> Result<Self, AtmError> {
        let tmux_pane_id = normalize_tmux_pane_id(tmux_pane_id.as_deref())?.map(|(pane, _)| pane);
        Ok(Self {
            atm_home_dir: atm_home_dir.into(),
            team: team.parse()?,
            member: member.parse()?,
            agent_type: parse_agent_type(agent_type)?,
            model: ModelName::new(model)?,
            member_home_dir: member_home_dir.into(),
            tmux_pane_id: tmux_pane_id.clone(),
            local_backend: tmux_pane_id
                .map(|pane_id| LocalMessageReceivedBackend::Tmux { pane_id }),
            backend_warning: None,
        })
    }

    /// Constructs a request from the explicit CLI backend selection.
    pub fn new_with_backend(
        atm_home_dir: PathBuf,
        team: &str,
        member: &str,
        agent_type: String,
        model: String,
        member_home_dir: PathBuf,
        options: BackendOptions<'_>,
    ) -> Result<Self, AtmError> {
        let member_name: AgentName = member.parse()?;
        let local_backend = parse_backend(
            &member_name,
            options.backend,
            options.target,
            options.session,
        )?;
        let backend_warning =
            nonstandard_tmux_warning(&member_name, options.backend, options.target)?;
        let tmux_pane_id = match &local_backend {
            Some(LocalMessageReceivedBackend::Tmux { pane_id }) => Some(pane_id.clone()),
            _ => None,
        };
        Ok(Self {
            atm_home_dir: atm_home_dir.into(),
            team: team.parse()?,
            member: member_name,
            agent_type: parse_agent_type(agent_type)?,
            model: ModelName::new(model)?,
            member_home_dir: member_home_dir.into(),
            tmux_pane_id,
            local_backend,
            backend_warning,
        })
    }
}

/// Result of adding one member and optional inbox to a team.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AddMemberOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub member: AgentName,
    pub created_inbox: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Parameters for updating one existing team member metadata row.
#[derive(Debug, Clone)]
pub struct UpdateMemberRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub team: TeamName,
    pub member: MemberName,
    pub home_dir: Option<HomeDirPath>,
    pub workspace_root: Option<HomeDirPath>,
    pub harness: Option<RosterHarness>,
    pub agent_type: Option<AgentType>,
    pub model: Option<ModelName>,
    pub tmux_pane_id: Option<PaneId>,
    pub local_backend: Option<LocalMessageReceivedBackend>,
    pub backend_warning: Option<String>,
}

impl UpdateMemberRequest {
    #[allow(
        clippy::too_many_arguments,
        reason = "CLI-owned repair path constructs one request from flat flags"
    )]
    pub fn new(
        caller_identity: AgentName,
        caller_team: TeamName,
        team: &str,
        member: &str,
        home_dir: Option<PathBuf>,
        workspace_root: Option<PathBuf>,
        harness: Option<String>,
        agent_type: Option<String>,
        model: Option<String>,
        tmux_pane_id: Option<String>,
    ) -> Result<Self, AtmError> {
        let tmux_pane_id = normalize_tmux_pane_id(tmux_pane_id.as_deref())?.map(|(pane, _)| pane);
        Ok(Self {
            caller_identity,
            caller_team,
            team: team.parse()?,
            member: MemberName(member.parse()?),
            home_dir: home_dir.map(Into::into),
            workspace_root: workspace_root.map(Into::into),
            harness: harness.map(parse_roster_harness).transpose()?,
            agent_type: agent_type.map(parse_agent_type).transpose()?,
            model: model.map(ModelName::new).transpose()?,
            local_backend: tmux_pane_id
                .clone()
                .map(|pane_id| LocalMessageReceivedBackend::Tmux { pane_id }),
            tmux_pane_id,
            backend_warning: None,
        })
    }

    /// Constructs an update request from explicit backend flags.
    #[allow(
        clippy::too_many_arguments,
        reason = "retained update fields are a compatibility surface; backend flags are grouped"
    )]
    pub fn new_with_backend(
        caller_identity: AgentName,
        caller_team: TeamName,
        team: &str,
        member: &str,
        home_dir: Option<PathBuf>,
        workspace_root: Option<PathBuf>,
        harness: Option<String>,
        agent_type: Option<String>,
        model: Option<String>,
        options: BackendOptions<'_>,
    ) -> Result<Self, AtmError> {
        let member_name: AgentName = member.parse()?;
        let local_backend = parse_backend(
            &member_name,
            options.backend,
            options.target,
            options.session,
        )?;
        let backend_warning =
            nonstandard_tmux_warning(&member_name, options.backend, options.target)?;
        let tmux_pane_id = match &local_backend {
            Some(LocalMessageReceivedBackend::Tmux { pane_id }) => Some(pane_id.clone()),
            _ => None,
        };
        Ok(Self {
            caller_identity,
            caller_team,
            team: team.parse()?,
            member: MemberName(member_name),
            home_dir: home_dir.map(Into::into),
            workspace_root: workspace_root.map(Into::into),
            harness: harness.map(parse_roster_harness).transpose()?,
            agent_type: agent_type.map(parse_agent_type).transpose()?,
            model: model.map(ModelName::new).transpose()?,
            tmux_pane_id,
            local_backend,
            backend_warning,
        })
    }
}

/// Result of updating one existing member metadata row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateMemberOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub member: AgentName,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Parameters for removing one member from a team roster.
///
/// Removal is authorized using the caller identity and team, matching the
/// authorization contract of `update-member`.
#[derive(Debug, Clone)]
pub struct RemoveMemberRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub team: TeamName,
    pub member: AgentName,
}

impl RemoveMemberRequest {
    pub fn new(
        caller_identity: AgentName,
        caller_team: TeamName,
        team: &str,
        member: &str,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            caller_identity,
            caller_team,
            team: team.parse()?,
            member: member.parse()?,
        })
    }
}

/// Result of removing one member from a team roster.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RemoveMemberOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub member: AgentName,
}

struct MemberAddContext {
    existing_roster: Vec<RosterEntry>,
}

/// Add one member record and inbox file to a team.
///
/// # Errors
///
/// Returns [`AtmError`] when the team is missing, the member already exists, or
/// inbox/config persistence fails.
pub fn add_member_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    request: AddMemberRequest,
) -> Result<AddMemberOutcome, AtmError> {
    let MemberAddContext {
        mut existing_roster,
    } = load_member_add_context(roster_store, &request)?;

    let inbox_path = home::inbox_path_from_home(
        request.atm_home_dir.as_ref(),
        &request.team,
        &request.member,
    )?;
    let created_inbox = filesystem::ensure_inbox_exists(&inbox_path)?;
    existing_roster.push(build_member_add_roster_record(&request));
    replace_roster_for_member_add(roster_store, &request.team, &existing_roster)?;

    Ok(AddMemberOutcome {
        action: "add-member",
        team: request.team,
        member: request.member,
        created_inbox,
        warnings: request.backend_warning.into_iter().collect(),
    })
}

/// Update one existing member record through the retained local repair path.
///
/// # Errors
///
/// Returns [`AtmError`] when the team is missing, the member does not exist,
/// or roster/config persistence fails.
pub fn update_member_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    request: UpdateMemberRequest,
) -> Result<UpdateMemberOutcome, AtmError> {
    validate_update_member_caller(roster_store, &request)?;
    let mut existing_roster = projection::load_team_roster(roster_store, &request.team)?;
    let member_name = request.member.0.clone();
    let member = existing_roster
        .iter_mut()
        .find(|existing_member| existing_member.agent_name == member_name)
        .ok_or_else(|| AtmError::member_not_found(member_name.as_str(), request.team.as_str()))?;

    apply_member_metadata_update(member, &request);
    roster_store.replace_roster(&request.team, &existing_roster)?;

    Ok(UpdateMemberOutcome {
        action: "update-member",
        team: request.team,
        member: member_name,
        warnings: request.backend_warning.into_iter().collect(),
    })
}

/// Remove one member record from a team roster.
///
/// # Errors
///
/// Returns [`AtmError`] when the caller does not belong to the target team,
/// the target team or member is missing, or loading/persisting the roster
/// fails. It never removes inbox data.
pub fn remove_member_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    request: RemoveMemberRequest,
) -> Result<RemoveMemberOutcome, AtmError> {
    validate_remove_member_caller(roster_store, &request)?;
    let mut existing_roster = projection::load_team_roster(roster_store, &request.team)?;
    ensure_member_present(&existing_roster, &request.team, &request.member)?;
    existing_roster.retain(|entry| entry.agent_name != request.member);
    roster_store.replace_roster(&request.team, &existing_roster)?;

    Ok(RemoveMemberOutcome {
        action: "remove-member",
        team: request.team,
        member: request.member,
    })
}

pub(crate) const MAX_MEMBER_METADATA_FIELD_LEN: usize = 256;

fn load_member_add_context(
    roster_store: &dyn RosterStore,
    request: &AddMemberRequest,
) -> Result<MemberAddContext, AtmError> {
    let existing_roster = projection::load_team_roster(roster_store, &request.team)?;
    ensure_member_absent(&existing_roster, &request.team, &request.member)?;
    Ok(MemberAddContext { existing_roster })
}

fn ensure_member_absent(
    existing_roster: &[RosterEntry],
    team: &TeamName,
    member: &AgentName,
) -> Result<(), AtmError> {
    if existing_roster
        .iter()
        .any(|existing_member| existing_member.agent_name == *member)
    {
        return Err(AtmError::member_already_exists(
            member.as_str(),
            team.as_str(),
        ));
    }
    Ok(())
}

fn validate_update_member_caller(
    roster_store: &dyn RosterStore,
    request: &UpdateMemberRequest,
) -> Result<(), AtmError> {
    validate_member_mutation_caller(
        roster_store,
        &request.caller_identity,
        &request.caller_team,
        &request.team,
        "update-member",
    )
}

fn ensure_member_present(
    existing_roster: &[RosterEntry],
    team: &TeamName,
    member: &AgentName,
) -> Result<(), AtmError> {
    if !existing_roster
        .iter()
        .any(|existing_member| existing_member.agent_name == *member)
    {
        return Err(AtmError::member_not_found(member.as_str(), team.as_str()));
    }
    Ok(())
}

fn validate_remove_member_caller(
    roster_store: &dyn RosterStore,
    request: &RemoveMemberRequest,
) -> Result<(), AtmError> {
    validate_member_mutation_caller(
        roster_store,
        &request.caller_identity,
        &request.caller_team,
        &request.team,
        "remove-member",
    )
}

/// Require a caller from the team that the mutation targets.
///
/// Keeping this in one helper makes `update-member` and `remove-member`
/// enforce the same membership gate while retaining action-specific errors.
fn validate_member_mutation_caller(
    roster_store: &dyn RosterStore,
    caller_identity: &AgentName,
    caller_team: &TeamName,
    target_team: &TeamName,
    action: &str,
) -> Result<(), AtmError> {
    if caller_team != target_team {
        return Err(AtmError::validation(format!(
            "caller team '{}' does not match {action} target team '{target_team}'",
            caller_team,
        )));
    }

    let caller_entry = roster_store.query_membership(target_team, caller_identity)?;
    if caller_entry.is_none() {
        return Err(AtmError::member_not_found(
            caller_identity.as_str(),
            target_team.as_str(),
        ));
    }

    Ok(())
}

fn build_member_add_roster_record(request: &AddMemberRequest) -> RosterEntry {
    let normalized_tmux_pane_id = request.tmux_pane_id.clone();
    let mut extra = serde_json::Map::new();
    match request.local_backend.as_ref() {
        Some(LocalMessageReceivedBackend::Tmux { .. }) => {
            extra.insert("backendType".to_string(), json!("tmux"));
            extra.insert("isActive".to_string(), json!(true));
        }
        Some(LocalMessageReceivedBackend::Herdr { session }) => {
            extra.insert("backendType".to_string(), json!("herdr"));
            if let Some(session) = session {
                extra.insert("herdrSession".to_string(), json!(session.as_str()));
            }
        }
        None if normalized_tmux_pane_id.is_some() => {
            extra.insert("backendType".to_string(), json!("tmux"));
            extra.insert("isActive".to_string(), json!(true));
        }
        None => {}
    }
    if matches!(
        request.local_backend,
        Some(LocalMessageReceivedBackend::Tmux { .. })
    ) || (request.local_backend.is_none() && normalized_tmux_pane_id.is_some())
    {
        extra.insert("isActive".to_string(), json!(true));
    }
    extra.insert(
        "agentId".to_string(),
        json!(format!("{}@{}", request.member, request.team)),
    );
    extra.insert(
        "joinedAt".to_string(),
        json!(Utc::now().timestamp_millis() as u64),
    );
    extra.insert(
        HOME_DIR_METADATA_KEY.to_string(),
        json!(request.member_home_dir.as_ref().display().to_string()),
    );

    RosterEntry {
        team_name: request.team.clone(),
        agent_name: request.member.clone(),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::ClaudeCode,
        agent_type: request.agent_type.clone(),
        model: request.model.clone(),
        recipient_pane_id: normalized_tmux_pane_id,
        metadata_json: extra,
    }
}

fn replace_roster_for_member_add(
    roster_store: &dyn RosterStore,
    team: &TeamName,
    existing_roster: &[RosterEntry],
) -> Result<(), AtmError> {
    roster_store.replace_roster(team, existing_roster)
}

fn apply_member_metadata_update(member: &mut RosterEntry, request: &UpdateMemberRequest) {
    if let Some(home_dir) = &request.home_dir {
        member.metadata_json.insert(
            HOME_DIR_METADATA_KEY.to_string(),
            json!(home_dir.as_ref().display().to_string()),
        );
    }
    if let Some(workspace_root) = &request.workspace_root {
        member.metadata_json.insert(
            WORKSPACE_ROOT_METADATA_KEY.to_string(),
            json!(workspace_root.as_ref().display().to_string()),
        );
    }
    if let Some(harness) = request.harness {
        member.harness = harness;
    }
    if let Some(agent_type) = &request.agent_type {
        member.agent_type = agent_type.clone();
    }
    if let Some(model) = &request.model {
        member.model = model.clone();
    }
    match request.local_backend.as_ref() {
        Some(LocalMessageReceivedBackend::Tmux { pane_id }) => {
            member.recipient_pane_id = Some(pane_id.clone());
            member
                .metadata_json
                .insert("backendType".to_string(), json!("tmux"));
            member
                .metadata_json
                .insert("isActive".to_string(), json!(true));
            member.metadata_json.remove("herdrSession");
        }
        Some(LocalMessageReceivedBackend::Herdr { session }) => {
            member.recipient_pane_id = None;
            member
                .metadata_json
                .insert("backendType".to_string(), json!("herdr"));
            member.metadata_json.remove("isActive");
            match session {
                Some(session) => {
                    member
                        .metadata_json
                        .insert("herdrSession".to_string(), json!(session.as_str()));
                }
                None => {
                    member.metadata_json.remove("herdrSession");
                }
            }
        }
        None if request.tmux_pane_id.is_some() => {
            let pane_id = request.tmux_pane_id.as_ref().expect("checked above");
            member.recipient_pane_id = Some(pane_id.clone());
            member
                .metadata_json
                .insert("backendType".to_string(), json!("tmux"));
            member
                .metadata_json
                .insert("isActive".to_string(), json!(true));
            member.metadata_json.remove("herdrSession");
        }
        None => {}
    }
}

fn parse_agent_type(value: String) -> Result<AgentType, AtmError> {
    if value.len() > MAX_MEMBER_METADATA_FIELD_LEN {
        return Err(AtmError::validation(format!(
            "agent_type must be at most {MAX_MEMBER_METADATA_FIELD_LEN} bytes"
        )));
    }
    Ok(AgentType::from(value))
}

fn parse_roster_harness(value: String) -> Result<RosterHarness, AtmError> {
    if value.len() > MAX_MEMBER_METADATA_FIELD_LEN {
        return Err(AtmError::validation(format!(
            "harness must be at most {MAX_MEMBER_METADATA_FIELD_LEN} bytes"
        )));
    }
    match value.as_str() {
        "claude-code" => Ok(RosterHarness::ClaudeCode),
        "codex-cli" => Ok(RosterHarness::CodexCli),
        "gemini-cli" => Ok(RosterHarness::GeminiCli),
        "opencode" => Ok(RosterHarness::Opencode),
        "hermes" => Ok(RosterHarness::Hermes),
        "python-graft" => Ok(RosterHarness::PythonGraft),
        _ => Err(AtmError::validation(
            "harness must be one of: claude-code, codex-cli, gemini-cli, opencode, hermes, python-graft"
                .to_string(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TmuxTargetShape {
    Strict,
    NonStandard,
}

fn normalize_tmux_pane_id(
    pane_id: Option<&str>,
) -> Result<Option<(PaneId, TmuxTargetShape)>, AtmError> {
    let Some(raw) = pane_id.map(str::trim) else {
        return Ok(None);
    };
    if raw.is_empty() {
        return PaneId::from_cli(raw).map(|pane| Some((pane, TmuxTargetShape::Strict)));
    }

    if (raw
        .strip_prefix('%')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())))
        || raw.chars().all(|ch| ch.is_ascii_digit())
    {
        return PaneId::from_cli(raw).map(|pane| Some((pane, TmuxTargetShape::Strict)));
    }

    PaneId::from_cli(raw).map(|pane| Some((pane, TmuxTargetShape::NonStandard)))
}

fn parse_backend(
    member: &AgentName,
    backend: Option<&str>,
    target: Option<&str>,
    session: Option<&str>,
) -> Result<Option<LocalMessageReceivedBackend>, AtmError> {
    match backend {
        None => {
            if session.is_some() || target.is_some() {
                return Err(AtmError::validation(
                    "--target and --session require an explicit --backend",
                ));
            }
            Ok(None)
        }
        Some("tmux") => {
            if session.is_some() {
                return Err(AtmError::validation("--session requires --backend herdr"));
            }
            let normalized = normalize_tmux_pane_id(target)?
                .ok_or_else(|| AtmError::validation("--backend tmux requires --target"))?;
            if normalized.1 == TmuxTargetShape::NonStandard {
                tracing::warn!(
                    member = %member,
                    backend = "tmux",
                    target = normalized.0.as_str(),
                    "non-standard tmux target accepted; verify backend ownership for every member"
                );
            }
            let pane = normalized.0;
            Ok(Some(LocalMessageReceivedBackend::Tmux { pane_id: pane }))
        }
        Some("herdr") => {
            if target.is_some() {
                return Err(AtmError::validation(
                    "--backend herdr does not accept --target",
                ));
            }
            if !is_herdr_agent_name(member.as_str()) {
                return Err(AtmError::validation(format!(
                    "Herdr agent name must match ^[a-z][a-z0-9_-]{{0,31}}$: {member}"
                )));
            }
            let session = session
                .map(|value| {
                    crate::address::validate_path_segment(value, "herdr session")?;
                    HerdrSession::new(value)
                })
                .transpose()?;
            Ok(Some(LocalMessageReceivedBackend::Herdr { session }))
        }
        Some(other) => Err(AtmError::validation(format!(
            "unsupported local backend '{other}'; expected tmux or herdr"
        ))),
    }
}

fn is_herdr_agent_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 32
        && first.is_ascii_lowercase()
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

fn nonstandard_tmux_warning(
    member: &AgentName,
    backend: Option<&str>,
    target: Option<&str>,
) -> Result<Option<String>, AtmError> {
    if backend != Some("tmux") {
        return Ok(None);
    }
    let Some((pane, shape)) = normalize_tmux_pane_id(target)? else {
        return Ok(None);
    };
    if shape == TmuxTargetShape::NonStandard {
        Ok(Some(format!(
            "member {member} uses backend tmux with non-standard target '{}'; verify --backend (herdr|tmux) for every member in the team; mixed-backend rosters require an explicit correct backend",
            pane.as_str()
        )))
    } else {
        Ok(None)
    }
}
