//! Post-send hook config normalization helpers.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::address::validate_path_segment;
use crate::error::{AtmError, AtmErrorCode};
use crate::home;
use crate::types::AgentName;

use super::RawPostSendHookRule;
use super::types::{HookRecipient, MAX_POST_SEND_HOOK_COMMAND_PATH_BYTES, PostSendHookRule};

/// Normalize recipient hook rules relative to the declaring config directory.
///
/// Path-like `command[0]` values resolve from `config_root`. Bare executable
/// names remain unchanged so the OS can resolve them through `PATH`.
pub(super) fn normalize_post_send_hooks(
    hooks: Vec<RawPostSendHookRule>,
    config_root: &Path,
) -> Result<Vec<PostSendHookRule>, AtmError> {
    hooks
        .into_iter()
        .map(|mut hook| {
            let recipient = normalize_hook_recipient(&hook.recipient)?;
            normalize_hook_program(&mut hook.command, config_root)?;
            Ok(PostSendHookRule {
                recipient,
                command: hook.command,
            })
        })
        .collect()
}

fn normalize_hook_recipient(recipient: &str) -> Result<HookRecipient, AtmError> {
    let recipient = recipient.trim();
    if recipient.is_empty() {
        return Err(AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            "post-send hook recipient must not be empty".to_string(),
        ));
    }
    if recipient == "*" {
        return Ok(HookRecipient::Wildcard);
    }
    validate_path_segment(recipient, "hook recipient")
        .map_err(|error| AtmError::new(AtmErrorCode::ConfigParseFailed, error.detail()))?;
    Ok(HookRecipient::Named(AgentName::from_validated(recipient)))
}

fn normalize_hook_program(command: &mut [String], config_root: &Path) -> Result<(), AtmError> {
    let Some(program) = command.first_mut() else {
        return Err(AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            "post-send hook command must not be empty".to_string(),
        ));
    };
    *program = program.trim().to_string();
    if program.is_empty() {
        return Err(AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            "post-send hook command program must not be empty".to_string(),
        ));
    }
    let normalized = resolve_hook_program(program, config_root)?;
    *program = normalized;
    Ok(())
}

fn resolve_hook_program(program: &str, config_root: &Path) -> Result<String, AtmError> {
    let has_home_tilde_prefix =
        matches!(program, "~") || program.starts_with("~/") || program.starts_with("~\\");
    let expanded_program = expand_home_tilde(program)?;
    if command_looks_like_path(expanded_program.as_ref()) || has_home_tilde_prefix {
        let resolved = resolve_hook_path(expanded_program.as_ref(), config_root)?;
        let resolved = hook_path_to_utf8(&resolved)?;
        validate_hook_command_path_length(resolved)?;
        return Ok(resolved.to_string());
    }
    validate_hook_command_path_length(expanded_program.as_ref())?;
    Ok(expanded_program.into_owned())
}

fn resolve_hook_path(program: &str, config_root: &Path) -> Result<PathBuf, AtmError> {
    if Path::new(program).is_absolute() {
        return Ok(PathBuf::from(program));
    }
    if let Some(expanded_home) = expand_tilde_to_home_path(program)? {
        return Ok(expanded_home);
    }
    Ok(config_root.join(program))
}

fn hook_path_to_utf8(path: &Path) -> Result<&str, AtmError> {
    path.to_str().ok_or_else(|| {
        AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            format!("hook command path is not valid UTF-8: {}", path.display()),
        )
    })
}

pub(crate) fn command_looks_like_path(program: &str) -> bool {
    program.contains('/') || program.contains('\\')
}

fn expand_home_tilde(program: &str) -> Result<Cow<'_, str>, AtmError> {
    if !program.starts_with('~') {
        return Ok(Cow::Borrowed(program));
    }
    // Issue #219: tilde-expansion for post-send hook command[0] paths.
    if matches!(program, "~") || program.starts_with("~/") || program.starts_with("~\\") {
        let expanded = expand_tilde_to_home_path(program)?;
        let Some(expanded) = expanded else {
            return Ok(Cow::Borrowed(program));
        };
        return Ok(Cow::Owned(
            expanded
                .to_str()
                .ok_or_else(|| {
                    AtmError::new(
                        AtmErrorCode::ConfigParseFailed,
                        format!(
                            "hook command path is not valid UTF-8: {}",
                            expanded.display()
                        ),
                    )
                })?
                .to_string(),
        ));
    }
    Ok(Cow::Borrowed(program))
}

fn expand_tilde_to_home_path(program: &str) -> Result<Option<PathBuf>, AtmError> {
    let rest = match program {
        "~" => Some(""),
        _ => program
            .strip_prefix("~/")
            .or_else(|| program.strip_prefix("~\\")),
    };
    let Some(rest) = rest else {
        return Ok(None);
    };

    let mut expanded = home::user_home()?;
    for segment in rest
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
    {
        expanded.push(segment);
    }
    Ok(Some(expanded))
}

fn validate_hook_command_path_length(path: &str) -> Result<(), AtmError> {
    if path.len() > MAX_POST_SEND_HOOK_COMMAND_PATH_BYTES {
        return Err(AtmError::new(
            AtmErrorCode::ConfigParseFailed,
            format!(
                "post-send hook command path exceeds the maximum supported length of {} bytes",
                MAX_POST_SEND_HOOK_COMMAND_PATH_BYTES
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{command_looks_like_path, normalize_post_send_hooks};
    use crate::config::RawPostSendHookRule;
    use crate::config::types::HookRecipient;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::test_support::EnvGuard;

    fn config_root_fixture() -> (tempfile::TempDir, PathBuf) {
        let tempdir = tempdir().expect("tempdir");
        let config_root = tempdir.path().join("atm config root").join("nested");
        std::fs::create_dir_all(&config_root).expect("config root");
        (tempdir, config_root)
    }

    fn home_fixture() -> (tempfile::TempDir, EnvGuard) {
        let tempdir = tempdir().expect("tempdir");
        let home = tempdir.path().join("home");
        std::fs::create_dir_all(&home).expect("home dir");
        let home_utf8 = home.to_str().expect("utf8 home path");
        let env = EnvGuard::set_many([("HOME", Some(home_utf8)), ("USERPROFILE", Some(home_utf8))]);
        (tempdir, env)
    }

    #[test]
    fn normalize_post_send_hooks_resolves_relative_script_commands() {
        let (_tempdir, config_root) = config_root_fixture();
        let hooks = vec![RawPostSendHookRule {
            recipient: ROLE_TEAM_LEAD.into(),
            command: vec!["scripts/atm-nudge.sh".into(), ROLE_TEAM_LEAD.into()],
        }];

        let hooks = normalize_post_send_hooks(hooks, &config_root).expect("hooks");

        assert_eq!(
            hooks[0].command[0],
            config_root
                .join("scripts/atm-nudge.sh")
                .display()
                .to_string()
        );
        assert_eq!(
            hooks[0].recipient,
            HookRecipient::Named(ROLE_TEAM_LEAD.parse().expect("recipient"))
        );
    }

    #[test]
    fn normalize_post_send_hooks_keeps_bare_executables_for_path_lookup() {
        let (_tempdir, config_root) = config_root_fixture();
        let hooks = vec![RawPostSendHookRule {
            recipient: "*".into(),
            command: vec!["bash".into(), "-lc".into(), "echo hi".into()],
        }];

        let hooks = normalize_post_send_hooks(hooks, &config_root).expect("hooks");

        assert_eq!(hooks[0].command[0], "bash");
        assert_eq!(hooks[0].recipient, HookRecipient::Wildcard);
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
        let hooks = vec![RawPostSendHookRule {
            recipient: "*".into(),
            command: vec![absolute.display().to_string()],
        }];

        let hooks = normalize_post_send_hooks(hooks, &config_root).expect("hooks");

        assert_eq!(hooks[0].command[0], absolute.display().to_string());
    }

    #[test]
    #[serial_test::serial(env)]
    fn normalize_post_send_hooks_expands_home_tilde_path() {
        let (_tempdir, config_root) = config_root_fixture();
        let (home_root, _env) = home_fixture();
        let home = home_root.path().join("home");
        let hooks = vec![RawPostSendHookRule {
            recipient: "*".into(),
            command: vec!["~/hooks/notify.sh".into()],
        }];

        let hooks = normalize_post_send_hooks(hooks, &config_root).expect("hooks");

        assert_eq!(
            hooks[0].command[0],
            home.join("hooks").join("notify.sh").display().to_string()
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn normalize_post_send_hooks_expands_windows_style_home_tilde_path() {
        let (_tempdir, config_root) = config_root_fixture();
        let (home_root, _env) = home_fixture();
        let home = home_root.path().join("home");
        let hooks = vec![RawPostSendHookRule {
            recipient: "*".into(),
            command: vec![r"~\hooks\notify.cmd".into()],
        }];

        let hooks = normalize_post_send_hooks(hooks, &config_root).expect("hooks");

        assert_eq!(
            hooks[0].command[0],
            home.join("hooks").join("notify.cmd").display().to_string()
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn normalize_post_send_hooks_expands_bare_home_tilde() {
        let (_tempdir, config_root) = config_root_fixture();
        let (home_root, _env) = home_fixture();
        let home = home_root.path().join("home");
        let hooks = vec![RawPostSendHookRule {
            recipient: "*".into(),
            command: vec!["~".into()],
        }];

        let hooks = normalize_post_send_hooks(hooks, &config_root).expect("hooks");

        assert_eq!(hooks[0].command[0], home.display().to_string());
    }

    #[test]
    fn normalize_post_send_hooks_rejects_empty_recipient() {
        let (_tempdir, config_root) = config_root_fixture();
        let error = normalize_post_send_hooks(
            vec![RawPostSendHookRule {
                recipient: "   ".into(),
                command: vec!["bash".into()],
            }],
            &config_root,
        )
        .expect_err("empty recipient should fail");

        assert!(error.message().contains("recipient must not be empty"));
    }

    #[test]
    fn normalize_post_send_hooks_rejects_invalid_recipient_selector() {
        let (_tempdir, config_root) = config_root_fixture();
        let error = normalize_post_send_hooks(
            vec![RawPostSendHookRule {
                recipient: "bad/name".into(),
                command: vec!["bash".into()],
            }],
            &config_root,
        )
        .expect_err("invalid recipient should fail");

        assert!(
            error
                .message()
                .contains("hook recipient name must use only ASCII letters, digits, '-' or '_'")
        );
    }

    #[test]
    fn normalize_post_send_hooks_rejects_empty_command_array() {
        let (_tempdir, config_root) = config_root_fixture();
        let error = normalize_post_send_hooks(
            vec![RawPostSendHookRule {
                recipient: ROLE_TEAM_LEAD.into(),
                command: Vec::new(),
            }],
            &config_root,
        )
        .expect_err("empty command should fail");

        assert!(error.message().contains("command must not be empty"));
    }

    #[test]
    fn normalize_post_send_hooks_rejects_blank_program_name() {
        let (_tempdir, config_root) = config_root_fixture();
        let error = normalize_post_send_hooks(
            vec![RawPostSendHookRule {
                recipient: ROLE_TEAM_LEAD.into(),
                command: vec!["   ".into(), "arg".into()],
            }],
            &config_root,
        )
        .expect_err("blank program should fail");

        assert!(
            error
                .message()
                .contains("command program must not be empty")
        );
    }

    #[test]
    fn normalize_post_send_hooks_rejects_overlong_expanded_path() {
        let (_tempdir, config_root) = config_root_fixture();
        let oversized_tail = "x".repeat(5000);
        let error = normalize_post_send_hooks(
            vec![RawPostSendHookRule {
                recipient: "*".into(),
                command: vec![format!("~/{}", oversized_tail)],
            }],
            &config_root,
        )
        .expect_err("overlong hook path should fail");

        assert!(
            error
                .message()
                .contains("exceeds the maximum supported length")
        );
    }
}
