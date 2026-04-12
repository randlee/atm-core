use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::error::AtmError;

const HOOK_FILE_TTL_SECS: f64 = 5.0;

#[derive(Debug, Deserialize)]
struct HookFileData {
    agent_name: Option<String>,
    created_at: f64,
}

pub fn read_hook_identity() -> Result<Option<String>, AtmError> {
    let Some(path) = hook_file_path() else {
        return Ok(None);
    };

    if !path.is_file() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path).map_err(|error| {
        AtmError::new(
            crate::error::AtmErrorKind::Identity,
            format!("failed to read hook file {}: {error}", path.display()),
        )
        .with_source(error)
        .with_recovery(
            "The hook identity file is ephemeral. Rerun the triggering hook or ignore if hook identity is optional.",
        )
    })?;

    let data: HookFileData = serde_json::from_str(&raw).map_err(|error| {
        AtmError::new(
            crate::error::AtmErrorKind::Identity,
            format!("invalid hook file JSON at {}: {error}", path.display()),
        )
        .with_source(error)
        .with_recovery(
            "The hook identity file is ephemeral. Rerun the triggering hook or ignore if hook identity is optional.",
        )
    })?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    if (now - data.created_at) > HOOK_FILE_TTL_SECS {
        return Ok(None);
    }

    Ok(data.agent_name.filter(|value| !value.trim().is_empty()))
}

fn hook_file_path() -> Option<std::path::PathBuf> {
    let pid = parent_pid()?;
    Some(std::env::temp_dir().join(format!("atm-hook-{pid}.json")))
}

fn parent_pid() -> Option<u32> {
    #[cfg(unix)]
    {
        let pid = unsafe { libc::getppid() };
        (pid > 0).then_some(pid as u32)
    }

    #[cfg(not(unix))]
    {
        None
    }
}
