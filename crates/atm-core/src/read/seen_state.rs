use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{AtmError, AtmErrorKind};
use crate::types::IsoTimestamp;

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
        .with_source(error)
    })?;

    Ok(Some(parsed.with_timezone(&chrono::Utc).into()))
}

pub fn save_seen_watermark(
    home_dir: &Path,
    team: &str,
    agent: &str,
    timestamp: IsoTimestamp,
) -> Result<(), AtmError> {
    let path = seen_state_path(home_dir, team, agent);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxWrite,
                format!("failed to create seen-state directory: {error}"),
            )
            .with_source(error)
        })?;
    }

    let temp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp_path).map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxWrite,
                format!("failed to create seen-state temp file: {error}"),
            )
            .with_source(error)
        })?;
        file.write_all(timestamp.into_inner().to_rfc3339().as_bytes())
            .map_err(|error| {
                AtmError::new(
                    AtmErrorKind::MailboxWrite,
                    format!("failed to write seen-state watermark: {error}"),
                )
                .with_source(error)
            })?;
        file.sync_all().map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxWrite,
                format!("failed to sync seen-state watermark: {error}"),
            )
            .with_source(error)
        })?;
    }

    fs::rename(&temp_path, &path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxWrite,
            format!("failed to replace seen-state watermark: {error}"),
        )
        .with_source(error)
    })?;
    Ok(())
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
