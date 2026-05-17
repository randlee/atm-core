use crate::schema::MessageEnvelope;

use super::WarningEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryPersistenceDisposition {
    Persisted,
    SqliteFailedRecovered,
}

#[derive(Debug, Clone)]
pub(crate) struct DeliveryPersistenceResult {
    pub(crate) disposition: DeliveryPersistenceDisposition,
    pub(crate) original_message: MessageEnvelope,
    pub(crate) companion_message: Option<MessageEnvelope>,
    pub(crate) warnings: Vec<WarningEntry>,
}

impl DeliveryPersistenceResult {
    pub(crate) fn persisted(original_message: MessageEnvelope) -> Self {
        Self {
            disposition: DeliveryPersistenceDisposition::Persisted,
            original_message,
            companion_message: None,
            warnings: Vec::new(),
        }
    }

    pub(crate) fn sqlite_failed_recovered(
        original_message: MessageEnvelope,
        companion_message: MessageEnvelope,
        warning: WarningEntry,
    ) -> Self {
        Self {
            disposition: DeliveryPersistenceDisposition::SqliteFailedRecovered,
            original_message,
            companion_message: Some(companion_message),
            warnings: vec![warning],
        }
    }
}
