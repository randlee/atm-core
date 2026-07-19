use crate::schema::InboxMessage;

use super::WarningEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[deprecated(note = "AG.20 deletion target; remove this symbol and all call sites")]
pub(crate) enum DeliveryPersistenceDisposition {
    Persisted,
    SqliteFailedRecovered,
}

#[derive(Debug, Clone)]
#[deprecated(note = "AG.20 deletion target; remove this symbol and all call sites")]
pub(crate) struct DeliveryPersistenceResult {
    pub(crate) disposition: DeliveryPersistenceDisposition,
    pub(crate) original_message: InboxMessage,
    pub(crate) companion_message: Option<InboxMessage>,
    pub(crate) warnings: Vec<WarningEntry>,
}

impl DeliveryPersistenceResult {
    pub(crate) fn persisted(original_message: InboxMessage) -> Self {
        Self {
            disposition: DeliveryPersistenceDisposition::Persisted,
            original_message,
            companion_message: None,
            warnings: Vec::new(),
        }
    }

    pub(crate) fn sqlite_failed_recovered(
        original_message: InboxMessage,
        companion_message: InboxMessage,
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
