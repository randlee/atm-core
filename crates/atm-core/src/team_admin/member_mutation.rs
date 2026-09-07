use std::path::PathBuf;

use chrono::Utc;
use serde::Serialize;
use serde_json::json;

use crate::boundary::{RosterEntry, RosterHarness, RosterMemberKind, RosterStore};
use crate::caller_context::resolve_roster_alias;
use crate::delivery_channel::{HerdrAgentName, HerdrSession, LocalMessageReceivedBackend};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::home;
use crate::schema::{AgentType, HOME_DIR_METADATA_KEY, HomeDirPath, WORKSPACE_ROOT_METADATA_KEY};
use crate::types::{AgentName, HostName, ModelName, PaneId, TeamName};

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
    pub alias: Option<&'a str>,
    pub clear_alias: bool,
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
    pub alias: Option<String>,
    pub backend_warning: Option<String>,
    /// This member's registered host (ADR-055 decision (e)), or `None` when
    /// unset. Set via [`AddMemberRequest::with_host`].
    pub host: Option<HostName>,
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
            alias: None,
            backend_warning: None,
            host: None,
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
        let alias = parse_alias(options.alias, options.clear_alias, false)?.flatten();
        let local_backend = parse_backend(
            &member_name,
            options.backend,
            options.target,
            options.session,
            alias.as_deref(),
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
            alias,
            backend_warning,
            host: None,
        })
    }

    /// Sets this member's registered host (ADR-055 decision (e)).
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when `host` is not a valid [`HostName`].
    pub fn with_host(mut self, host: Option<&str>) -> Result<Self, AtmError> {
        self.host = host.map(str::parse).transpose()?;
        Ok(self)
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
    /// `None` leaves the roster alias unchanged; `Some(None)` clears it.
    pub alias: Option<Option<String>>,
    pub backend_warning: Option<String>,
    /// This member's registered host (ADR-055 decision (e)); `None` means
    /// "leave unchanged" (the same convention as `harness`/`agent_type`/
    /// `model`), never "clear". Set via [`UpdateMemberRequest::with_host`].
    pub host: Option<HostName>,
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
            alias: None,
            backend_warning: None,
            host: None,
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
        let alias = parse_alias(options.alias, options.clear_alias, true)?;
        let local_backend = parse_backend(
            &member_name,
            options.backend,
            options.target,
            options.session,
            alias.as_ref().and_then(|value| value.as_deref()),
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
            alias,
            backend_warning,
            host: None,
        })
    }

    /// Sets this member's registered host (ADR-055 decision (e)); `None`
    /// leaves it unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`AtmError`] when `host` is not a valid [`HostName`].
    pub fn with_host(mut self, host: Option<&str>) -> Result<Self, AtmError> {
        self.host = host.map(str::parse).transpose()?;
        Ok(self)
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
    preflight_roster_unique_names(roster_store, &request.team, &existing_roster)?;
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
    let mut request = request;
    let mut existing_roster = projection::load_team_roster(roster_store, &request.team)?;
    canonicalize_member_mutation_aliases(&mut request, &existing_roster);
    validate_update_member_caller(roster_store, &request)?;
    let member_name = request.member.0.clone();
    ensure_member_name_available(&member_name)?;
    let member = existing_roster
        .iter_mut()
        .find(|existing_member| existing_member.agent_name == member_name)
        .ok_or_else(|| AtmError::member_not_found(member_name.as_str(), request.team.as_str()))?;

    validate_effective_herdr_agent_name(member, &request)?;
    apply_member_metadata_update(member, &request);
    preflight_roster_unique_names(roster_store, &request.team, &existing_roster)?;
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
    let mut request = request;
    let mut existing_roster = projection::load_team_roster(roster_store, &request.team)?;
    canonicalize_remove_member_aliases(&mut request, &existing_roster);
    validate_remove_member_caller(roster_store, &request)?;
    ensure_member_present(&existing_roster, &request.team, &request.member)?;
    existing_roster.retain(|entry| entry.agent_name != request.member);
    roster_store.replace_roster(&request.team, &existing_roster)?;

    Ok(RemoveMemberOutcome {
        action: "remove-member",
        team: request.team,
        member: request.member,
    })
}

/// Canonicalize every agent-name input before membership validation or roster
/// persistence so an alias cannot become a durable target or audit identity.
fn canonicalize_member_mutation_aliases(request: &mut UpdateMemberRequest, roster: &[RosterEntry]) {
    request.caller_identity =
        resolve_roster_alias(&request.caller_identity, &request.caller_team, roster);
    request.member.0 = resolve_roster_alias(&request.member.0, &request.team, roster);
}

fn canonicalize_remove_member_aliases(request: &mut RemoveMemberRequest, roster: &[RosterEntry]) {
    request.caller_identity =
        resolve_roster_alias(&request.caller_identity, &request.caller_team, roster);
    request.member = resolve_roster_alias(&request.member, &request.team, roster);
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
    ensure_member_name_available(member)?;
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

/// Produces the same collision diagnostic as durable enforcement without
/// claiming authority over the write. The caller's proposed roster replaces
/// its persisted snapshot while every other team's database-owned projection
/// stays in the comparison.
fn preflight_roster_unique_names(
    roster_store: &dyn RosterStore,
    team: &TeamName,
    proposed_roster: &[RosterEntry],
) -> Result<(), AtmError> {
    let mut names = roster_store
        .unique_names()?
        .into_iter()
        .filter(|name| name.team_name != *team)
        .collect::<Vec<_>>();
    names.extend(
        proposed_roster
            .iter()
            .map(atm_storage::RosterUniqueName::from_member),
    );
    names.sort_by(|left, right| left.unique_name.cmp(&right.unique_name));
    let mut collisions = Vec::new();
    let mut start = 0;
    while start < names.len() {
        let end = names[start + 1..]
            .iter()
            .position(|entry| entry.unique_name != names[start].unique_name)
            .map_or(names.len(), |offset| start + offset + 1);
        if end - start > 1 {
            collisions.extend_from_slice(&names[start..end]);
        }
        start = end;
    }
    if collisions.is_empty() {
        Ok(())
    } else {
        Err(atm_storage::roster_unique_name_collision_error(&collisions))
    }
}

fn validate_effective_herdr_agent_name(
    member: &RosterEntry,
    request: &UpdateMemberRequest,
) -> Result<(), AtmError> {
    let requested_herdr = matches!(
        request.local_backend,
        Some(LocalMessageReceivedBackend::Herdr { .. })
    );
    let retained_herdr = request.local_backend.is_none()
        && member
            .metadata_json
            .get("backendType")
            .and_then(serde_json::Value::as_str)
            == Some("herdr");
    if !(requested_herdr || retained_herdr) {
        return Ok(());
    }
    let alias = request
        .alias
        .as_ref()
        .and_then(|value| value.as_deref())
        .or_else(|| {
            member
                .metadata_json
                .get("alias")
                .and_then(serde_json::Value::as_str)
        });
    HerdrAgentName::new(alias.unwrap_or(member.agent_name.as_str())).map(|_| ())
}

fn ensure_member_name_available(member: &AgentName) -> Result<(), AtmError> {
    if member.as_str() == atm_storage::DAEMON_ACTOR_NAME {
        return Err(AtmError::new(
            AtmErrorCode::MessageValidationFailed,
            "atm-daemon is a reserved sender name",
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
        Some(LocalMessageReceivedBackend::Herdr { session, .. }) => {
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
    if let Some(alias) = &request.alias {
        extra.insert("alias".to_string(), json!(alias));
    }
    extra.insert(
        "joinedAt".to_string(),
        json!(Utc::now().timestamp_millis() as u64),
    );
    extra.insert(
        HOME_DIR_METADATA_KEY.to_string(),
        json!(request.member_home_dir.as_ref().display().to_string()),
    );
    if let Some(host) = &request.host {
        extra.insert(
            crate::send_to::ROSTER_HOST_METADATA_KEY.to_string(),
            json!(host.as_str()),
        );
    }

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
    if let Some(host) = &request.host {
        member.metadata_json.insert(
            crate::send_to::ROSTER_HOST_METADATA_KEY.to_string(),
            json!(host.as_str()),
        );
    }
    if let Some(alias) = &request.alias {
        match alias {
            Some(alias) => {
                member
                    .metadata_json
                    .insert("alias".to_string(), json!(alias));
            }
            None => {
                member.metadata_json.remove("alias");
            }
        }
    }
    update_member_local_backend(member, request);
}

fn update_member_local_backend(member: &mut RosterEntry, request: &UpdateMemberRequest) {
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
        Some(LocalMessageReceivedBackend::Herdr { session, .. }) => {
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
    alias: Option<&str>,
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
            if alias.is_none() {
                // A member without an alias retains its canonical Herdr name.
                HerdrAgentName::new(member.as_str())?;
            }
            let session = session
                .map(|value| {
                    crate::address::validate_path_segment(value, "herdr session")?;
                    HerdrSession::new(value)
                })
                .transpose()?;
            Ok(Some(LocalMessageReceivedBackend::Herdr {
                session,
                agent: alias.map(HerdrAgentName::new).transpose()?,
            }))
        }
        Some(other) => Err(AtmError::validation(format!(
            "unsupported local backend '{other}'; expected tmux or herdr"
        ))),
    }
}

fn parse_alias(
    alias: Option<&str>,
    clear_alias: bool,
    _allow_clear: bool,
) -> Result<Option<Option<String>>, AtmError> {
    if clear_alias && alias.is_some_and(|value| !value.trim().is_empty()) {
        return Err(AtmError::validation(
            "--alias and --clear-alias cannot be combined",
        ));
    }
    if clear_alias || alias.is_some_and(|value| value.trim().is_empty()) {
        // A blank add-member alias means no alias; on update the same inner
        // `None` remains the established explicit-clear request shape.
        return Ok(Some(None));
    }
    let Some(alias) = alias else {
        return Ok(None);
    };
    crate::address::validate_path_segment(alias, "alias")?;
    if alias == atm_storage::DAEMON_ACTOR_NAME {
        return Err(AtmError::new(
            AtmErrorCode::MessageValidationFailed,
            "atm-daemon is a reserved sender name",
        ));
    }
    Ok(Some(Some(alias.to_owned())))
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;
    use crate::test_support::{ROLE_TEAM_LEAD, TEST_TEAM};

    #[derive(Default)]
    struct TestRosterStore {
        teams: Mutex<BTreeMap<TeamName, Vec<RosterEntry>>>,
    }

    impl TestRosterStore {
        fn seed(&self, team: &TeamName, members: Vec<RosterEntry>) {
            self.teams
                .lock()
                .expect("roster lock")
                .insert(team.clone(), members);
        }

        fn members(&self, team: &TeamName) -> Vec<RosterEntry> {
            self.teams
                .lock()
                .expect("roster lock")
                .get(team)
                .cloned()
                .unwrap_or_default()
        }
    }

    impl crate::boundary::sealed::Sealed for TestRosterStore {}

    #[allow(
        deprecated,
        reason = "tests cover the retained member mutation boundary"
    )]
    impl RosterStore for TestRosterStore {
        fn replace_roster(&self, team: &TeamName, members: &[RosterEntry]) -> Result<(), AtmError> {
            self.seed(team, members.to_vec());
            Ok(())
        }

        fn load_roster(&self, team: &TeamName) -> Result<Vec<RosterEntry>, AtmError> {
            Ok(self.members(team))
        }

        fn query_membership(
            &self,
            team: &TeamName,
            member: &AgentName,
        ) -> Result<Option<RosterEntry>, AtmError> {
            Ok(self
                .members(team)
                .into_iter()
                .find(|entry| entry.agent_name == *member))
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            Ok(self
                .teams
                .lock()
                .expect("roster lock")
                .keys()
                .cloned()
                .collect())
        }

        fn health_snapshot(
            &self,
            team: &TeamName,
        ) -> Result<crate::boundary::RosterStoreHealthSnapshot, AtmError> {
            Ok(crate::boundary::RosterStoreHealthSnapshot {
                team: team.clone(),
                member_count: self.members(team).len() as u64,
                stale: false,
                refreshed_at: None,
            })
        }
    }

    fn roster_member(team: &str, agent: &str) -> RosterEntry {
        RosterEntry {
            team_name: team.parse().expect("team"),
            agent_name: agent.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: AgentType::from("worker".to_string()),
            model: ModelName::new("gpt-5").expect("model"),
            recipient_pane_id: None,
            metadata_json: serde_json::Map::new(),
        }
    }

    fn lead_member(team: &str, agent: &str) -> RosterEntry {
        let mut member = roster_member(team, agent);
        member.agent_type = AgentType::Lead;
        member
    }

    fn roster_member_with_alias(team: &str, agent: &str, alias: &str) -> RosterEntry {
        let mut member = roster_member(team, agent);
        member
            .metadata_json
            .insert("alias".to_owned(), json!(alias));
        member
    }

    #[test]
    fn add_member_rejects_reserved_daemon_name_without_changing_roster() {
        let store = TestRosterStore::default();
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let initial = vec![lead_member(TEST_TEAM, ROLE_TEAM_LEAD)];
        store.seed(&team, initial.clone());
        let root = tempfile::tempdir().expect("tempdir");
        let request = AddMemberRequest::new(
            root.path().to_path_buf(),
            TEST_TEAM,
            atm_storage::DAEMON_ACTOR_NAME,
            "worker".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("member-home"),
            None,
        )
        .expect("request");

        let error = add_member_with_roster_store(&store, request).expect_err("reserved name");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert_eq!(store.members(&team), initial);
    }

    #[test]
    fn update_member_rejects_reserved_daemon_name_without_changing_roster() {
        let store = TestRosterStore::default();
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let initial = vec![
            lead_member(TEST_TEAM, ROLE_TEAM_LEAD),
            roster_member(TEST_TEAM, atm_storage::DAEMON_ACTOR_NAME),
        ];
        store.seed(&team, initial.clone());
        let request = UpdateMemberRequest::new(
            ROLE_TEAM_LEAD.parse().expect("caller"),
            team.clone(),
            TEST_TEAM,
            atm_storage::DAEMON_ACTOR_NAME,
            None,
            None,
            None,
            Some("lead".to_owned()),
            None,
            None,
        )
        .expect("request");

        let error = update_member_with_roster_store(&store, request).expect_err("reserved name");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert_eq!(store.members(&team), initial);
    }

    /// Regression test: `apply_member_metadata_update` used to insert the
    /// roster host twice -- once under a hardcoded `"host"` string literal,
    /// once under `crate::send_to::ROSTER_HOST_METADATA_KEY` (the two
    /// happened to share the same value, masking the duplication). Only the
    /// constant-keyed insert remains, so a `--host` update must land exactly
    /// one metadata entry under `ROSTER_HOST_METADATA_KEY`: the value is
    /// correct, and the map does not silently grow past this single field.
    #[test]
    fn host_update_writes_roster_host_metadata_exactly_once() {
        let mut member = roster_member(TEST_TEAM, "cipher");
        let request = UpdateMemberRequest::new(
            ROLE_TEAM_LEAD.parse().expect("caller"),
            TEST_TEAM.parse().expect("caller team"),
            TEST_TEAM,
            "cipher",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("request")
        .with_host(Some("m5.local"))
        .expect("valid host");

        apply_member_metadata_update(&mut member, &request);

        assert_eq!(
            member.metadata_json.len(),
            1,
            "only the single host field should be written: {:?}",
            member.metadata_json
        );
        assert_eq!(
            member
                .metadata_json
                .get(crate::send_to::ROSTER_HOST_METADATA_KEY),
            Some(&json!("m5.local"))
        );
    }

    #[test]
    fn add_member_persists_alias_without_a_herdr_backend() {
        let store = TestRosterStore::default();
        let team: TeamName = TEST_TEAM.parse().expect("team");
        store.seed(&team, vec![lead_member(TEST_TEAM, ROLE_TEAM_LEAD)]);
        let root = tempfile::tempdir().expect("tempdir");
        let request = AddMemberRequest::new_with_backend(
            root.path().to_path_buf(),
            TEST_TEAM,
            "worker",
            "worker".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("worker-home"),
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: Some("Team_Lead"),
                clear_alias: false,
            },
        )
        .expect("alias-only request");

        add_member_with_roster_store(&store, request).expect("add member");

        let worker = store
            .members(&team)
            .into_iter()
            .find(|member| member.agent_name.as_str() == "worker")
            .expect("worker record");
        assert_eq!(worker.metadata_json.get("alias"), Some(&json!("Team_Lead")));
    }

    #[test]
    fn unique_name_b03_rejects_invalid_canonical_herdr_member_name() {
        let root = tempfile::tempdir().expect("tempdir");
        let error = AddMemberRequest::new_with_backend(
            root.path().to_path_buf(),
            TEST_TEAM,
            "Team-Lead",
            "worker".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("worker-home"),
            BackendOptions {
                backend: Some("herdr"),
                target: None,
                session: None,
                alias: None,
                clear_alias: false,
            },
        )
        .expect_err("canonical Herdr name must meet Herdr grammar");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
    }

    #[test]
    fn unique_name_b06_rejects_switching_an_invalid_canonical_name_to_herdr() {
        let error = UpdateMemberRequest::new_with_backend(
            ROLE_TEAM_LEAD.parse().expect("caller"),
            TEST_TEAM.parse().expect("team"),
            TEST_TEAM,
            "Team-Lead",
            None,
            None,
            None,
            None,
            None,
            BackendOptions {
                backend: Some("herdr"),
                target: None,
                session: None,
                alias: None,
                clear_alias: false,
            },
        )
        .expect_err("Herdr backend requires a valid effective name");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
    }

    #[test]
    fn unique_name_b07_rejects_reserved_daemon_alias_at_add_and_update() {
        let root = tempfile::tempdir().expect("tempdir");
        let options = BackendOptions {
            backend: None,
            target: None,
            session: None,
            alias: Some(atm_storage::DAEMON_ACTOR_NAME),
            clear_alias: false,
        };
        let add_error = AddMemberRequest::new_with_backend(
            root.path().to_path_buf(),
            TEST_TEAM,
            "worker",
            "worker".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("worker-home"),
            options,
        )
        .expect_err("reserved alias must be rejected at add-member");
        assert_eq!(add_error.code(), AtmErrorCode::MessageValidationFailed);

        let update_error = UpdateMemberRequest::new_with_backend(
            ROLE_TEAM_LEAD.parse().expect("caller"),
            TEST_TEAM.parse().expect("team"),
            TEST_TEAM,
            "worker",
            None,
            None,
            None,
            None,
            None,
            options,
        )
        .expect_err("reserved alias must be rejected at set-member");
        assert_eq!(update_error.code(), AtmErrorCode::MessageValidationFailed);
    }

    #[test]
    fn herdr_alias_uses_herdr_agent_name_validation() {
        let root = tempfile::tempdir().expect("tempdir");
        let error = AddMemberRequest::new_with_backend(
            root.path().to_path_buf(),
            TEST_TEAM,
            "worker",
            "worker".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("worker-home"),
            BackendOptions {
                backend: Some("herdr"),
                target: None,
                session: None,
                alias: Some("TeamLead"),
                clear_alias: false,
            },
        )
        .expect_err("uppercase Herdr aliases are rejected");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
    }

    #[test]
    fn unique_name_b01_rejects_invalid_atm_aliases_at_add_and_update() {
        let root = tempfile::tempdir().expect("tempdir");
        for alias in ["bad/name", "has space", "member@team", "member.name"] {
            let add_error = AddMemberRequest::new_with_backend(
                root.path().to_path_buf(),
                TEST_TEAM,
                "worker",
                "worker".to_owned(),
                "gpt-5".to_owned(),
                root.path().join("worker-home"),
                BackendOptions {
                    backend: None,
                    target: None,
                    session: None,
                    alias: Some(alias),
                    clear_alias: false,
                },
            )
            .expect_err("invalid ATM alias must be rejected at add-member");
            assert_eq!(
                add_error.code(),
                AtmErrorCode::AddressParseFailed,
                "{alias}"
            );

            let update_error = UpdateMemberRequest::new_with_backend(
                ROLE_TEAM_LEAD.parse().expect("caller"),
                TEST_TEAM.parse().expect("team"),
                TEST_TEAM,
                "worker",
                None,
                None,
                None,
                None,
                None,
                BackendOptions {
                    backend: None,
                    target: None,
                    session: None,
                    alias: Some(alias),
                    clear_alias: false,
                },
            )
            .expect_err("invalid ATM alias must be rejected at set-member");
            assert_eq!(
                update_error.code(),
                AtmErrorCode::AddressParseFailed,
                "{alias}"
            );
        }
    }

    #[test]
    fn unique_name_b04_rejects_invalid_herdr_aliases_at_add_and_update() {
        let root = tempfile::tempdir().expect("tempdir");
        let too_long = "a".repeat(33);
        for alias in [too_long.as_str(), "1member", "-member"] {
            let add_error = AddMemberRequest::new_with_backend(
                root.path().to_path_buf(),
                TEST_TEAM,
                "worker",
                "worker".to_owned(),
                "gpt-5".to_owned(),
                root.path().join("worker-home"),
                BackendOptions {
                    backend: Some("herdr"),
                    target: None,
                    session: None,
                    alias: Some(alias),
                    clear_alias: false,
                },
            )
            .expect_err("invalid Herdr alias must be rejected at add-member");
            assert_eq!(
                add_error.code(),
                AtmErrorCode::MessageValidationFailed,
                "{alias}"
            );

            let update_error = UpdateMemberRequest::new_with_backend(
                ROLE_TEAM_LEAD.parse().expect("caller"),
                TEST_TEAM.parse().expect("team"),
                TEST_TEAM,
                "worker",
                None,
                None,
                None,
                None,
                None,
                BackendOptions {
                    backend: Some("herdr"),
                    target: None,
                    session: None,
                    alias: Some(alias),
                    clear_alias: false,
                },
            )
            .expect_err("invalid Herdr alias must be rejected at set-member");
            assert_eq!(
                update_error.code(),
                AtmErrorCode::MessageValidationFailed,
                "{alias}"
            );
        }
    }

    #[test]
    fn unique_name_a09_canonical_name_of_aliased_member_is_available() {
        let store = TestRosterStore::default();
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let mut worker = roster_member(TEST_TEAM, "worker");
        worker
            .metadata_json
            .insert("alias".to_string(), json!("worker_atm-dev"));
        store.seed(&team, vec![lead_member(TEST_TEAM, ROLE_TEAM_LEAD), worker]);
        let root = tempfile::tempdir().expect("tempdir");

        let allowed = AddMemberRequest::new_with_backend(
            root.path().to_path_buf(),
            TEST_TEAM,
            "other",
            "worker".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("other-home"),
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: Some("worker"),
                clear_alias: false,
            },
        )
        .expect("request");
        add_member_with_roster_store(&store, allowed).expect("canonical name is hidden by alias");

        let rejected = AddMemberRequest::new_with_backend(
            root.path().to_path_buf(),
            TEST_TEAM,
            "another",
            "worker".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("another-home"),
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: Some("worker_atm-dev"),
                clear_alias: false,
            },
        )
        .expect("request");
        let error =
            add_member_with_roster_store(&store, rejected).expect_err("effective-name collision");
        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().contains(&format!("({TEST_TEAM}, worker)")));
        assert!(error.message().contains("--alias"));
    }

    #[test]
    fn unique_name_a14_update_alias_rejects_other_team_alias() {
        let store = TestRosterStore::default();
        let team_a: TeamName = "team-a".parse().expect("team");
        let team_b: TeamName = "team-b".parse().expect("team");
        store.seed(
            &team_a,
            vec![roster_member_with_alias("team-a", "bob", "bobby")],
        );
        store.seed(
            &team_b,
            vec![
                lead_member("team-b", ROLE_TEAM_LEAD),
                roster_member("team-b", "sam"),
            ],
        );

        let request = UpdateMemberRequest::new_with_backend(
            ROLE_TEAM_LEAD.parse().expect("caller"),
            team_b.clone(),
            "team-b",
            "sam",
            None,
            None,
            None,
            None,
            None,
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: Some("bobby"),
                clear_alias: false,
            },
        )
        .expect("request");
        let error = update_member_with_roster_store(&store, request)
            .expect_err("other-team effective alias collision");

        assert!(error.message().contains("(team-a, bob)"));
        assert!(error.message().contains("--alias"));
    }

    #[test]
    fn unique_name_a15_update_alias_may_match_aliased_member_canonical() {
        let store = TestRosterStore::default();
        let team_a: TeamName = "team-a".parse().expect("team");
        let team_b: TeamName = "team-b".parse().expect("team");
        store.seed(
            &team_a,
            vec![roster_member_with_alias("team-a", "bob", "bobby")],
        );
        store.seed(
            &team_b,
            vec![
                lead_member("team-b", ROLE_TEAM_LEAD),
                roster_member("team-b", "sam"),
            ],
        );

        let request = UpdateMemberRequest::new_with_backend(
            ROLE_TEAM_LEAD.parse().expect("caller"),
            team_b.clone(),
            "team-b",
            "sam",
            None,
            None,
            None,
            None,
            None,
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: Some("bob"),
                clear_alias: false,
            },
        )
        .expect("request");
        update_member_with_roster_store(&store, request)
            .expect("aliased canonical name is available");

        let sam = store
            .members(&team_b)
            .into_iter()
            .find(|member| member.agent_name.as_str() == "sam")
            .expect("updated member");
        assert_eq!(sam.metadata_json.get("alias"), Some(&json!("bob")));
    }

    #[test]
    fn unique_name_a16_update_alias_rejects_other_team_canonical() {
        let store = TestRosterStore::default();
        let team_a: TeamName = "team-a".parse().expect("team");
        let team_b: TeamName = "team-b".parse().expect("team");
        store.seed(&team_a, vec![roster_member("team-a", "bob")]);
        store.seed(
            &team_b,
            vec![
                lead_member("team-b", ROLE_TEAM_LEAD),
                roster_member("team-b", "sam"),
            ],
        );

        let request = UpdateMemberRequest::new_with_backend(
            ROLE_TEAM_LEAD.parse().expect("caller"),
            team_b.clone(),
            "team-b",
            "sam",
            None,
            None,
            None,
            None,
            None,
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: Some("bob"),
                clear_alias: false,
            },
        )
        .expect("request");
        let error = update_member_with_roster_store(&store, request)
            .expect_err("other-team canonical effective name collision");

        assert!(error.message().contains("(team-a, bob)"));
        assert!(error.message().contains("--alias"));
    }

    #[test]
    fn unique_name_a18_blank_add_alias_is_normalized_to_none() {
        let root = tempfile::tempdir().expect("tempdir");
        let request = AddMemberRequest::new_with_backend(
            root.path().to_path_buf(),
            TEST_TEAM,
            "worker",
            "worker".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("worker-home"),
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: Some("  "),
                clear_alias: false,
            },
        )
        .expect("blank alias is no alias");
        assert_eq!(request.alias, None);
    }

    #[test]
    fn cross_team_member_names_require_alias_but_first_occurrence_does_not() {
        let store = TestRosterStore::default();
        let first_team: TeamName = "other-team".parse().expect("team");
        store.seed(&first_team, vec![lead_member("other-team", ROLE_TEAM_LEAD)]);
        let root = tempfile::tempdir().expect("tempdir");
        let no_alias = AddMemberRequest::new_with_backend(
            root.path().to_path_buf(),
            TEST_TEAM,
            ROLE_TEAM_LEAD,
            "lead".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("duplicate-home"),
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: None,
                clear_alias: false,
            },
        )
        .expect("request");
        let error = add_member_with_roster_store(&store, no_alias).expect_err("duplicate name");
        assert!(error.message().contains("other-team"));
        assert!(error.message().contains("--alias"));

        let with_alias = AddMemberRequest::new_with_backend(
            root.path().to_path_buf(),
            TEST_TEAM,
            ROLE_TEAM_LEAD,
            "lead".to_owned(),
            "gpt-5".to_owned(),
            root.path().join("aliased-home"),
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: Some("team-lead_atm-dev"),
                clear_alias: false,
            },
        )
        .expect("aliased request");
        add_member_with_roster_store(&store, with_alias).expect("alias resolves duplicate name");
    }

    #[test]
    fn clearing_alias_is_rejected_when_the_canonical_name_is_cross_team_duplicate() {
        let store = TestRosterStore::default();
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let other: TeamName = "other-team".parse().expect("team");
        let mut local = lead_member(TEST_TEAM, ROLE_TEAM_LEAD);
        local
            .metadata_json
            .insert("alias".to_string(), json!("team-lead_atm-dev"));
        store.seed(&team, vec![local]);
        store.seed(&other, vec![lead_member("other-team", ROLE_TEAM_LEAD)]);
        let request = UpdateMemberRequest::new_with_backend(
            ROLE_TEAM_LEAD.parse().expect("caller"),
            team.clone(),
            TEST_TEAM,
            ROLE_TEAM_LEAD,
            None,
            None,
            None,
            None,
            None,
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: None,
                clear_alias: true,
            },
        )
        .expect("clear request");
        let error = update_member_with_roster_store(&store, request).expect_err("duplicate name");
        assert!(error.message().contains("other-team"));
        assert!(error.message().contains("--alias"));
    }

    #[test]
    fn update_member_can_clear_an_existing_alias() {
        let store = TestRosterStore::default();
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let mut worker = roster_member(TEST_TEAM, "worker");
        worker
            .metadata_json
            .insert("alias".to_string(), json!("worker_atm-dev"));
        store.seed(&team, vec![lead_member(TEST_TEAM, ROLE_TEAM_LEAD), worker]);
        let request = UpdateMemberRequest::new_with_backend(
            ROLE_TEAM_LEAD.parse().expect("caller"),
            team.clone(),
            TEST_TEAM,
            "worker",
            None,
            None,
            None,
            None,
            None,
            BackendOptions {
                backend: None,
                target: None,
                session: None,
                alias: None,
                clear_alias: true,
            },
        )
        .expect("clear-alias request");

        update_member_with_roster_store(&store, request).expect("update member");

        let worker = store
            .members(&team)
            .into_iter()
            .find(|member| member.agent_name.as_str() == "worker")
            .expect("worker record");
        assert!(!worker.metadata_json.contains_key("alias"));
    }

    #[test]
    fn update_and_remove_member_canonicalize_alias_arguments_before_persistence() {
        let store = TestRosterStore::default();
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let mut lead = lead_member(TEST_TEAM, ROLE_TEAM_LEAD);
        lead.metadata_json
            .insert("alias".to_owned(), json!("atm-lead"));
        let mut worker = roster_member(TEST_TEAM, "worker");
        worker
            .metadata_json
            .insert("alias".to_owned(), json!("atm-worker"));
        store.seed(&team, vec![lead, worker]);

        let update = UpdateMemberRequest::new(
            "atm-lead".parse().expect("alias caller"),
            team.clone(),
            TEST_TEAM,
            "atm-worker",
            None,
            None,
            None,
            Some("lead".to_owned()),
            None,
            None,
        )
        .expect("update request");
        let outcome =
            update_member_with_roster_store(&store, update).expect("update through alias");
        assert_eq!(outcome.member.as_str(), "worker");

        let remove = RemoveMemberRequest::new(
            "atm-lead".parse().expect("alias caller"),
            team.clone(),
            TEST_TEAM,
            "atm-worker",
        )
        .expect("remove request");
        let outcome =
            remove_member_with_roster_store(&store, remove).expect("remove through alias");
        assert_eq!(outcome.member.as_str(), "worker");
        assert!(
            store
                .members(&team)
                .iter()
                .all(|member| member.agent_name.as_str() != "worker")
        );
    }
}
