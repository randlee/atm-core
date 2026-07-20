use crate::error::AtmError;

pub(crate) fn validate_path_segment(value: &str, kind: &str) -> Result<(), AtmError> {
    if value.is_empty() {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not be empty"
        )));
    }

    if value.starts_with('.') {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not start with '.'"
        )));
    }

    if value.contains("..") {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not contain '..'"
        )));
    }

    if value.contains(['/', '\\']) {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not contain path separators"
        )));
    }

    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(AtmError::address_parse(format!(
            "{kind} name contains invalid characters"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_path_segment;

    #[test]
    fn validate_path_segment_rejects_empty_values() {
        assert!(validate_path_segment("", "agent").is_err());
    }

    #[test]
    fn validate_path_segment_rejects_dot_prefixed_values() {
        let error = validate_path_segment(".agent", "agent").expect_err("dot prefix");
        assert!(error.to_string().contains("must not start with '.'"));
    }

    #[test]
    fn validate_path_segment_rejects_double_dot_values() {
        let error = validate_path_segment("agent..name", "agent").expect_err("double dot");
        assert!(error.to_string().contains("must not contain '..'"));
    }

    #[test]
    fn validate_path_segment_rejects_path_separators() {
        let slash = validate_path_segment("bad/name", "agent").expect_err("slash");
        let backslash = validate_path_segment("bad\\name", "agent").expect_err("backslash");

        assert!(slash.to_string().contains("path separators"));
        assert!(backslash.to_string().contains("path separators"));
    }

    #[test]
    fn validate_path_segment_rejects_invalid_characters_and_accepts_valid_names() {
        let error = validate_path_segment("bad name", "agent").expect_err("space");
        assert!(error.to_string().contains("invalid characters"));

        validate_path_segment("valid_name-1.2", "agent").expect("valid");
    }

    /// REQ-SEC-001 lists `.` as a valid local team/agent name character; the
    /// `.` rejection is scoped to cross-host remote-target parsing
    /// (ADR-031, `atm-core::send::parse_send_target_impl`), not this shared
    /// local path-segment validator.
    #[test]
    fn validate_path_segment_accepts_dotted_team_and_agent_names() {
        validate_path_segment("dev.qa", "team").expect("dotted team name is local-valid");
        validate_path_segment("dev.win", "agent").expect("dotted agent name is local-valid");
    }

    #[test]
    fn validate_path_segment_accepts_hyphen_and_underscore_names() {
        validate_path_segment("dev-win_2", "agent").expect("hyphen and underscore allowed");
    }
}
