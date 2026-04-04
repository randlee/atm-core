use std::collections::HashMap;

use crate::schema::LegacyMessageId;
use crate::types::IsoTimestamp;

/// Canonicalize a merged mailbox surface by the legacy top-level `message_id`
/// owned by docs/atm-message-schema.md §2 and
/// docs/atm-core/design/dedup-metadata-schema.md §3.1. For read/ack/clear, the
/// newest message for a given LegacyMessageId wins; equal timestamps fall back
/// to the later merged-surface position.
pub(crate) fn dedupe_legacy_message_id_surface<T, FId, FTs>(
    messages: Vec<T>,
    mut legacy_message_id: FId,
    mut timestamp: FTs,
) -> Vec<T>
where
    FId: FnMut(&T) -> Option<LegacyMessageId>,
    FTs: FnMut(&T) -> IsoTimestamp,
{
    let mut latest_for_id: HashMap<LegacyMessageId, (IsoTimestamp, usize)> = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        if let Some(message_id) = legacy_message_id(message) {
            latest_for_id
                .entry(message_id)
                .and_modify(|entry| {
                    let message_timestamp = timestamp(message);
                    if message_timestamp > entry.0
                        || (message_timestamp == entry.0 && index > entry.1)
                    {
                        *entry = (message_timestamp, index);
                    }
                })
                .or_insert((timestamp(message), index));
        }
    }

    messages
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| match legacy_message_id(&message) {
            Some(message_id) => latest_for_id
                .get(&message_id)
                .and_then(|(_, keep_index)| (*keep_index == index).then_some(message)),
            None => Some(message),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::schema::LegacyMessageId;
    use crate::types::IsoTimestamp;

    use super::dedupe_legacy_message_id_surface;

    #[derive(Clone)]
    struct SurfaceRecord {
        message_id: Option<LegacyMessageId>,
        timestamp: IsoTimestamp,
        body: &'static str,
    }

    #[test]
    fn dedupe_legacy_message_id_surface_keeps_newest_timestamp() {
        let message_id = LegacyMessageId::new();
        let messages = vec![
            SurfaceRecord {
                message_id: Some(message_id),
                timestamp: iso("2026-04-04T10:00:00Z"),
                body: "older",
            },
            SurfaceRecord {
                message_id: Some(message_id),
                timestamp: iso("2026-04-04T10:00:01Z"),
                body: "newer",
            },
        ];

        let deduped = dedupe_legacy_message_id_surface(
            messages,
            |message| message.message_id,
            |message| message.timestamp,
        );

        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].body, "newer");
    }

    #[test]
    fn dedupe_legacy_message_id_surface_keeps_later_position_on_timestamp_tie() {
        let message_id = LegacyMessageId::new();
        let timestamp = iso("2026-04-04T10:00:00Z");
        let messages = vec![
            SurfaceRecord {
                message_id: Some(message_id),
                timestamp,
                body: "first",
            },
            SurfaceRecord {
                message_id: Some(message_id),
                timestamp,
                body: "second",
            },
        ];

        let deduped = dedupe_legacy_message_id_surface(
            messages,
            |message| message.message_id,
            |message| message.timestamp,
        );

        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].body, "second");
    }

    #[test]
    fn dedupe_legacy_message_id_surface_preserves_records_without_message_id() {
        let message_id = LegacyMessageId::new();
        let messages = vec![
            SurfaceRecord {
                message_id: None,
                timestamp: iso("2026-04-04T10:00:00Z"),
                body: "no-id",
            },
            SurfaceRecord {
                message_id: Some(message_id),
                timestamp: iso("2026-04-04T10:00:01Z"),
                body: "first",
            },
            SurfaceRecord {
                message_id: Some(message_id),
                timestamp: iso("2026-04-04T10:00:02Z"),
                body: "second",
            },
        ];

        let deduped = dedupe_legacy_message_id_surface(
            messages,
            |message| message.message_id,
            |message| message.timestamp,
        );

        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].body, "no-id");
        assert_eq!(deduped[1].body, "second");
    }

    #[test]
    fn dedupe_legacy_message_id_surface_keeps_distinct_ids() {
        let first_id = LegacyMessageId::new();
        let second_id = LegacyMessageId::new();
        let messages = vec![
            SurfaceRecord {
                message_id: Some(first_id),
                timestamp: iso("2026-04-04T10:00:00Z"),
                body: "first-id",
            },
            SurfaceRecord {
                message_id: Some(second_id),
                timestamp: iso("2026-04-04T10:00:01Z"),
                body: "second-id",
            },
        ];

        let deduped = dedupe_legacy_message_id_surface(
            messages,
            |message| message.message_id,
            |message| message.timestamp,
        );

        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].body, "first-id");
        assert_eq!(deduped[1].body, "second-id");
    }

    fn iso(value: &str) -> IsoTimestamp {
        IsoTimestamp::from_datetime(
            chrono::DateTime::parse_from_rfc3339(value)
                .expect("timestamp")
                .with_timezone(&Utc),
        )
    }
}
