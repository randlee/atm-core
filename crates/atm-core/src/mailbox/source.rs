#![allow(
    dead_code,
    reason = "Phase AD obsolete: retained only for historical Claude compatibility paths until the later deletion sprint removes the remaining helpers."
)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::warn;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorCode};
use crate::home;
use crate::schema::InboxMessage;
use crate::types::{AgentName, SourceIndex, TeamName};

#[derive(Debug, Clone)]
pub(crate) struct SourceFile {
    pub path: PathBuf,
    pub messages: Vec<InboxMessage>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
pub(crate) struct SourcedMessage {
    pub envelope: InboxMessage,
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
    caller_team: &TeamName,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedTarget, AtmError> {
    let Some(target_address) = target_address else {
        return Ok(ResolvedTarget {
            agent: actor.clone(),
            team: caller_team.clone(),
            explicit: false,
        });
    };

    let team = target_address
        .team()
        .cloned()
        .unwrap_or_else(|| caller_team.clone());
    Ok(ResolvedTarget {
        agent: config::aliases::resolve_agent_name(target_address.agent(), config)?,
        team,
        explicit: true,
    })
}

pub(crate) fn discover_origin_inboxes(
    inboxes_dir: &Path,
    agent: &str,
) -> Result<Vec<PathBuf>, AtmError> {
    if !inboxes_dir.exists() {
        return Ok(Vec::new());
    }

    let prefix = format!("{agent}.");
    let primary = format!("{agent}.json");
    if let Some(error) = forced_source_discovery_fault() {
        return Err(origin_inbox_enumeration_error(inboxes_dir, agent, error));
    }

    let entries = fs::read_dir(inboxes_dir).map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!(
                "failed to read inbox directory {}: {error}",
                inboxes_dir.display()
            ),
        )
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
                    agent,
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
    let inbox_path = home::inbox_path_from_home(home_dir, team, agent)?;
    let inboxes_dir = inbox_path
        .parent()
        .ok_or_else(|| AtmError::mailbox_read("inbox path has no parent directory"))?;
    let inboxes_dir = inboxes_dir.to_path_buf();

    let mut paths = Vec::new();
    if inbox_path.exists() {
        paths.push(inbox_path);
    }
    paths.extend(discover_origin_inboxes(&inboxes_dir, agent.as_str())?);
    paths.sort_by_key(|path| path.to_string_lossy().into_owned());
    paths.dedup();
    Ok(paths)
}

#[cfg(test)]
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
        ));
    }
    Ok(rediscovered)
}

fn origin_inbox_enumeration_error(inboxes_dir: &Path, agent: &str, error: io::Error) -> AtmError {
    AtmError::new(
        AtmErrorCode::MailboxReadFailed,
        format!(
            "failed to enumerate origin inbox entries for agent '{agent}' in {}: {error}",
            inboxes_dir.display()
        ),
    )
}

fn forced_source_discovery_fault() -> Option<io::Error> {
    std::env::var_os("ATM_TEST_FORCE_SOURCE_DISCOVERY_FAULT")
        .map(|_| io::Error::other("synthetic read_dir entry enumeration fault"))
}

pub(crate) fn load_source_files(paths: &[PathBuf]) -> Result<Vec<SourceFile>, AtmError> {
    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        if !path.exists() {
            return Err(AtmError::mailbox_read(format!(
                "mailbox file disappeared before locked read completed: {}",
                path.display()
            )));
        }

        let messages = super::load_compat_mailbox_messages(path)?;
        sources.push(SourceFile {
            path: path.clone(),
            messages,
        });
    }

    Ok(sources)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        discover_origin_inboxes, load_source_files, origin_inbox_enumeration_error,
        rediscover_and_validate_source_paths, resolve_target,
    };
    use crate::config::AtmConfig;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::test_support::{TEST_ORIGIN, TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, TeamName};

    #[test]
    fn discover_origin_inboxes_ignores_primary_and_sorts_matches() {
        let tempdir = tempdir().expect("tempdir");
        let inboxes = tempdir.path();
        std::fs::write(inboxes.join(format!("{TEST_SENDER}.json")), "").expect("primary");
        std::fs::write(
            inboxes.join(format!("{TEST_SENDER}.{}.json", TEST_ORIGIN)),
            "",
        )
        .expect("host a");
        std::fs::write(
            inboxes.join(format!("{TEST_SENDER}.{}-b.json", TEST_ORIGIN)),
            "",
        )
        .expect("host b");
        std::fs::write(inboxes.join("other.json"), "").expect("other");

        let discovered = discover_origin_inboxes(inboxes, TEST_SENDER).expect("discover");
        assert_eq!(
            discovered,
            vec![
                inboxes.join(format!("{TEST_SENDER}.{}-b.json", TEST_ORIGIN)),
                inboxes.join(format!("{TEST_SENDER}.{}.json", TEST_ORIGIN))
            ]
        );
    }

    #[test]
    fn origin_inbox_enumeration_error_is_mailbox_read_failure() {
        let error = origin_inbox_enumeration_error(
            Path::new("test-inbox-dir"),
            TEST_SENDER,
            io::Error::other("synthetic"),
        );

        assert!(error.code() == crate::error_codes::AtmErrorCode::MailboxReadFailed);
        assert!(
            error
                .message()
                .contains("failed to enumerate origin inbox entries")
        );
    }

    #[test]
    fn resolve_target_canonicalizes_alias_before_mailbox_lookup() {
        let mut aliases = BTreeMap::new();
        aliases.insert("tl".to_string(), ROLE_TEAM_LEAD.to_string());
        let config = AtmConfig {
            default_team: Some(TEST_TEAM.parse().expect("team")),
            aliases,
            ..Default::default()
        };

        let target = resolve_target(
            Some(&"tl".parse().expect("address")),
            &TEST_SENDER.parse().expect("agent"),
            &TEST_TEAM.parse().expect("team"),
            Some(&config),
        )
        .expect("target");
        assert_eq!(target.agent, ROLE_TEAM_LEAD);
        assert!(target.explicit);
    }

    #[test]
    fn resolve_target_rejects_invalid_alias_target() {
        let mut aliases = BTreeMap::new();
        aliases.insert("tl".to_string(), "../bad-agent".to_string());
        let config = AtmConfig {
            default_team: Some(TEST_TEAM.parse().expect("team")),
            aliases,
            ..Default::default()
        };

        let error = resolve_target(
            Some(&"tl".parse().expect("address")),
            &TEST_SENDER.parse().expect("agent"),
            &TEST_TEAM.parse().expect("team"),
            Some(&config),
        )
        .expect_err("invalid alias target");

        assert!(error.code() == crate::error_codes::AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn load_source_files_reports_disappearing_mailbox() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join(format!("{TEST_SENDER}.json"));
        std::fs::write(&path, "").expect("mailbox");
        std::fs::remove_file(&path).expect("remove");

        let error = load_source_files(&[path]).expect_err("missing mailbox");
        assert!(error.code() == crate::error_codes::AtmErrorCode::MailboxReadFailed);
        assert!(error.message().contains("disappeared"));
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
        let locked = inboxes.join(format!("{TEST_SENDER}.json"));
        let added = inboxes.join(format!("{TEST_SENDER}.{}.json", TEST_ORIGIN));
        std::fs::write(&locked, "").expect("primary");

        let discovered = super::discover_source_paths(
            home,
            &TEST_TEAM.parse().expect("team"),
            &TEST_SENDER.parse().expect("sender"),
        )
        .expect("discover");
        std::fs::write(&added, "").expect("origin");

        let error = rediscover_and_validate_source_paths(
            &discovered,
            home,
            &TEST_TEAM.parse().expect("team"),
            &TEST_SENDER.parse().expect("sender"),
        )
        .expect_err("drift error");
        assert!(error.code() == crate::error_codes::AtmErrorCode::MailboxLockFailed);
        assert!(error.message().contains("source path set changed"));
    }

    #[test]
    fn discover_source_paths_rejects_invalid_team_segment() {
        let error = "../evil".parse::<TeamName>().expect_err("team");

        assert!(error.code() == crate::error_codes::AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn discover_source_paths_rejects_invalid_agent_segment() {
        let error = "../evil".parse::<AgentName>().expect_err("agent");

        assert!(error.code() == crate::error_codes::AtmErrorCode::AddressParseFailed);
    }
}
