use std::collections::{HashMap, HashSet};

use crate::schema::{AtmMessageId, MessageEnvelope, ThreadMode};
use crate::types::{AgentName, IsoTimestamp};

const MAX_LOGICAL_THREAD_TEXT_BYTES: usize = 256 * 1024;
const TRUNCATED_THREAD_CONTEXT_SENTINEL: &str = "\n\n[ATM thread context truncated]";

pub(crate) fn canonical_sender_identity(message: &MessageEnvelope) -> AgentName {
    message.from.clone()
}

pub(crate) fn is_ephemeral(message: &MessageEnvelope) -> bool {
    message.stale_at.is_some()
}

pub(crate) fn is_expired_ephemeral(message: &MessageEnvelope, now: IsoTimestamp) -> bool {
    message.stale_at.is_some_and(|stale_at| stale_at <= now)
}

pub(crate) struct ThreadIndex<'a> {
    by_id: HashMap<AtmMessageId, &'a MessageEnvelope>,
    children: HashMap<AtmMessageId, Vec<&'a MessageEnvelope>>,
}

impl<'a> ThreadIndex<'a> {
    pub(crate) fn new(messages: &'a [MessageEnvelope]) -> Self {
        let mut by_id = HashMap::new();
        let mut children: HashMap<AtmMessageId, Vec<&MessageEnvelope>> = HashMap::new();

        for message in messages {
            if let Some(message_id) = message.message_id {
                by_id.insert(message_id, message);
            }
        }

        for message in messages {
            if let Some(parent_id) = message.parent_message_id {
                children.entry(parent_id).or_default().push(message);
            }
        }

        Self { by_id, children }
    }

    pub(crate) fn message(&self, message_id: AtmMessageId) -> Option<&'a MessageEnvelope> {
        self.by_id.get(&message_id).copied()
    }

    pub(crate) fn root_id(&self, message_id: AtmMessageId) -> Option<AtmMessageId> {
        let mut current = message_id;
        let mut seen = HashSet::new();

        loop {
            let message = self.message(current)?;
            let Some(parent_id) = message.parent_message_id else {
                return Some(current);
            };
            if !seen.insert(current) {
                return Some(current);
            }
            current = parent_id;
        }
    }

    pub(crate) fn successor_count(&self, parent_id: AtmMessageId) -> usize {
        self.children.get(&parent_id).map_or(0, Vec::len)
    }

    pub(crate) fn has_successor(&self, parent_id: AtmMessageId) -> bool {
        self.successor_count(parent_id) > 0
    }

    pub(crate) fn terminal_id(&self, message_id: AtmMessageId) -> Option<AtmMessageId> {
        let mut current = message_id;
        let mut seen = HashSet::new();

        loop {
            if !seen.insert(current) {
                return Some(current);
            }

            let Some(successor) = self.primary_successor(current) else {
                return Some(current);
            };
            current = successor.message_id?;
        }
    }

    pub(crate) fn is_terminal(&self, message_id: AtmMessageId) -> bool {
        self.terminal_id(message_id) == Some(message_id)
    }

    pub(crate) fn logical_current_envelope(
        &self,
        message_id: AtmMessageId,
    ) -> Option<MessageEnvelope> {
        let terminal_id = self.terminal_id(message_id)?;
        let terminal = self.message(terminal_id)?.clone();
        if terminal.thread_mode != Some(ThreadMode::AddDetails) {
            return Some(terminal);
        }

        let chain = self.chain_messages(terminal_id);
        let start_index = chain
            .iter()
            .rposition(|message| message.thread_mode == Some(ThreadMode::Supersede))
            .unwrap_or(0);
        let mut composed_text = chain[start_index..]
            .iter()
            .filter_map(|message| {
                let trimmed = message.text.trim();
                (!trimmed.is_empty()).then_some(trimmed)
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if composed_text.len() > MAX_LOGICAL_THREAD_TEXT_BYTES {
            let mut truncate_at = MAX_LOGICAL_THREAD_TEXT_BYTES
                .saturating_sub(TRUNCATED_THREAD_CONTEXT_SENTINEL.len());
            while truncate_at > 0 && !composed_text.is_char_boundary(truncate_at) {
                truncate_at -= 1;
            }
            composed_text.truncate(truncate_at);
            composed_text.push_str(TRUNCATED_THREAD_CONTEXT_SENTINEL);
        }

        let mut logical = terminal;
        if !composed_text.is_empty() {
            logical.text = composed_text;
        }
        Some(logical)
    }

    pub(crate) fn thread_requires_ack(&self, message_id: AtmMessageId) -> bool {
        self.chain_messages(message_id)
            .into_iter()
            .any(|message| message.pending_ack_at.is_some() || message.acknowledged_at.is_some())
    }

    pub(crate) fn chain_messages(&self, message_id: AtmMessageId) -> Vec<&'a MessageEnvelope> {
        let Some(root_id) = self.root_id(message_id) else {
            return Vec::new();
        };
        let mut chain = Vec::new();
        let mut current = Some(root_id);
        let mut seen = HashSet::new();

        while let Some(message_id) = current {
            if !seen.insert(message_id) {
                break;
            }
            let Some(message) = self.message(message_id) else {
                break;
            };
            chain.push(message);
            current = self
                .primary_successor(message_id)
                .and_then(|next| next.message_id);
        }

        chain
    }

    fn primary_successor(&self, parent_id: AtmMessageId) -> Option<&'a MessageEnvelope> {
        let successors = self.children.get(&parent_id)?;
        successors.iter().copied().max_by(|left, right| {
            left.timestamp
                .cmp(&right.timestamp)
                .then_with(|| left.message_id.cmp(&right.message_id))
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::{
        MAX_LOGICAL_THREAD_TEXT_BYTES, TRUNCATED_THREAD_CONTEXT_SENTINEL, ThreadIndex,
        canonical_sender_identity, is_ephemeral, is_expired_ephemeral,
    };
    use crate::schema::{AtmMessageId, MessageEnvelope, ThreadMode};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    fn message(
        from: &str,
        message_id: AtmMessageId,
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
    ) -> MessageEnvelope {
        MessageEnvelope {
            from: from.parse::<AgentName>().expect("agent"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id,
            thread_mode,
            stale_at: None,
            task_id: None,
            extra: Map::new(),
        }
    }

    #[test]
    fn canonical_sender_identity_uses_from_field() {
        let message = message(TEST_SENDER, AtmMessageId::new(), None, None);

        assert_eq!(canonical_sender_identity(&message).as_str(), TEST_SENDER);
    }

    #[test]
    fn thread_index_resolves_root_terminal_and_ack_requirement() {
        let root_id = AtmMessageId::new();
        let terminal_id = AtmMessageId::new();
        let mut root = message(TEST_SENDER, root_id, None, None);
        root.acknowledged_at = Some(IsoTimestamp::now());
        let terminal = message(
            TEST_SENDER,
            terminal_id,
            Some(root_id),
            Some(ThreadMode::AddDetails),
        );
        let messages = vec![root, terminal];
        let index = ThreadIndex::new(&messages);

        assert_eq!(index.root_id(terminal_id), Some(root_id));
        assert_eq!(index.terminal_id(root_id), Some(terminal_id));
        assert!(index.thread_requires_ack(terminal_id));
    }

    #[test]
    fn logical_current_envelope_appends_add_details_context() {
        let root_id = AtmMessageId::new();
        let detail_id = AtmMessageId::new();
        let messages = vec![
            MessageEnvelope {
                text: "root context".to_string(),
                ..message(TEST_SENDER, root_id, None, None)
            },
            MessageEnvelope {
                text: "follow-up detail".to_string(),
                ..message(
                    TEST_SENDER,
                    detail_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                )
            },
        ];
        let index = ThreadIndex::new(&messages);

        let logical = index
            .logical_current_envelope(root_id)
            .expect("logical current envelope");

        assert_eq!(logical.message_id, Some(detail_id));
        assert_eq!(logical.text, "root context\n\nfollow-up detail");
    }

    #[test]
    fn logical_current_envelope_resets_context_after_supersede() {
        let root_id = AtmMessageId::new();
        let supersede_id = AtmMessageId::new();
        let detail_id = AtmMessageId::new();
        let messages = vec![
            MessageEnvelope {
                text: "root context".to_string(),
                ..message(TEST_SENDER, root_id, None, None)
            },
            MessageEnvelope {
                text: "replacement instruction".to_string(),
                ..message(
                    TEST_SENDER,
                    supersede_id,
                    Some(root_id),
                    Some(ThreadMode::Supersede),
                )
            },
            MessageEnvelope {
                text: "follow-up detail".to_string(),
                ..message(
                    TEST_SENDER,
                    detail_id,
                    Some(supersede_id),
                    Some(ThreadMode::AddDetails),
                )
            },
        ];
        let index = ThreadIndex::new(&messages);

        let logical = index
            .logical_current_envelope(root_id)
            .expect("logical current envelope");

        assert_eq!(logical.message_id, Some(detail_id));
        assert_eq!(logical.text, "replacement instruction\n\nfollow-up detail");
    }

    #[test]
    fn logical_current_envelope_truncates_oversized_add_details_chain() {
        let root_id = AtmMessageId::new();
        let detail_id = AtmMessageId::new();
        let messages = vec![
            MessageEnvelope {
                text: "a".repeat(200_000),
                ..message(TEST_SENDER, root_id, None, None)
            },
            MessageEnvelope {
                text: "b".repeat(200_000),
                ..message(
                    TEST_SENDER,
                    detail_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                )
            },
        ];
        let index = ThreadIndex::new(&messages);

        let logical = index
            .logical_current_envelope(root_id)
            .expect("logical current envelope");

        assert_eq!(logical.message_id, Some(detail_id));
        assert!(logical.text.len() <= MAX_LOGICAL_THREAD_TEXT_BYTES);
        assert!(logical.text.ends_with(TRUNCATED_THREAD_CONTEXT_SENTINEL));
    }

    #[test]
    fn ephemeral_helpers_use_stale_at() {
        let mut message = message(TEST_SENDER, AtmMessageId::new(), None, None);
        let stale_at = IsoTimestamp::now();
        message.stale_at = Some(stale_at);

        assert!(is_ephemeral(&message));
        assert!(is_expired_ephemeral(&message, stale_at));
    }
}
