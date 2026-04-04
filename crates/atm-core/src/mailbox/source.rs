use std::fs;
use std::path::{Path, PathBuf};

use tracing::warn;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorKind};
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

    Ok(ResolvedTarget {
        agent: parsed.agent,
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::discover_origin_inboxes;

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
}
