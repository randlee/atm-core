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

#[cfg(test)]
mod tests {
    use super::validate_path_segment;

    #[test]
    fn validate_path_segment_rejects_empty_values() {
        assert!(validate_path_segment("", "agent").is_err());
    }

    #[test]
    fn validate_path_segment_accepts_safe_set_and_rejects_reserved_characters() {
        for valid in ["team-lead", "arch_ctm", "atm123", "A_B-9"] {
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
}
