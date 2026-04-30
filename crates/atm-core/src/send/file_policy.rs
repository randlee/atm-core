use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AtmError, AtmErrorKind};

const MAX_FILE_REFERENCE_BYTES: u64 = 10 * 1024 * 1024;

/// Process a send `--file` reference under the ATM file-policy rules.
///
/// # Errors
///
/// Returns [`AtmError`] when the source file is missing, the team share
/// directory cannot be created, the source path has no terminal file name, or
/// copying the file into the share directory fails.
pub fn process_file_reference(
    file_path: &Path,
    message_text: Option<&str>,
    team_name: &str,
    current_dir: &Path,
    home_dir: &Path,
) -> Result<String, AtmError> {
    if !file_path.is_file() {
        return Err(AtmError::file_policy(format!(
            "file not found: {}",
            file_path.display()
        )));
    }

    if is_file_in_repo(file_path, current_dir) {
        return Ok(render_reference_message(message_text, file_path));
    }

    let file_size = fs::metadata(file_path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::FilePolicy,
            format!("failed to inspect file {}: {error}", file_path.display()),
        )
        .with_source(error)
        .with_recovery(
            "Check that the source file still exists and is readable, then retry the send.",
        )
    })?;
    if file_size.len() > MAX_FILE_REFERENCE_BYTES {
        return Err(AtmError::file_policy(format!(
            "file reference exceeds the {}-byte limit: {}",
            MAX_FILE_REFERENCE_BYTES,
            file_path.display()
        ))
        .with_recovery(
            "Use a file no larger than 10 MiB or move the content into the repository so ATM can reference it without copying.",
        ));
    }

    let share_dir = home_dir
        .join(".config")
        .join("atm")
        .join("share")
        .join(team_name);
    fs::create_dir_all(&share_dir).map_err(|error| {
        AtmError::new(
            AtmErrorKind::FilePolicy,
            format!(
                "failed to create share directory {}: {error}",
                share_dir.display()
            ),
        )
        .with_source(error)
        .with_recovery(
            "Check the ATM share directory permissions for the target team and retry the send.",
        )
    })?;

    let file_name = file_path.file_name().ok_or_else(|| {
        AtmError::file_policy("file path has no file name").with_recovery(
            "Pass a concrete file path with a terminal file name or retry with inline message text.",
        )
    })?;
    let share_copy = share_dir.join(file_name);
    fs::copy(file_path, &share_copy).map_err(|error| {
        AtmError::file_policy(format!("failed to copy file into share directory: {error}"))
            .with_source(error)
            .with_recovery(
                "Check source/share permissions and available disk space, then retry the send.",
            )
    })?;

    Ok(render_reference_message(message_text, &share_copy))
}

fn render_reference_message(message_text: Option<&str>, file_path: &Path) -> String {
    match message_text.filter(|message| !message.trim().is_empty()) {
        Some(message_text) => {
            format!("{message_text}\n\nFile reference: {}", file_path.display())
        }
        None => format!("File reference: {}", file_path.display()),
    }
}

fn is_file_in_repo(file_path: &Path, current_dir: &Path) -> bool {
    match (canonical(file_path), find_git_root(current_dir)) {
        (Some(file_path), Some(repo_root)) => file_path.starts_with(repo_root),
        _ => false,
    }
}

fn canonical(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok()
}

fn find_git_root(start_dir: &Path) -> Option<PathBuf> {
    let mut current = Some(start_dir);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return canonical(dir);
        }
        current = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};

    use tempfile::tempdir;

    use super::{MAX_FILE_REFERENCE_BYTES, process_file_reference};

    #[test]
    fn rejects_oversized_non_repo_file_references_before_copying() {
        let source_dir = tempdir().expect("source tempdir");
        let current_dir = tempdir().expect("current tempdir");
        let home_dir = tempdir().expect("home tempdir");
        let oversized_path = source_dir.path().join("large.bin");
        File::create(&oversized_path)
            .and_then(|file| file.set_len(MAX_FILE_REFERENCE_BYTES + 1))
            .expect("oversized file");

        let error = process_file_reference(
            &oversized_path,
            Some("see attached"),
            "atm-dev",
            current_dir.path(),
            home_dir.path(),
        )
        .expect_err("oversized file should fail");

        assert!(error.is_file_policy());
        assert!(error.message.contains("exceeds"));
        assert!(
            error
                .recovery
                .as_deref()
                .is_some_and(|value| value.contains("10 MiB"))
        );
        assert!(
            fs::read_dir(
                home_dir
                    .path()
                    .join(".config")
                    .join("atm")
                    .join("share")
                    .join("atm-dev")
            )
            .is_err()
        );
    }
}
