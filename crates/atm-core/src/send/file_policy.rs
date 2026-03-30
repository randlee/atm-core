use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AtmError;

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

    let share_dir = home_dir
        .join(".config")
        .join("atm")
        .join("share")
        .join(team_name);
    fs::create_dir_all(&share_dir)?;

    let file_name = file_path
        .file_name()
        .ok_or_else(|| AtmError::file_policy("file path has no file name"))?;
    let share_copy = share_dir.join(file_name);
    fs::copy(file_path, &share_copy).map_err(|error| {
        AtmError::file_policy(format!("failed to copy file into share directory: {error}"))
            .with_source(error)
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
