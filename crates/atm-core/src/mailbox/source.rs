use std::fs;
use std::path::{Path, PathBuf};

use tracing::warn;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorKind};
use crate::home;
use crate::schema::MessageEnvelope;

#[derive(Debug, Clone)]
pub(crate) struct SourceFile {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
}

#[derive(Debug, Clone)]
pub(crate) struct SourcedMessage {
    pub envelope: MessageEnvelope,
    pub source_path: PathBuf,
    pub source_index: usize,
}

#[derive(Debug)]
pub(crate) struct ResolvedTarget {
    pub agent: String,
    pub team: String,
    pub explicit: bool,
}

pub(crate) fn resolve_target(
    target_address: Option<&str>,
    actor: &str,
    team_override: Option<&str>,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedTarget, AtmError> {
    let Some(target_address) = target_address else {
        let team =
            config::resolve_team(team_override, config).ok_or_else(AtmError::team_unavailable)?;
        return Ok(ResolvedTarget {
            agent: actor.to_string(),
            team,
            explicit: false,
        });
    };

    let parsed: AgentAddress = target_address.parse()?;
    let team = parsed
        .team
        .or_else(|| config::resolve_team(team_override, config))
        .ok_or_else(AtmError::team_unavailable)?;
    let agent = config::aliases::resolve_agent(&parsed.agent, config);

    Ok(ResolvedTarget {
        agent,
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
    let mut paths = fs::read_dir(inboxes_dir)
        .map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxRead,
                format!(
                    "failed to read inbox directory {}: {error}",
                    inboxes_dir.display()
                ),
            )
            .with_source(error)
        })?
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry.path()),
            Err(error) => {
                warn!(
                    inbox_dir = %inboxes_dir.display(),
                    agent,
                    %error,
                    "skipping unreadable origin inbox entry"
                );
                None
            }
        })
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(|name| name.starts_with(&prefix) && name.ends_with(".json") && name != primary)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    paths.sort();
    Ok(paths)
}

pub(crate) fn discover_source_paths(
    home_dir: &Path,
    team: &str,
    agent: &str,
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
    paths.extend(discover_origin_inboxes(&inboxes_dir, agent)?);
    paths.sort_by_key(|path| path.to_string_lossy().into_owned());
    paths.dedup();
    Ok(paths)
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
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::{discover_origin_inboxes, resolve_target};
    use crate::config::AtmConfig;

    #[test]
    fn discover_origin_inboxes_ignores_primary_and_sorts_matches() {
        let tempdir = tempdir().expect("tempdir");
        let inboxes = tempdir.path();
        std::fs::write(inboxes.join("arch-ctm.json"), "").expect("primary");
        std::fs::write(inboxes.join("arch-ctm.host-b.json"), "").expect("host b");
        std::fs::write(inboxes.join("arch-ctm.host-a.json"), "").expect("host a");
        std::fs::write(inboxes.join("other.json"), "").expect("other");

        let discovered = discover_origin_inboxes(inboxes, "arch-ctm").expect("discover");
        assert_eq!(
            discovered,
            vec![
                inboxes.join("arch-ctm.host-a.json"),
                inboxes.join("arch-ctm.host-b.json")
            ]
        );
    }

    #[test]
    fn resolve_target_canonicalizes_alias_before_mailbox_lookup() {
        let mut aliases = BTreeMap::new();
        aliases.insert("tl".to_string(), "team-lead".to_string());
        let config = AtmConfig {
            default_team: Some("atm-dev".to_string()),
            aliases,
            ..Default::default()
        };

        let target = resolve_target(Some("tl"), "arch-ctm", None, Some(&config)).expect("target");
        assert_eq!(target.agent, "team-lead");
        assert_eq!(target.team, "atm-dev");
        assert!(target.explicit);
    }
}
