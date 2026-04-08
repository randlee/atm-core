use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AtmError, AtmErrorKind};
use crate::persistence;
use crate::types::IsoTimestamp;

/// Load the last-seen watermark for one agent inbox.
///
/// # Errors
///
/// Returns [`AtmError`] when the watermark file exists but cannot be read or
/// parsed as RFC3339.
pub fn load_seen_watermark(
    home_dir: &Path,
    team: &str,
    agent: &str,
) -> Result<Option<IsoTimestamp>, AtmError> {
    let path = seen_state_path(home_dir, team, agent);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxRead,
            format!("failed to read seen-state watermark: {error}"),
        )
        .with_recovery("Check seen-state file permissions or remove the malformed watermark file before rerunning the read command.")
        .with_source(error)
    })?;

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let parsed = chrono::DateTime::parse_from_rfc3339(trimmed).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Serialization,
            format!("invalid seen-state watermark: {error}"),
        )
        .with_recovery("Remove the malformed seen-state watermark file so ATM can rebuild it on the next successful read.")
        .with_source(error)
    })?;

    Ok(Some(parsed.with_timezone(&chrono::Utc).into()))
}

/// Persist the last-seen watermark for one agent inbox.
///
/// # Errors
///
/// Returns [`AtmError`] when the seen-state directory cannot be created or the
/// watermark cannot be atomically replaced.
pub fn save_seen_watermark(
    home_dir: &Path,
    team: &str,
    agent: &str,
    timestamp: IsoTimestamp,
) -> Result<(), AtmError> {
    let path = seen_state_path(home_dir, team, agent);
    persistence::atomic_write_string(
        &path,
        &timestamp.into_inner().to_rfc3339(),
        AtmErrorKind::MailboxWrite,
        "seen-state watermark",
        "Check seen-state directory permissions and rerun the read command.",
    )
}

fn seen_state_path(home_dir: &Path, team: &str, agent: &str) -> PathBuf {
    home_dir
        .join(".claude")
        .join("teams")
        .join(team)
        .join(".seen")
        .join(agent)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use tempfile::TempDir;

    use super::{load_seen_watermark, save_seen_watermark};
    use crate::types::IsoTimestamp;

    #[test]
    fn load_missing_seen_state_returns_none() {
        let tempdir = TempDir::new().expect("tempdir");
        let loaded = load_seen_watermark(tempdir.path(), "atm-dev", "arch-ctm").expect("load");
        assert!(loaded.is_none());
    }

    #[test]
    fn save_and_load_seen_state_round_trips() {
        let tempdir = TempDir::new().expect("tempdir");
        let timestamp = IsoTimestamp::from_datetime(
            chrono::Utc
                .with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                .single()
                .expect("timestamp"),
        );

        save_seen_watermark(tempdir.path(), "atm-dev", "arch-ctm", timestamp).expect("save");
        let loaded = load_seen_watermark(tempdir.path(), "atm-dev", "arch-ctm").expect("load");

        assert_eq!(loaded, Some(timestamp));
    }
}
