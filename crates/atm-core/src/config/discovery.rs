//! Post-send hook config normalization helpers.

use std::path::{Path, PathBuf};

use crate::address::validate_path_segment;
use crate::error::{AtmError, AtmErrorKind};

use super::types::PostSendHookRule;

/// Normalize recipient hook rules relative to the declaring config directory.
///
/// Path-like `command[0]` values resolve from `config_root`. Bare executable
/// names remain unchanged so the OS can resolve them through `PATH`.
pub fn normalize_post_send_hooks(
    hooks: Vec<PostSendHookRule>,
    config_root: &Path,
) -> Result<Vec<PostSendHookRule>, AtmError> {
    hooks.into_iter()
        .map(|mut hook| {
            hook.recipient = hook.recipient.trim().to_string();
            if hook.recipient.is_empty() {
                return Err(AtmError::new(
                    AtmErrorKind::Config,
                    "post-send hook recipient must not be empty".to_string(),
                )
                .with_recovery(
                    "Set [[atm.post_send_hooks]].recipient to one concrete recipient name or '*'.",
                ));
            }
            if hook.recipient != "*" {
                validate_path_segment(&hook.recipient, "hook recipient").map_err(|error| {
                    AtmError::new(AtmErrorKind::Config, error.message).with_recovery(
                        "Use one concrete recipient name or '*' in [[atm.post_send_hooks]].recipient.",
                    )
                })?;
            }

            let Some(program) = hook.command.first_mut() else {
                return Err(AtmError::new(
                    AtmErrorKind::Config,
                    "post-send hook command must not be empty".to_string(),
                )
                .with_recovery(
                    "Set [[atm.post_send_hooks]].command to a non-empty argv array beginning with the executable to run.",
                ));
            };
            *program = program.trim().to_string();
            if program.is_empty() {
                return Err(AtmError::new(
                    AtmErrorKind::Config,
                    "post-send hook command program must not be empty".to_string(),
                )
                .with_recovery(
                    "Set [[atm.post_send_hooks]].command[0] to a relative path, absolute path, or bare executable name.",
                ));
            }
            if command_looks_like_path(program) {
                let resolved = if Path::new(program).is_absolute() {
                    PathBuf::from(&*program)
                } else {
                    config_root.join(&*program)
                };
                *program = resolved
                    .to_str()
                    .ok_or_else(|| {
                        AtmError::new(
                            AtmErrorKind::Config,
                            format!("hook command path is not valid UTF-8: {}", resolved.display()),
                        )
                        .with_recovery(
                            "Use a UTF-8 hook path or invoke the hook through a bare executable name so ATM can resolve it via PATH.",
                        )
                    })?
                    .to_string();
            }
            Ok(hook)
        })
        .collect()
}

pub fn command_looks_like_path(program: &str) -> bool {
    program.contains('/') || program.contains('\\')
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{command_looks_like_path, normalize_post_send_hooks};
    use crate::config::types::PostSendHookRule;

    fn config_root_fixture() -> (tempfile::TempDir, PathBuf) {
        let tempdir = tempdir().expect("tempdir");
        let config_root = tempdir.path().join("atm config root").join("nested");
        std::fs::create_dir_all(&config_root).expect("config root");
        (tempdir, config_root)
    }

    #[test]
    fn normalize_post_send_hooks_resolves_relative_script_commands() {
        let (_tempdir, config_root) = config_root_fixture();
        let hooks = vec![PostSendHookRule {
            recipient: "team-lead".into(),
            command: vec!["scripts/atm-nudge.sh".into(), "team-lead".into()],
        }];

        let hooks = normalize_post_send_hooks(hooks, &config_root).expect("hooks");

        assert_eq!(
            hooks[0].command[0],
            config_root
                .join("scripts/atm-nudge.sh")
                .display()
                .to_string()
        );
    }

    #[test]
    fn normalize_post_send_hooks_keeps_bare_executables_for_path_lookup() {
        let (_tempdir, config_root) = config_root_fixture();
        let hooks = vec![PostSendHookRule {
            recipient: "*".into(),
            command: vec!["bash".into(), "-lc".into(), "echo hi".into()],
        }];

        let hooks = normalize_post_send_hooks(hooks, &config_root).expect("hooks");

        assert_eq!(hooks[0].command[0], "bash");
    }

    #[test]
    fn command_looks_like_path_matches_path_like_programs_only() {
        assert!(command_looks_like_path("scripts/atm-nudge.sh"));
        assert!(command_looks_like_path(r"scripts\atm-nudge.bat"));
        assert!(!command_looks_like_path("python3"));
        assert!(!command_looks_like_path("tmux"));
    }

    #[test]
    fn normalize_post_send_hooks_preserves_absolute_paths() {
        let (_tempdir, config_root) = config_root_fixture();
        let absolute = config_root.join("absolute hook.cmd");
        let hooks = vec![PostSendHookRule {
            recipient: "*".into(),
            command: vec![absolute.display().to_string()],
        }];

        let hooks = normalize_post_send_hooks(hooks, &config_root).expect("hooks");

        assert_eq!(hooks[0].command[0], absolute.display().to_string());
    }

    #[test]
    fn normalize_post_send_hooks_rejects_empty_recipient() {
        let (_tempdir, config_root) = config_root_fixture();
        let error = normalize_post_send_hooks(
            vec![PostSendHookRule {
                recipient: "   ".into(),
                command: vec!["bash".into()],
            }],
            &config_root,
        )
        .expect_err("empty recipient should fail");

        assert!(error.message.contains("recipient must not be empty"));
    }

    #[test]
    fn normalize_post_send_hooks_rejects_invalid_recipient_selector() {
        let (_tempdir, config_root) = config_root_fixture();
        let error = normalize_post_send_hooks(
            vec![PostSendHookRule {
                recipient: "bad/name".into(),
                command: vec!["bash".into()],
            }],
            &config_root,
        )
        .expect_err("invalid recipient should fail");

        assert!(
            error
                .message
                .contains("hook recipient name must not contain path separators")
        );
    }

    #[test]
    fn normalize_post_send_hooks_rejects_empty_command_array() {
        let (_tempdir, config_root) = config_root_fixture();
        let error = normalize_post_send_hooks(
            vec![PostSendHookRule {
                recipient: "team-lead".into(),
                command: Vec::new(),
            }],
            &config_root,
        )
        .expect_err("empty command should fail");

        assert!(error.message.contains("command must not be empty"));
    }

    #[test]
    fn normalize_post_send_hooks_rejects_blank_program_name() {
        let (_tempdir, config_root) = config_root_fixture();
        let error = normalize_post_send_hooks(
            vec![PostSendHookRule {
                recipient: "team-lead".into(),
                command: vec!["   ".into(), "arg".into()],
            }],
            &config_root,
        )
        .expect_err("blank program should fail");

        assert!(error.message.contains("command program must not be empty"));
    }
}
