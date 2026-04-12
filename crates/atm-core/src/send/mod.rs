//! Send command service implementation and post-send hook handling.

use std::collections::BTreeSet;
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, json};
use tracing::{debug, warn};

use crate::address::AgentAddress;
use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::persistence;
use crate::schema::{LegacyMessageId, MessageEnvelope};
use crate::types::{AgentName, IsoTimestamp, TeamName};

pub(crate) mod file_policy;
pub(crate) mod input;
pub(crate) mod summary;

#[derive(Debug, Clone)]
pub enum SendMessageSource {
    Inline(String),
    Stdin,
    File {
        path: PathBuf,
        message: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct SendRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub sender_override: Option<AgentName>,
    pub to: String,
    pub team_override: Option<TeamName>,
    pub message_source: SendMessageSource,
    pub summary_override: Option<String>,
    pub requires_ack: bool,
    pub task_id: Option<String>,
    pub dry_run: bool,
}

/// Result of sending one ATM mailbox message.
#[derive(Debug, Clone, Serialize)]
pub struct SendOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub agent: AgentName,
    pub sender: String,
    pub outcome: &'static str,
    pub message_id: LegacyMessageId,
    pub requires_ack: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dry_run: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SendAlertState {
    #[serde(default)]
    missing_team_config_keys: BTreeSet<String>,
}

/// Send one mailbox message to a team member.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamNotFound`],
/// [`crate::error_codes::AtmErrorCode::AgentNotFound`],
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`],
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`],
/// [`crate::error_codes::AtmErrorCode::FilePolicyRejected`],
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`], or
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`] when sender
/// identity cannot be resolved, recipient or team validation fails,
/// message/file-policy validation fails, or mailbox persistence fails.
pub fn send_mail(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
) -> Result<SendOutcome, AtmError> {
    let config = config::load_config(&request.current_dir)?;
    let canonical_sender =
        identity::resolve_sender_identity(request.sender_override.as_deref(), config.as_ref())?;
    let recipient = resolve_recipient(
        &request.to,
        request.team_override.as_deref(),
        config.as_ref(),
    )?;
    let sender_team = config::resolve_team(None, config.as_ref());
    let sender = display_sender_identity(
        &canonical_sender,
        request.sender_override.as_deref(),
        sender_team.as_deref(),
        &recipient.team,
        config.as_ref(),
    );

    let team_dir = home::team_dir_from_home(&request.home_dir, &recipient.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&recipient.team));
    }

    let inbox_path =
        home::inbox_path_from_home(&request.home_dir, &recipient.team, &recipient.agent)?;
    let mut warnings = Vec::new();

    match config::load_team_config(&team_dir) {
        Ok(team_config) => {
            clear_missing_team_config_alert(&request.home_dir, &team_dir);
            if !team_config
                .members
                .iter()
                .any(|member| member.name == recipient.agent)
            {
                return Err(AtmError::agent_not_found(&recipient.agent, &recipient.team));
            }
        }
        Err(error) if error.is_missing_document() => {
            if !inbox_path.exists() {
                return Err(AtmError::missing_document(format!(
                    "team config is missing at {} and inbox {} does not exist, so send cannot safely proceed",
                    team_dir.join("config.json").display(),
                    inbox_path.display()
                ))
                .with_recovery(
                    "Restore config.json for the team or create the intended inbox by an approved workflow before retrying.",
                ));
            }

            warnings.push(format!(
                "warning: team config is missing at {}; send used existing inbox fallback for {}@{}. Restore the team config.",
                team_dir.join("config.json").display(),
                recipient.agent,
                recipient.team
            ));

            if !request.dry_run {
                notify_team_lead_missing_config(
                    &request.home_dir,
                    &team_dir,
                    &recipient.team,
                    &recipient.agent,
                );
            }
        }
        Err(error) => return Err(error),
    }

    let task_id = input::validate_task_id(request.task_id)?;
    let requires_ack = request.requires_ack || task_id.is_some();
    let body = resolve_message_body(
        &request.message_source,
        &request.current_dir,
        &request.home_dir,
        &recipient.team,
    )?;
    let summary = summary::build_summary(&body, request.summary_override);
    let message_id = LegacyMessageId::new();
    let timestamp = IsoTimestamp::now();

    if !request.dry_run {
        let mut extra = Map::new();
        if sender != canonical_sender {
            set_canonical_sender_metadata(
                &mut extra,
                &qualified_sender_identity(&canonical_sender, sender_team.as_deref()),
            );
        }
        let envelope = MessageEnvelope {
            from: sender.clone(),
            text: body.clone(),
            timestamp,
            read: false,
            source_team: sender_team.clone().or(Some(recipient.team.clone())),
            summary: Some(summary.clone()),
            message_id: Some(message_id),
            pending_ack_at: requires_ack.then_some(timestamp),
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: task_id.clone(),
            extra,
        };
        mailbox::append_message(&inbox_path, &envelope)?;
    }

    let mut outcome = SendOutcome {
        action: "send",
        team: recipient.team.clone().into(),
        agent: recipient.agent.clone().into(),
        sender: sender.clone(),
        outcome: if request.dry_run { "dry_run" } else { "sent" },
        message_id,
        requires_ack,
        task_id: task_id.clone(),
        summary: Some(summary),
        message: request.dry_run.then_some(body.clone()),
        warnings,
        dry_run: request.dry_run,
    };

    if !request.dry_run {
        maybe_run_post_send_hook(
            &mut outcome.warnings,
            config.as_ref(),
            PostSendHookContext {
                sender: &canonical_sender,
                sender_team: sender_team.as_deref(),
                recipient: &recipient,
                message_id,
                requires_ack,
                task_id: task_id.as_deref(),
            },
        );
    }

    let _ = observability.emit(CommandEvent {
        command: "send",
        action: "send",
        outcome: outcome.outcome,
        team: outcome.team.to_string(),
        agent: outcome.agent.to_string(),
        sender: canonical_sender,
        message_id: Some(outcome.message_id),
        requires_ack: outcome.requires_ack,
        dry_run: outcome.dry_run,
        task_id,
        error_code: None,
        error_message: None,
    });

    Ok(outcome)
}

#[derive(Debug)]
struct ResolvedRecipient {
    agent: String,
    team: String,
}

struct PostSendHookContext<'a> {
    sender: &'a str,
    sender_team: Option<&'a str>,
    recipient: &'a ResolvedRecipient,
    message_id: LegacyMessageId,
    requires_ack: bool,
    task_id: Option<&'a str>,
}

fn resolve_recipient(
    target_address: &str,
    team_override: Option<&str>,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedRecipient, AtmError> {
    let parsed: AgentAddress = target_address.parse()?;
    let team = parsed
        .team
        .or_else(|| config::resolve_team(team_override, config))
        .ok_or_else(AtmError::team_unavailable)?;

    Ok(ResolvedRecipient {
        agent: config::aliases::resolve_agent(&parsed.agent, config),
        team,
    })
}

fn resolve_message_body(
    source: &SendMessageSource,
    current_dir: &Path,
    home_dir: &Path,
    team_name: &str,
) -> Result<String, AtmError> {
    match source {
        SendMessageSource::Inline(message) => input::validate_message_text(message.clone()),
        SendMessageSource::Stdin => input::read_message_from_stdin(),
        SendMessageSource::File { path, message } => file_policy::process_file_reference(
            path,
            message.as_deref(),
            team_name,
            current_dir,
            home_dir,
        ),
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn notify_team_lead_missing_config(home_dir: &Path, team_dir: &Path, team: &str, recipient: &str) {
    let alert_key = missing_team_config_alert_key(team_dir);
    if !register_missing_team_config_alert(home_dir, &alert_key) {
        return;
    }

    let team_lead_inbox = match home::inbox_path_from_home(home_dir, team, "team-lead") {
        Ok(path) => path,
        Err(error) => {
            warn!(%error, team, "failed to resolve team-lead inbox for missing-config notice");
            return;
        }
    };

    if !team_lead_inbox.exists() {
        return;
    }

    let config_path = team_dir.join("config.json");
    let timestamp = IsoTimestamp::now();
    let mut extra = Map::new();
    extra.insert(
        "atmAlertKind".into(),
        serde_json::Value::String("missing_team_config".into()),
    );
    extra.insert(
        "missingConfigPath".into(),
        serde_json::Value::String(config_path.display().to_string()),
    );

    let notice = MessageEnvelope {
        from: format!("atm-identity-missing@{team}"),
        text: format!(
            "ATM warning: send used existing inbox fallback for {recipient}@{team} because team config is missing at {}. Please restore config.json.",
            config_path.display()
        ),
        timestamp,
        read: false,
        source_team: Some(team.to_string()),
        summary: Some(format!(
            "ATM warning: missing team config fallback used for {recipient}@{team}"
        )),
        message_id: Some(LegacyMessageId::new()),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra,
    };

    if let Err(error) = mailbox::append_message(&team_lead_inbox, &notice) {
        warn!(
            %error,
            path = %team_lead_inbox.display(),
            "failed to append missing-config notice to team-lead inbox"
        );
    }
}

fn display_sender_identity(
    canonical_sender: &str,
    sender_override: Option<&str>,
    sender_team: Option<&str>,
    recipient_team: &str,
    config: Option<&config::AtmConfig>,
) -> String {
    let cross_team = sender_team.is_some_and(|team| team != recipient_team);
    if !cross_team {
        return canonical_sender.to_string();
    }

    if let Some(sender_override) = sender_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && config::aliases::resolve_agent(sender_override, config) == canonical_sender
    {
        return sender_override.to_string();
    }

    config::aliases::preferred_alias(canonical_sender, config)
        .unwrap_or_else(|| canonical_sender.to_string())
}

fn qualified_sender_identity(sender: &str, sender_team: Option<&str>) -> String {
    sender_team
        .map(|team| format!("{sender}@{team}"))
        .unwrap_or_else(|| sender.to_string())
}

fn set_canonical_sender_metadata(extra: &mut Map<String, serde_json::Value>, canonical_from: &str) {
    let metadata = extra
        .entry("metadata".to_string())
        .or_insert_with(|| serde_json::Value::Object(Map::new()));
    if !metadata.is_object() {
        *metadata = serde_json::Value::Object(Map::new());
    }
    let Some(metadata) = metadata.as_object_mut() else {
        return;
    };
    let atm = metadata
        .entry("atm".to_string())
        .or_insert_with(|| serde_json::Value::Object(Map::new()));
    if !atm.is_object() {
        *atm = serde_json::Value::Object(Map::new());
    }
    let Some(atm) = atm.as_object_mut() else {
        return;
    };
    atm.insert(
        "fromIdentity".to_string(),
        serde_json::Value::String(canonical_from.to_string()),
    );
}

fn maybe_run_post_send_hook(
    warnings: &mut Vec<String>,
    config: Option<&config::AtmConfig>,
    context: PostSendHookContext<'_>,
) {
    const POST_SEND_HOOK_TIMEOUT: Duration = Duration::from_secs(5);

    let Some(config) = config else {
        return;
    };
    let Some(command_argv) = config.post_send_hook.as_ref() else {
        return;
    };
    if !config
        .post_send_hook_members
        .iter()
        .any(|member| member == context.sender)
    {
        debug!(
            sender = context.sender,
            allowlist = ?config.post_send_hook_members,
            "post-send hook skipped: sender is not in post_send_hook_members"
        );
        return;
    }

    let mut argv = command_argv.iter();
    let Some(command_path) = argv.next() else {
        return;
    };
    let command_path = {
        let path = PathBuf::from(command_path);
        if path.is_absolute() {
            path
        } else {
            config.config_root.join(path)
        }
    };

    let payload = json!({
        "from": qualified_sender_identity(context.sender, context.sender_team),
        "to": format!("{}@{}", context.recipient.agent, context.recipient.team),
        "message_id": context.message_id.to_string(),
        "requires_ack": context.requires_ack,
        "task_id": context.task_id,
    });

    let mut command = Command::new(&command_path);
    command
        .args(argv)
        .current_dir(&config.config_root)
        .env("ATM_POST_SEND", payload.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            warnings.push(format!(
                "warning: post-send hook failed to start from {}: {error}. Check that post_send_hook in .atm.toml points to a valid executable.",
                command_path.display()
            ));
            return;
        }
    };

    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    warnings.push(format!(
                        "warning: post-send hook exited unsuccessfully from {} with status {status}. Check the hook script for errors; it exited with a non-zero status.",
                        command_path.display()
                    ));
                }
                return;
            }
            Ok(None) if started_at.elapsed() < POST_SEND_HOOK_TIMEOUT => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                warnings.push(format!(
                    "warning: post-send hook timed out after {}s for {}. The hook script exceeded the 5-second timeout; ensure it exits promptly.",
                    POST_SEND_HOOK_TIMEOUT.as_secs(),
                    command_path.display()
                ));
                return;
            }
            Err(error) => {
                warnings.push(format!(
                    "warning: post-send hook status check failed for {}: {error}",
                    command_path.display()
                ));
                return;
            }
        }
    }
}

fn missing_team_config_alert_key(team_dir: &Path) -> String {
    team_dir.join("config.json").display().to_string()
}

fn register_missing_team_config_alert(home_dir: &Path, key: &str) -> bool {
    let state_path = send_alert_state_path(home_dir);
    let lock_path = send_alert_lock_path(home_dir);
    let Some(_guard) = acquire_send_alert_lock(&lock_path) else {
        warn!(
            path = %lock_path.display(),
            "failed to acquire send alert lock; skipping team-lead notification"
        );
        return false;
    };

    let mut state = match load_send_alert_state(&state_path) {
        Ok(state) => state,
        Err(error) => {
            warn!(
                %error,
                path = %state_path.display(),
                "failed to read send state file - defaulting to empty state"
            );
            SendAlertState::default()
        }
    };
    if state.missing_team_config_keys.contains(key) {
        return false;
    }

    state.missing_team_config_keys.insert(key.to_string());
    if let Err(error) = save_send_alert_state(&state_path, &state) {
        warn!(%error, path = %state_path.display(), "failed to save send alert dedup state");
    }
    true
}

fn clear_missing_team_config_alert(home_dir: &Path, team_dir: &Path) {
    let state_path = send_alert_state_path(home_dir);
    let lock_path = send_alert_lock_path(home_dir);
    let Some(_guard) = acquire_send_alert_lock(&lock_path) else {
        warn!(
            path = %lock_path.display(),
            "failed to acquire send alert lock while clearing dedup state"
        );
        return;
    };

    let Ok(mut state) = load_send_alert_state(&state_path) else {
        return;
    };

    let key = missing_team_config_alert_key(team_dir);
    if !state.missing_team_config_keys.remove(&key) {
        return;
    }

    if let Err(error) = save_send_alert_state(&state_path, &state) {
        warn!(%error, path = %state_path.display(), "failed to clear send alert dedup state");
    }
}

fn send_alert_state_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".config").join("atm").join("state.json")
}

fn send_alert_lock_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".config").join("atm").join("state.lock")
}

fn load_send_alert_state(path: &Path) -> Result<SendAlertState, AtmError> {
    if !path.exists() {
        return Ok(SendAlertState::default());
    }

    let raw = fs::read_to_string(path).map_err(|error| {
        AtmError::new(
            crate::error::AtmErrorKind::Config,
            format!(
                "failed to read send alert state at {}: {error}",
                path.display()
            ),
        )
        .with_recovery("Check ATM config-state permissions or remove the damaged state file before retrying the send command.")
        .with_source(error)
    })?;
    serde_json::from_str(&raw).map_err(|error| {
        AtmError::new(
            crate::error::AtmErrorKind::Config,
            format!(
                "failed to parse send alert state at {}: {error}",
                path.display()
            ),
        )
        .with_recovery(
            "Remove the malformed send alert state file so ATM can recreate it on the next send.",
        )
        .with_source(error)
    })
}

fn save_send_alert_state(path: &Path, state: &SendAlertState) -> Result<(), AtmError> {
    let data = serde_json::to_vec(state)?;
    persistence::atomic_write_bytes(
        path,
        &data,
        crate::error::AtmErrorKind::Config,
        "send alert state",
        "Check ATM config-state directory permissions and rerun the send operation.",
    )
}

fn acquire_send_alert_lock(path: &Path) -> Option<SendAlertLock> {
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        warn!(
            %error,
            path = %parent.display(),
            "failed to create send alert lock directory"
        );
        return None;
    }

    for _ in 0..100 {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                let pid = std::process::id().to_string();
                if let Err(error) = std::io::Write::write_all(&mut file, pid.as_bytes()) {
                    warn!(%error, path = %path.display(), "failed to write send alert lock pid");
                    let _ = fs::remove_file(path);
                    return None;
                }
                return Some(SendAlertLock {
                    path: path.to_path_buf(),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if evict_stale_send_alert_lock(path) {
                    continue;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                warn!(%error, path = %path.display(), "failed to create send alert lock");
                return None;
            }
        }
    }

    None
}

fn evict_stale_send_alert_lock(path: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        return false;
    };
    if process_is_alive(pid) {
        return false;
    }

    match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            warn!(%error, path = %path.display(), pid, "failed to evict stale send alert lock");
            false
        }
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let pid: libc::pid_t = match pid.try_into() {
        Ok(pid) => pid,
        Err(_) => return false,
    };
    // SAFETY: libc::kill with signal 0 performs an existence/permission check
    // only and does not deliver a signal.
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return true;
    }

    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: OpenProcess is called read-only for process liveness inspection.
    let process_id: u32 = pid;
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle == ptr::null_mut() {
        return false;
    }

    let mut exit_code = 0u32;
    // SAFETY: handle was returned by OpenProcess and remains valid until CloseHandle below.
    let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    // SAFETY: handle was opened successfully above and must be closed once.
    unsafe { CloseHandle(handle) };
    ok != 0 && exit_code == STILL_ACTIVE as u32
}

struct SendAlertLock {
    path: PathBuf,
}

impl Drop for SendAlertLock {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            warn!(
                %error,
                path = %self.path.display(),
                "failed to remove send alert lock"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        SendAlertState, acquire_send_alert_lock, load_send_alert_state, process_is_alive,
        save_send_alert_state, send_alert_lock_path, send_alert_state_path,
    };

    #[test]
    fn load_send_alert_state_parse_errors_are_config_errors() {
        let tempdir = tempdir().expect("tempdir");
        let path = send_alert_state_path(tempdir.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("state dir");
        }
        fs::write(&path, "{not-json").expect("state file");

        let error = load_send_alert_state(&path).expect_err("malformed state");
        assert!(error.is_config());
    }

    #[test]
    fn save_send_alert_state_round_trips() {
        let tempdir = tempdir().expect("tempdir");
        let path = send_alert_state_path(tempdir.path());
        let mut state = SendAlertState::default();
        state
            .missing_team_config_keys
            .insert("teams/atm-dev/config.json".to_string());

        save_send_alert_state(&path, &state).expect("save");
        let loaded = load_send_alert_state(&path).expect("load");
        assert_eq!(
            loaded.missing_team_config_keys,
            state.missing_team_config_keys
        );
    }

    #[test]
    fn process_is_alive_reports_current_process() {
        assert!(process_is_alive(std::process::id()));
    }

    #[test]
    fn acquire_send_alert_lock_evicts_stale_pid_lock() {
        let tempdir = tempdir().expect("tempdir");
        let path = send_alert_lock_path(tempdir.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("lock dir");
        }
        fs::write(&path, u32::MAX.to_string()).expect("stale lock");

        let guard = acquire_send_alert_lock(&path).expect("acquire lock");
        let pid = fs::read_to_string(&path).expect("lock contents");
        assert_eq!(pid.trim(), std::process::id().to_string());
        drop(guard);
        assert!(!path.exists());
    }
}
