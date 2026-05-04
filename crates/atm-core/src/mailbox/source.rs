use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::warn;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorCode, AtmErrorKind};
use crate::home;
use crate::schema::MessageEnvelope;
use crate::types::{AgentName, SourceIndex, TeamName};

#[derive(Debug, Clone)]
pub(crate) struct SourceFile {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
}

#[derive(Debug, Clone)]
pub(crate) struct SourcedMessage {
    pub envelope: MessageEnvelope,
    pub source_path: PathBuf,
    pub source_index: SourceIndex,
}

#[derive(Debug)]
pub(crate) struct ResolvedTarget {
    pub agent: AgentName,
    pub team: TeamName,
    pub explicit: bool,
}

pub(crate) fn resolve_target(
    target_address: Option<&AgentAddress>,
    actor: &AgentName,
    team_override: Option<&TeamName>,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedTarget, AtmError> {
    let Some(target_address) = target_address else {
        let team = config::resolve_team(team_override.map(TeamName::as_str), config)
            .ok_or_else(AtmError::team_unavailable)?;
        return Ok(ResolvedTarget {
            agent: actor.clone(),
            team,
            explicit: false,
        });
    };

    let team = target_address
        .team
        .as_deref()
        .and_then(|team| team.parse().ok())
        .or_else(|| config::resolve_team(team_override.map(TeamName::as_str), config))
        .ok_or_else(AtmError::team_unavailable)?;
    let agent = config::aliases::resolve_agent(&target_address.agent, config);

    Ok(ResolvedTarget {
        agent: AgentName::from_validated(agent),
        team,
        explicit: true,
    })
}

pub(crate) fn discover_origin_inboxes(
    inboxes_dir: &Path,
    agent: &AgentName,
) -> Result<Vec<PathBuf>, AtmError> {
    if !inboxes_dir.exists() {
        return Ok(Vec::new());
    }

    let prefix = format!("{}.", agent.as_str());
    let primary = format!("{}.json", agent.as_str());
    if let Some(error) = forced_source_discovery_fault() {
        return Err(origin_inbox_enumeration_error(inboxes_dir, agent, error));
    }

    let entries = fs::read_dir(inboxes_dir).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxRead,
            format!(
                "failed to read inbox directory {}: {error}",
                inboxes_dir.display()
            ),
        )
        .with_recovery(
            "Check inbox directory permissions and ensure the source inbox directory still exists before retrying the ATM command.",
        )
        .with_source(error)
    })?;

    let mut paths = Vec::new();
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(error) => {
                let enumerated = origin_inbox_enumeration_error(inboxes_dir, agent, error);
                warn!(
                    code = %AtmErrorCode::WarningOriginInboxEntrySkipped,
                    inbox_dir = %inboxes_dir.display(),
                    agent = agent.as_str(),
                    %enumerated,
                    "failed while enumerating origin inbox entries; aborting source discovery"
                );
                return Err(enumerated);
            }
        };
        if path
            .file_name()
            .and_then(|value| value.to_str())
            .map(|name| name.starts_with(&prefix) && name.ends_with(".json") && name != primary)
            .unwrap_or(false)
        {
            paths.push(path);
        }
    }

    paths.sort();
    Ok(paths)
}

pub(crate) fn discover_source_paths(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<PathBuf>, AtmError> {
    let inbox_path = home::inbox_path_from_home(home_dir, team.as_str(), agent.as_str())?;
    let inboxes_dir = inbox_path
        .parent()
        .ok_or_else(|| AtmError::mailbox_read("inbox path has no parent directory"))?;
    let inboxes_dir = inboxes_dir.to_path_buf();

    let mut paths = Vec::new();
    if inbox_path.exists() {
        paths.push(inbox_path);
    }
    paths.extend(discover_origin_inboxes(&inboxes_dir, agent)?);
    paths.sort_by_key(|path| path.to_string_lossy().into_owned());
    paths.dedup();
    Ok(paths)
}

pub(crate) fn rediscover_and_validate_source_paths(
    locked_paths: &[PathBuf],
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<PathBuf>, AtmError> {
    let rediscovered = discover_source_paths(home_dir, team, agent)?;
    if rediscovered != locked_paths {
        return Err(AtmError::mailbox_lock(
            "source path set changed between discovery and lock acquisition",
        )
        .with_recovery(
            "Retry after the competing ATM operation completes so ATM can rediscover the stable inbox set.",
        ));
    }
    Ok(rediscovered)
}

fn origin_inbox_enumeration_error(
    inboxes_dir: &Path,
    agent: &AgentName,
    error: io::Error,
) -> AtmError {
    AtmError::new(
        AtmErrorKind::MailboxRead,
        format!(
            "failed to enumerate origin inbox entries for agent '{}' in {}: {error}",
            agent.as_str(),
            inboxes_dir.display()
        ),
    )
    .with_recovery(
        "Check inbox directory permissions and ensure the source inbox directory can be enumerated completely before retrying the ATM command.",
    )
    .with_source(error)
}

#[cfg(test)]
fn forced_source_discovery_fault() -> Option<io::Error> {
    if source_discovery_fault_test_override::get()
        || std::env::var_os("ATM_TEST_FORCE_SOURCE_DISCOVERY_FAULT").is_some()
    {
        return Some(io::Error::other(
            "synthetic read_dir entry enumeration fault",
        ));
    }
    None
}

#[cfg(not(test))]
fn forced_source_discovery_fault() -> Option<io::Error> {
    std::env::var_os("ATM_TEST_FORCE_SOURCE_DISCOVERY_FAULT")
        .map(|_| io::Error::other("synthetic read_dir entry enumeration fault"))
}

#[cfg(test)]
mod source_discovery_fault_test_override {
    use std::cell::Cell;

    thread_local! {
        static OVERRIDE: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn get() -> bool {
        OVERRIDE.with(Cell::get)
    }

    pub(super) fn set(enabled: bool) -> bool {
        OVERRIDE.with(|cell| {
            let original = cell.get();
            cell.set(enabled);
            original
        })
    }
}

#[cfg(test)]
pub(crate) struct ScopedSourceDiscoveryFaultOverride {
    original: bool,
}

#[cfg(test)]
impl ScopedSourceDiscoveryFaultOverride {
    pub(crate) fn enable() -> Self {
        Self {
            original: source_discovery_fault_test_override::set(true),
        }
    }
}

#[cfg(test)]
impl Drop for ScopedSourceDiscoveryFaultOverride {
    fn drop(&mut self) {
        source_discovery_fault_test_override::set(self.original);
    }
}

pub(crate) fn load_source_files(paths: &[PathBuf]) -> Result<Vec<SourceFile>, AtmError> {
    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        if !path.exists() {
            return Err(AtmError::mailbox_read(format!(
                "mailbox file disappeared before locked read completed: {}",
                path.display()
            ))
            .with_recovery(
                "Retry after the competing ATM operation completes, or verify the team inbox files still exist.",
            ));
        }

        let messages = super::read_messages(path)?;
        sources.push(SourceFile {
            path: path.clone(),
            messages,
        });
    }

    Ok(sources)
}

#[cfg(test)]
// rule-008: allow-start -- source-discovery tests use explicit mailbox and
// alias fixture strings to keep path and alias assertions readable.
mod tests {
    use std::collections::BTreeMap;
    use std::io;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        ScopedSourceDiscoveryFaultOverride, discover_origin_inboxes, load_source_files,
        origin_inbox_enumeration_error, rediscover_and_validate_source_paths, resolve_target,
    };
    use crate::config::AtmConfig;
    use crate::types::{AgentName, TeamName};

    const TEST_TEAM: &str = "test-team";
    const TEST_ACTOR: &str = "test-sender";
    const TEST_LEAD: &str = "test-lead";

    #[test]
    fn discover_origin_inboxes_ignores_primary_and_sorts_matches() {
        let tempdir = tempdir().expect("tempdir");
        let inboxes = tempdir.path();
        std::fs::write(inboxes.join(format!("{TEST_ACTOR}.json")), "").expect("primary");
        std::fs::write(inboxes.join(format!("{TEST_ACTOR}.host-b.json")), "").expect("host b");
        std::fs::write(inboxes.join(format!("{TEST_ACTOR}.host-a.json")), "").expect("host a");
        std::fs::write(inboxes.join("other.json"), "").expect("other");

        let discovered = discover_origin_inboxes(inboxes, &TEST_ACTOR.parse().expect("agent"))
            .expect("discover");
        assert_eq!(
            discovered,
            vec![
                inboxes.join(format!("{TEST_ACTOR}.host-a.json")),
                inboxes.join(format!("{TEST_ACTOR}.host-b.json"))
            ]
        );
    }

    #[test]
    fn origin_inbox_enumeration_error_is_mailbox_read_failure() {
        let error = origin_inbox_enumeration_error(
            Path::new("test-inbox-dir"),
            &TEST_ACTOR.parse().expect("agent"),
            io::Error::other("synthetic"),
        );

        assert!(error.is_mailbox_read());
        assert!(
            error
                .message
                .contains("failed to enumerate origin inbox entries")
        );
    }

    #[test]
    fn resolve_target_canonicalizes_alias_before_mailbox_lookup() {
        let mut aliases = BTreeMap::new();
        aliases.insert("tl".to_string(), TEST_LEAD.to_string());
        let config = AtmConfig {
            default_team: Some(TEST_TEAM.parse().expect("team")),
            aliases,
            ..Default::default()
        };

        let target = resolve_target(
            Some(&"tl".parse().expect("address")),
            &TEST_ACTOR.parse().expect("agent"),
            Some(&TEST_TEAM.parse().expect("team")),
            Some(&config),
        )
        .expect("target");
        assert_eq!(target.agent, TEST_LEAD);
        assert_eq!(target.team, TEST_TEAM);
        assert!(target.explicit);
    }

    #[test]
    fn load_source_files_reports_disappearing_mailbox() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join(format!("{TEST_ACTOR}.json"));
        std::fs::write(&path, "").expect("mailbox");
        std::fs::remove_file(&path).expect("remove");

        let error = load_source_files(&[path]).expect_err("missing mailbox");
        assert!(error.is_mailbox_read());
        assert!(error.message.contains("disappeared"));
    }

    #[test]
    fn discover_origin_inboxes_reports_synthetic_fault() {
        let tempdir = tempdir().expect("tempdir");
        let inboxes = tempdir.path();
        std::fs::write(inboxes.join(format!("{TEST_ACTOR}.host-a.json")), "").expect("host a");
        let _fault = ScopedSourceDiscoveryFaultOverride::enable();

        let error = discover_origin_inboxes(inboxes, &TEST_ACTOR.parse().expect("agent"))
            .expect_err("synthetic fault");
        assert!(error.is_mailbox_read());
        assert!(
            error
                .message
                .contains("failed to enumerate origin inbox entries")
        );
    }

    #[test]
    fn rediscover_and_validate_source_paths_reports_drift() {
        let tempdir = tempdir().expect("tempdir");
        let home = tempdir.path();
        let inboxes = home
            .join(".claude")
            .join("teams")
            .join(TEST_TEAM)
            .join("inboxes");
        std::fs::create_dir_all(&inboxes).expect("inboxes");
        let locked = inboxes.join(format!("{TEST_ACTOR}.json"));
        let added = inboxes.join(format!("{TEST_ACTOR}.host-a.json"));
        std::fs::write(&locked, "").expect("primary");

        let discovered = super::discover_source_paths(
            home,
            &TEST_TEAM.parse().expect("team"),
            &TEST_ACTOR.parse().expect("agent"),
        )
        .expect("discover");
        std::fs::write(&added, "").expect("origin");

        let error = rediscover_and_validate_source_paths(
            &discovered,
            home,
            &TEST_TEAM.parse().expect("team"),
            &TEST_ACTOR.parse().expect("agent"),
        )
        .expect_err("drift error");
        assert!(error.is_mailbox_lock());
        assert!(error.message.contains("source path set changed"));
    }

    #[test]
    fn discover_source_paths_rejects_invalid_team_segment() {
        assert!("../evil".parse::<TeamName>().is_err());
    }

    #[test]
    fn discover_source_paths_rejects_invalid_agent_segment() {
        assert!("../evil".parse::<AgentName>().is_err());
    }
}
// rule-008: allow-end
