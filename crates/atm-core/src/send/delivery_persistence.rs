use crate::schema::InboxMessage;

use super::WarningEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryPersistenceDisposition {
    Persisted,
    SqliteFailedRecovered,
}

/// Transient classification of one immutable-ULID storage attempt. This is
/// deliberately not durable delivery state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DuplicateWriteDisposition {
    NotDuplicate,
    AlreadyDeliveredRemote,
    SameStorePeerReceipt,
}

#[derive(Debug, Clone)]
pub(crate) struct DeliveryPersistenceResult {
    pub(crate) disposition: DeliveryPersistenceDisposition,
    pub(crate) duplicate_disposition: DuplicateWriteDisposition,
    /// True only when this write inserted the immutable message record.
    /// Replays of the same ULID are durable no-ops and must not re-run the
    /// daemon-owned post-write action.
    pub(crate) newly_persisted: bool,
    pub(crate) original_message: InboxMessage,
    pub(crate) companion_message: Option<InboxMessage>,
    pub(crate) warnings: Vec<WarningEntry>,
}

impl DeliveryPersistenceResult {
    pub(crate) fn persisted(original_message: InboxMessage) -> Self {
        Self {
            disposition: DeliveryPersistenceDisposition::Persisted,
            duplicate_disposition: DuplicateWriteDisposition::NotDuplicate,
            newly_persisted: true,
            original_message,
            companion_message: None,
            warnings: Vec::new(),
        }
    }

    pub(crate) fn already_persisted(original_message: InboxMessage) -> Self {
        Self {
            disposition: DeliveryPersistenceDisposition::Persisted,
            duplicate_disposition: DuplicateWriteDisposition::AlreadyDeliveredRemote,
            newly_persisted: false,
            original_message,
            companion_message: None,
            warnings: Vec::new(),
        }
    }

    pub(crate) fn same_store_peer_receipt(original_message: InboxMessage) -> Self {
        Self {
            disposition: DeliveryPersistenceDisposition::Persisted,
            duplicate_disposition: DuplicateWriteDisposition::SameStorePeerReceipt,
            newly_persisted: false,
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
            duplicate_disposition: DuplicateWriteDisposition::NotDuplicate,
            newly_persisted: true,
            original_message,
            companion_message: Some(companion_message),
            warnings: vec![warning],
        }
    }

    pub(crate) fn requires_post_write(&self) -> bool {
        self.newly_persisted
            || matches!(
                self.duplicate_disposition,
                DuplicateWriteDisposition::SameStorePeerReceipt
            )
    }
}
