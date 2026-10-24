use std::path::{Path, PathBuf};

use atm_core::error::AtmError;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

/// Read the module's declared timeout without importing or executing it.
pub fn read_timeout_ms(module: &Path) -> Result<u32, AtmError> {
    read_constant(module, "TIMEOUT_MS")?
        .ok_or_else(|| invalid(module, "expected exactly one TIMEOUT_MS assignment"))
}

/// Read the cron timeout, falling back only when its declaration is absent.
pub fn read_cron_timeout_ms(module: &Path) -> Result<u32, AtmError> {
    match read_constant(module, "CRON_TIMEOUT_MS")? {
        Some(timeout) => Ok(timeout),
        None => read_timeout_ms(module),
    }
}

fn invalid(module: &Path, rule: &str) -> AtmError {
    AtmError::hook_module_invalid(&format!("{}: {rule}", module.display()))
}

/// Canonicalize a hook path while keeping it consumable by the external hook
/// interpreter.
///
/// Windows' `std::fs::canonicalize` returns a verbatim `\\?\\` path. That
/// spelling is appropriate for Win32 filesystem calls, while the persisted
/// hook target is later passed to an external child process. The path is
/// already resolved, so retain its identity but use the portable drive or UNC
/// spelling before it crosses the CLI-to-daemon boundary.
fn canonicalize_hook_path(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = path.canonicalize()?;
    #[cfg(windows)]
    {
        let Some(value) = canonical.to_str() else {
            return Ok(canonical);
        };
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{unc}")));
        }
        if let Some(portable) = value.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(portable));
        }
    }
    Ok(canonical)
}

fn read_constant(module: &Path, name: &str) -> Result<Option<u32>, AtmError> {
    let source = std::fs::read_to_string(module)
        .map_err(|error| invalid(module, &format!("cannot read module: {error}")))?;
    let mut result = None;
    for line in source.lines() {
        let Some(tail) = line.strip_prefix(name) else {
            continue;
        };
        let tail = tail.trim_start_matches([' ', '\t']);
        let tail = tail
            .strip_prefix(": int")
            .unwrap_or(tail)
            .trim_start_matches([' ', '\t']);
        let Some(value) = tail.strip_prefix('=') else {
            continue;
        };
        let value = value.split('#').next().unwrap_or_default().trim();
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid(
                module,
                &format!("{name} must be positive decimal digits"),
            ));
        }
        let value = value
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| invalid(module, &format!("{name} must be a positive u32")))?;
        if result.replace(value).is_some() {
            return Err(invalid(
                module,
                &format!("expected exactly one {name} assignment"),
            ));
        }
    }
    Ok(result)
}

/// Parse the clap custom value as a JSON object, never a scalar or array.
pub fn parse_custom_object(raw: &str) -> Result<Map<String, Value>, String> {
    serde_json::from_str::<Map<String, Value>>(raw)
        .map_err(|error| format!("--custom must be a JSON object: {error}"))
}

/// Normalize a timezone-aware RFC 3339 stop instant to UTC.
pub fn parse_rfc3339_utc(raw: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(raw)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|error| format!("--until must be RFC 3339: {error}"))
}

pub(super) fn canonical_paths(
    module: &Path,
    cwd: Option<&Path>,
) -> Result<(PathBuf, PathBuf), AtmError> {
    let canonical_module = canonicalize_hook_path(module)
        .map_err(|error| invalid(module, &format!("module does not exist: {error}")))?;
    let cwd = cwd.unwrap_or_else(|| {
        canonical_module
            .parent()
            .expect("canonical module has a parent")
    });
    let canonical_cwd = canonicalize_hook_path(cwd).map_err(|error| {
        invalid(
            module,
            &format!("cwd {} does not exist: {error}", cwd.display()),
        )
    })?;
    if !canonical_module.is_file() || !canonical_cwd.is_dir() {
        return Err(invalid(
            module,
            "module must be a file and cwd must be a directory",
        ));
    }
    Ok((canonical_module, canonical_cwd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_ms_grammar() {
        let directory = tempfile::tempdir().unwrap();
        let module = directory.path().join("hook.py");
        for source in ["TIMEOUT_MS = 2500", "TIMEOUT_MS: int = 2500  # ms"] {
            std::fs::write(&module, source).unwrap();
            assert_eq!(read_timeout_ms(&module).unwrap(), 2500);
        }
        for source in [
            "TIMEOUT_MSX = 1",
            " TIMEOUT_MS = 1",
            "TIMEOUT_MS = 0",
            "TIMEOUT_MS = 1\nTIMEOUT_MS = 2",
            "TIMEOUT_MS = -1",
            "TIMEOUT_MS = 1.5",
        ] {
            std::fs::write(&module, source).unwrap();
            assert!(read_timeout_ms(&module).is_err(), "{source}");
        }
    }

    #[test]
    fn cron_timeout_uses_the_same_grammar_and_only_falls_back_when_absent() {
        let directory = tempfile::tempdir().unwrap();
        let module = directory.path().join("hook.py");
        for (source, expected) in [
            ("TIMEOUT_MS = 2500", 2500),
            (
                "TIMEOUT_MS = 2500\nCRON_TIMEOUT_MS: int = 20000 # tick",
                20000,
            ),
            ("TIMEOUT_MS = 2500\nCRON_TIMEOUT_MS = 30001", 30001),
        ] {
            std::fs::write(&module, source).unwrap();
            assert_eq!(read_cron_timeout_ms(&module).unwrap(), expected);
        }
        for source in [
            "TIMEOUT_MS = 2500\nCRON_TIMEOUT_MS = 0",
            "TIMEOUT_MS = 2500\nCRON_TIMEOUT_MS = nope",
            "TIMEOUT_MS = 2500\nCRON_TIMEOUT_MS = 100\nCRON_TIMEOUT_MS = 200",
        ] {
            std::fs::write(&module, source).unwrap();
            assert!(read_cron_timeout_ms(&module).is_err(), "{source}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn canonical_hook_paths_do_not_persist_windows_verbatim_prefixes() {
        let directory = tempfile::tempdir().unwrap();
        let module = directory.path().join("hook.py");
        std::fs::write(&module, "TIMEOUT_MS = 2500").unwrap();

        let (canonical_module, canonical_cwd) = canonical_paths(&module, None).unwrap();

        assert!(!canonical_module.to_string_lossy().starts_with(r"\\?\"));
        assert!(!canonical_cwd.to_string_lossy().starts_with(r"\\?\"));
    }
}
