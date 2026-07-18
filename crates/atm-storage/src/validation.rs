use crate::error::AtmError;

pub fn validate_path_segment(value: &str, kind: &str) -> Result<(), AtmError> {
    if value.is_empty() {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not be empty"
        )));
    }

    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(AtmError::address_parse(format!(
            "{kind} name must use only ASCII letters, digits, '-' or '_'"
        )));
    }

    Ok(())
}

pub fn validate_agent_at_team(value: &str, kind: &str) -> Result<(), AtmError> {
    match value.split_once('@') {
        Some((agent, team)) => {
            validate_path_segment(agent, kind)?;
            validate_path_segment(team, kind)?;
        }
        None => validate_path_segment(value, kind)?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_agent_at_team, validate_path_segment};

    #[test]
    fn validate_path_segment_rejects_empty_values() {
        assert!(validate_path_segment("", "agent").is_err());
    }

    #[test]
    fn validate_path_segment_accepts_safe_set_and_rejects_reserved_characters() {
        for valid in ["alpha-beta", "ops-team", "atm123", "A_B-9"] {
            validate_path_segment(valid, "agent").expect("valid");
        }

        for invalid in [
            ".agent",
            "agent..name",
            "bad/name",
            "bad\\name",
            "bad name",
            "bad\tname",
            "bad:name",
            "bad.name",
            "bad*name",
            "bad?name",
            "bad[name",
            "bad]name",
        ] {
            let error = validate_path_segment(invalid, "agent").expect_err("invalid");
            assert!(
                error
                    .to_string()
                    .contains("must use only ASCII letters, digits, '-' or '_'"),
                "{invalid}: {error}"
            );
        }
    }

    #[test]
    fn validate_agent_at_team_accepts_safe_set_shapes_and_rejects_invalid_segments() {
        for valid in [
            "alpha-beta@ops-team",
            "route_mgr@qa-lab",
            "notify-node@alpha_beta",
            "relay7",
        ] {
            validate_agent_at_team(valid, "agent id").expect("valid");
        }

        for invalid in [
            "bad.name@ops-team",
            "alpha-beta@bad.team",
            "bad:name@ops-team",
            "alpha-beta@bad team",
            "alpha-beta@",
            "@ops-team",
        ] {
            assert!(
                validate_agent_at_team(invalid, "agent id").is_err(),
                "expected `{invalid}` to fail"
            );
        }
    }
}
