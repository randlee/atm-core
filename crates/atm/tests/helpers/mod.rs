use serde_json::Value;

pub fn parse_inbox_values(raw: &str) -> Vec<Value> {
    if raw.trim().is_empty() {
        return Vec::new();
    }

    match raw.chars().find(|ch| !ch.is_whitespace()) {
        Some('[') => serde_json::from_str(raw).expect("json array"),
        _ => raw
            .lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect(),
    }
}
