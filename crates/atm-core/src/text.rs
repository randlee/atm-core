pub fn truncate(s: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }

    match s.char_indices().nth(max_chars) {
        Some((index, _)) => &s[..index],
        None => s,
    }
}

#[cfg(test)]
pub fn wrap_lines(s: &str, width: usize) -> String {
    if width == 0 {
        return s.to_string();
    }

    let mut result = String::new();

    for (line_index, line) in s.lines().enumerate() {
        if line_index > 0 {
            result.push('\n');
        }

        let mut current_len = 0usize;
        for word in line.split_whitespace() {
            let word_len = word.chars().count();
            let separator_len = usize::from(current_len > 0);

            if current_len > 0 && current_len + separator_len + word_len > width {
                result.push('\n');
                result.push_str(word);
                current_len = word_len;
            } else {
                if current_len > 0 {
                    result.push(' ');
                }
                result.push_str(word);
                current_len += separator_len + word_len;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{truncate, wrap_lines};

    #[test]
    fn truncate_is_char_boundary_safe() {
        assert_eq!(truncate("naive cafe", 5), "naive");
        assert_eq!(truncate("cafe\u{301}", 4), "cafe");
    }

    #[test]
    fn truncate_returns_original_when_shorter_than_limit() {
        assert_eq!(truncate("short", 10), "short");
    }

    #[test]
    fn wrap_lines_wraps_on_word_boundaries() {
        let wrapped = wrap_lines("alpha beta gamma", 10);
        assert_eq!(wrapped, "alpha beta\ngamma");
    }

    #[test]
    fn wrap_lines_preserves_existing_line_breaks() {
        let wrapped = wrap_lines("alpha beta\ngamma delta", 6);
        assert_eq!(wrapped, "alpha\nbeta\ngamma\ndelta");
    }
}
