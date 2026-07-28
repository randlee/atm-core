#![forbid(unsafe_code)]
#![allow(
    deprecated,
    reason = "the SQL Server proof crate intentionally compiles against the transitional shared storage traits"
)]

//! Compile-only proof crate for Phase AC.7.
//!
//! This crate intentionally does not speak to SQL Server yet. Its purpose is to
//! prove that the final `atm-storage` contract is backend-neutral enough for a
//! future `atm-storage-sqlserver` backend to implement without introducing an
//! `atm-core` dependency or another storage-architecture reset.

use atm_storage::{
    AtmError, AtmErrorCode, Message, MessageKey, MessageQuery, MessageStore, RosterSnapshot,
    RosterStore, TeamName,
};

fn compile_only_error(surface: &str) -> AtmError {
    AtmError::new(
        AtmErrorCode::InternalError,
        format!("{surface} is compile-only in atm-storage-sqlserver-proof"),
    )
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SqlServerMessageStore;

impl atm_storage::contract::sealed::Sealed for SqlServerMessageStore {}

impl MessageStore for SqlServerMessageStore {
    fn save_message(&self, _message: &Message) -> Result<(), AtmError> {
        Err(compile_only_error("SqlServerMessageStore::save_message"))
    }

    fn save_messages_atomically(&self, _messages: &[Message]) -> Result<(), AtmError> {
        Err(compile_only_error(
            "SqlServerMessageStore::save_messages_atomically",
        ))
    }

    fn load_message(&self, _key: &MessageKey) -> Result<Option<Message>, AtmError> {
        Err(compile_only_error("SqlServerMessageStore::load_message"))
    }

    fn list_messages(&self, _query: &MessageQuery) -> Result<Vec<Message>, AtmError> {
        Err(compile_only_error("SqlServerMessageStore::list_messages"))
    }

    fn delete_message(&self, _key: &MessageKey) -> Result<(), AtmError> {
        Err(compile_only_error("SqlServerMessageStore::delete_message"))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SqlServerRosterStore;

impl atm_storage::contract::sealed::Sealed for SqlServerRosterStore {}

impl RosterStore for SqlServerRosterStore {
    fn load_roster(&self, _team: &TeamName) -> Result<RosterSnapshot, AtmError> {
        Err(compile_only_error("SqlServerRosterStore::load_roster"))
    }

    fn save_roster(&self, _roster: &RosterSnapshot) -> Result<(), AtmError> {
        Err(compile_only_error("SqlServerRosterStore::save_roster"))
    }

    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
        Err(compile_only_error("SqlServerRosterStore::list_teams"))
    }
}

#[cfg(test)]
mod tests {
    use super::{SqlServerMessageStore, SqlServerRosterStore};
    use atm_storage::{MessageStore, RosterStore};

    fn assert_message_store<T: MessageStore>() {}
    fn assert_roster_store<T: RosterStore>() {}

    #[test]
    fn proof_types_implement_storage_traits() {
        assert_message_store::<SqlServerMessageStore>();
        assert_roster_store::<SqlServerRosterStore>();
    }
}
