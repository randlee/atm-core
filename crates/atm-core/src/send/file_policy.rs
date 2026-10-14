use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AtmError, AtmErrorCode};
use crate::types::TeamName;

const MAX_FILE_REFERENCE_BYTES: u64 = 10 * 1024 * 1024;
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
    if paths_refer_to_same_file(file_path, &share_copy).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to compare file identities for {} and {}: {error}",
            file_path.display(),
            share_copy.display()
        ))
    })? {
        return Ok(render_reference_message(message_text, &share_copy));
    }
    let copied_bytes = fs::copy(file_path, &share_copy).map_err(|error| {
        AtmError::file_policy(format!("failed to copy file into share directory: {error}"))
    })?;
    if copied_bytes != file_size.len() {
        let _ = fs::remove_file(&share_copy);
        return Err(AtmError::file_policy(format!(
            "file reference copy was incomplete (copied {copied_bytes} of {} bytes): {}",
            file_size.len(),
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

#[cfg(unix)]
fn paths_refer_to_same_file(source: &Path, destination: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let source = fs::metadata(source)?;
    let destination = match fs::metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(source.dev() == destination.dev() && source.ino() == destination.ino())
}

#[cfg(windows)]
fn paths_refer_to_same_file(source: &Path, destination: &Path) -> std::io::Result<bool> {
    let source = windows_file_identity(source)?;
    let destination = match windows_file_identity(destination) {
        Ok(identity) => identity,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(source == destination)
}

#[cfg(windows)]
fn windows_file_identity(path: &Path) -> std::io::Result<(u32, u64)> {
    use std::fs::File;
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = File::open(path)?;
    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    // SAFETY: `file` owns a valid Windows file handle and `information` points to
    // writable storage of the exact structure required by the API call.
    let result =
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a nonzero result guarantees that the API initialized `information`.
    let information = unsafe { information.assume_init() };
    let file_index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Ok((information.dwVolumeSerialNumber, file_index))
}

#[cfg(not(any(unix, windows)))]
fn paths_refer_to_same_file(source: &Path, destination: &Path) -> std::io::Result<bool> {
    match (source.canonicalize(), destination.canonicalize()) {
        (Ok(source), Ok(destination)) => Ok(source == destination),
        (_, Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
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
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use super::{MAX_FILE_REFERENCE_BYTES, process_file_reference};
    use crate::test_support::TEST_TEAM;

    fn share_dir(home_dir: &Path) -> PathBuf {
        home_dir
            .join(".config")
            .join("atm")
            .join("share")
            .join(TEST_TEAM)
    }

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
    fn share_dir_source_is_not_truncated_by_file_reference_copy() {
        let current_dir = tempdir().expect("current tempdir");
        let home_dir = tempdir().expect("home tempdir");
        let share_dir = share_dir(home_dir.path());
        fs::create_dir_all(&share_dir).expect("share directory");
        let source_path = share_dir.join("payload.txt");
        let original = b"source content must survive";
        fs::write(&source_path, original).expect("source file");

        let reference = process_file_reference(
            &source_path,
            None,
            &TEST_TEAM.parse().expect("team"),
            current_dir.path(),
            home_dir.path(),
        )
        .expect("share-dir source should be accepted");

        assert_eq!(source_path.parent(), Some(share_dir.as_path()));
        assert!(reference.contains("payload.txt"));
        assert_eq!(
            fs::read(&source_path).expect("source remains readable"),
            original
        );
    }

    #[test]
    fn hard_link_source_is_not_truncated_by_file_reference_copy() {
        let current_dir = tempdir().expect("current tempdir");
        let home_dir = tempdir().expect("home tempdir");
        let source_dir = tempdir().expect("source tempdir");
        let share_dir = share_dir(home_dir.path());
        fs::create_dir_all(&share_dir).expect("share directory");
        let share_copy = share_dir.join("payload.txt");
        let source_path = source_dir.path().join("payload.txt");
        let original = b"hard-linked source content must survive";
        fs::write(&share_copy, original).expect("share file");
        fs::hard_link(&share_copy, &source_path).expect("hard link");

        let reference = process_file_reference(
            &source_path,
            None,
            &TEST_TEAM.parse().expect("team"),
            current_dir.path(),
            home_dir.path(),
        )
        .expect("hard-linked source should be accepted");

        assert!(reference.contains("payload.txt"));
        assert_eq!(
            fs::read(&source_path).expect("source remains readable"),
            original
        );
        assert_eq!(
            fs::read(&share_copy).expect("share copy remains readable"),
            original
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_source_is_not_truncated_by_file_reference_copy() {
        use std::os::unix::fs::symlink;

        let current_dir = tempdir().expect("current tempdir");
        let home_dir = tempdir().expect("home tempdir");
        let source_dir = tempdir().expect("source tempdir");
        let share_dir = share_dir(home_dir.path());
        fs::create_dir_all(&share_dir).expect("share directory");
        let share_copy = share_dir.join("payload.txt");
        let source_path = source_dir.path().join("payload.txt");
        let original = b"symlinked source content must survive";
        fs::write(&share_copy, original).expect("share file");
        symlink(&share_copy, &source_path).expect("symlink");

        let reference = process_file_reference(
            &source_path,
            None,
            &TEST_TEAM.parse().expect("team"),
            current_dir.path(),
            home_dir.path(),
        )
        .expect("symlinked source should be accepted");

        assert!(reference.contains("payload.txt"));
        assert_eq!(
            fs::read(&source_path).expect("source remains readable"),
            original
        );
        assert_eq!(
            fs::read(&share_copy).expect("share copy remains readable"),
            original
        );
    }
}
