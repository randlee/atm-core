use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{AtmError, AtmErrorCode};
use crate::types::TeamName;

const MAX_FILE_REFERENCE_BYTES: u64 = 10 * 1024 * 1024;
const TASK_ENVELOPE_INSPECTION_BYTES: u64 = 8 * 1024;

/// Return whether a referenced file is an ATM task envelope.
///
/// This is deliberately a narrow recognition rule: only an XML document whose
/// root tag is `atm-task` changes durable acknowledgement semantics. Other
/// files remain ordinary references, including files that merely mention the
/// string in prose.
pub fn is_task_envelope(file_path: &Path) -> bool {
    let Ok(file) = fs::File::open(file_path) else {
        return false;
    };
    let mut prefix = Vec::with_capacity(TASK_ENVELOPE_INSPECTION_BYTES as usize);
    let Ok(_) = file
        .take(TASK_ENVELOPE_INSPECTION_BYTES)
        .read_to_end(&mut prefix)
    else {
        return false;
    };
    std::str::from_utf8(&prefix).is_ok_and(is_task_envelope_body)
}

fn is_task_envelope_body(body: &str) -> bool {
    let body = body.trim_start_matches('\u{feff}').trim_start();
    let Some(after_name) = body.strip_prefix("<atm-task") else {
        return false;
    };
    after_name
        .chars()
        .next()
        .is_some_and(|character| character == '>' || character == '/' || character.is_whitespace())
}

/// Process a send `--file` reference under the ATM file-policy rules.
///
/// # Errors
///
/// Returns [`AtmError`] with [`crate::error_codes::AtmErrorCode::FilePolicyRejected`]
/// when the source file is missing, metadata inspection fails, the source file
/// exceeds the 10 MiB copy limit, the team share directory cannot be created,
/// the source path has no terminal file name, or copying the file into the
/// share directory fails.
pub fn process_file_reference(
    file_path: &Path,
    message_text: Option<&str>,
    team_name: &TeamName,
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
            AtmErrorCode::FilePolicyRejected,
            format!("failed to inspect file {}: {error}", file_path.display()),
        )
    })?;
    if file_size.len() > MAX_FILE_REFERENCE_BYTES {
        return Err(AtmError::file_policy(format!(
            "file reference exceeds the {}-byte limit: {}",
            MAX_FILE_REFERENCE_BYTES,
            file_path.display()
        )));
    }

    let share_dir = home_dir
        .join(".config")
        .join("atm")
        .join("share")
        .join(team_name.as_str());
    fs::create_dir_all(&share_dir).map_err(|error| {
        AtmError::new(
            AtmErrorCode::FilePolicyRejected,
            format!(
                "failed to create share directory {}: {error}",
                share_dir.display()
            ),
        )
    })?;

    let file_name = file_path
        .file_name()
        .ok_or_else(|| AtmError::file_policy("file path has no file name"))?;
    let share_copy = share_dir.join(file_name);
    let copied_bytes = fs::copy(file_path, &share_copy).map_err(|error| {
        AtmError::file_policy(format!("failed to copy file into share directory: {error}"))
    })?;
    if copied_bytes > MAX_FILE_REFERENCE_BYTES {
        let _ = fs::remove_file(&share_copy);
        return Err(AtmError::file_policy(format!(
            "file reference exceeds the {}-byte limit after copy: {}",
            MAX_FILE_REFERENCE_BYTES,
            file_path.display()
        )));
    }

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

    use super::{MAX_FILE_REFERENCE_BYTES, is_task_envelope, process_file_reference};
    use crate::test_support::TEST_TEAM;

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
            &TEST_TEAM.parse().expect("team"),
            current_dir.path(),
            home_dir.path(),
        )
        .expect_err("oversized file should fail");

        assert!(error.code() == crate::error_codes::AtmErrorCode::FilePolicyRejected);
        assert!(error.message().contains("exceeds"));
        assert!(error.message().contains("Recovery:"));
        assert!(
            fs::read_dir(
                home_dir
                    .path()
                    .join(".config")
                    .join("atm")
                    .join("share")
                    .join(TEST_TEAM)
            )
            .is_err()
        );
    }

    #[test]
    fn recognizes_only_an_atm_task_root_element() {
        let directory = tempdir().expect("tempdir");
        let task = directory.path().join("task.xml");
        let prose = directory.path().join("prose.txt");
        let similarly_named = directory.path().join("similarly-named.xml");
        fs::write(&task, "\u{feff}\n <atm-task id=\"TASK-1\"></atm-task>").expect("task fixture");
        fs::write(&prose, "please review <atm-task id=\"TASK-1\">").expect("prose fixture");
        fs::write(&similarly_named, "<atm-task-note />").expect("similarly named fixture");

        assert!(is_task_envelope(&task));
        assert!(!is_task_envelope(&prose));
        assert!(!is_task_envelope(&similarly_named));
    }
}
