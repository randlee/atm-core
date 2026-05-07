use std::collections::{HashMap, HashSet};

use crate::schema::{LegacyMessageId, MessageEnvelope};
use crate::types::{AgentName, IsoTimestamp};

pub(crate) fn canonical_sender_identity(message: &MessageEnvelope) -> AgentName {
    message
        .extra
        .get("metadata")
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get("atm"))
        .and_then(serde_json::Value::as_object)
        .and_then(|atm| atm.get("fromIdentity"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_else(|| message.from.clone())
}

pub(crate) fn is_ephemeral(message: &MessageEnvelope) -> bool {
    message.stale_at.is_some()
}

pub(crate) fn is_expired_ephemeral(message: &MessageEnvelope, now: IsoTimestamp) -> bool {
    message.stale_at.is_some_and(|stale_at| stale_at <= now)
}

pub(crate) struct ThreadIndex<'a> {
    by_id: HashMap<LegacyMessageId, &'a MessageEnvelope>,
    children: HashMap<LegacyMessageId, Vec<&'a MessageEnvelope>>,
}

impl<'a> ThreadIndex<'a> {
    pub(crate) fn new(messages: &'a [MessageEnvelope]) -> Self {
        let mut by_id = HashMap::new();
        let mut children: HashMap<LegacyMessageId, Vec<&MessageEnvelope>> = HashMap::new();

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

    pub(crate) fn message(&self, message_id: LegacyMessageId) -> Option<&'a MessageEnvelope> {
        self.by_id.get(&message_id).copied()
    }

    pub(crate) fn root_id(&self, message_id: LegacyMessageId) -> Option<LegacyMessageId> {
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

    pub(crate) fn successor_count(&self, parent_id: LegacyMessageId) -> usize {
        self.children.get(&parent_id).map_or(0, Vec::len)
    }

    pub(crate) fn has_successor(&self, parent_id: LegacyMessageId) -> bool {
        self.successor_count(parent_id) > 0
    }

    pub(crate) fn terminal_id(&self, message_id: LegacyMessageId) -> Option<LegacyMessageId> {
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

    pub(crate) fn is_terminal(&self, message_id: LegacyMessageId) -> bool {
        self.terminal_id(message_id) == Some(message_id)
    }

    pub(crate) fn thread_requires_ack(&self, message_id: LegacyMessageId) -> bool {
        self.chain_messages(message_id)
            .into_iter()
            .any(|message| message.pending_ack_at.is_some() || message.acknowledged_at.is_some())
    }

    pub(crate) fn chain_messages(&self, message_id: LegacyMessageId) -> Vec<&'a MessageEnvelope> {
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

    fn primary_successor(&self, parent_id: LegacyMessageId) -> Option<&'a MessageEnvelope> {
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

    use super::{ThreadIndex, canonical_sender_identity, is_ephemeral, is_expired_ephemeral};
    use crate::schema::{LegacyMessageId, MessageEnvelope, ThreadMode};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    fn message(
        from: &str,
        message_id: LegacyMessageId,
        parent_message_id: Option<LegacyMessageId>,
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
    fn canonical_sender_identity_prefers_metadata_override() {
        let mut message = message(TEST_SENDER, LegacyMessageId::new(), None, None);
        message.extra.insert(
            "metadata".to_string(),
            serde_json::json!({"atm": {"fromIdentity": "canonical-sender"}}),
        );

        assert_eq!(
            canonical_sender_identity(&message).as_str(),
            "canonical-sender"
        );
    }

    #[test]
    fn thread_index_resolves_root_terminal_and_ack_requirement() {
        let root_id = LegacyMessageId::new();
        let terminal_id = LegacyMessageId::new();
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
    fn ephemeral_helpers_use_stale_at() {
        let mut message = message(TEST_SENDER, LegacyMessageId::new(), None, None);
        let stale_at = IsoTimestamp::now();
        message.stale_at = Some(stale_at);

        assert!(is_ephemeral(&message));
        assert!(is_expired_ephemeral(&message, stale_at));
    }
}
