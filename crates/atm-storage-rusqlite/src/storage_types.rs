//! Private SQLite adapter state carriers.

use std::sync::Arc;

use atm_storage::types::IsoTimestamp;

use crate::shared_db::SharedDb;

#[derive(Debug)]
pub(super) struct SqliteMessageStore {
    pub(super) db: Arc<SharedDb>,
}

#[derive(Debug)]
pub(super) struct SqliteRosterStore {
    pub(super) db: Arc<SharedDb>,
}

#[derive(Debug)]
pub(super) struct SqliteNudgeTemplateOverrideStore {
    pub(super) db: Arc<SharedDb>,
}

#[derive(Debug)]
pub(super) struct SqlitePendingNudgeStore {
    pub(super) db: Arc<SharedDb>,
}

#[derive(Debug)]
pub(super) struct SqlitePeerConfigStore {
    pub(super) db: Arc<SharedDb>,
}

#[derive(Debug, Clone)]
pub(super) struct StoredMailMessageState {
    pub(super) read: bool,
    pub(super) pending_ack_at: Option<IsoTimestamp>,
    pub(super) acknowledged_at: Option<IsoTimestamp>,
    pub(super) expires_at: Option<IsoTimestamp>,
}
