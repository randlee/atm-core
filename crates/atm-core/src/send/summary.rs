use crate::text::truncate;

const SUMMARY_LIMIT: usize = 100;

pub fn build_summary(message: &str, explicit_summary: Option<String>) -> String {
    if let Some(summary) = explicit_summary.filter(|value| !value.trim().is_empty()) {
        return summary;
    }

    let trimmed = message.trim();
    if trimmed.chars().count() <= SUMMARY_LIMIT {
        return trimmed.to_string();
    }

    let slice = truncate(trimmed, SUMMARY_LIMIT);
    match slice.rfind(char::is_whitespace) {
        Some(index) => format!("{}...", slice[..index].trim_end()),
        None => format!("{slice}..."),
    }
}
