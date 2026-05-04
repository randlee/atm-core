use super::*;

#[test]
fn store_errors_stay_discriminated() {
    let tempdir = TempDir::new().expect("tempdir");
    let error = RusqliteStore::open_path(tempdir.path()).expect_err("directory path should fail");
    assert_eq!(error.kind, StoreErrorKind::Open);
    assert_eq!(
        error.code,
        atm_core::error_codes::AtmErrorCode::StoreOpenFailed
    );

    let busy = crate::classify_store_error(
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::DatabaseBusy,
                extended_code: rusqlite::ffi::SQLITE_BUSY,
            },
            Some("database busy".to_string()),
        ),
        "busy",
    );
    assert_eq!(busy.kind, StoreErrorKind::Busy);
    assert_eq!(busy.code, atm_core::error_codes::AtmErrorCode::StoreBusy);

    let busy_snapshot = crate::classify_store_error(
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::DatabaseBusy,
                extended_code: rusqlite::ffi::SQLITE_BUSY_SNAPSHOT,
            },
            Some("database busy snapshot".to_string()),
        ),
        "busy_snapshot",
    );
    assert_eq!(busy_snapshot.kind, StoreErrorKind::Busy);
    assert_eq!(
        busy_snapshot.code,
        atm_core::error_codes::AtmErrorCode::StoreBusy
    );

    let constraint = crate::classify_store_error(
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                extended_code: rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY,
            },
            Some("constraint failed".to_string()),
        ),
        "constraint",
    );
    assert_eq!(constraint.kind, StoreErrorKind::Constraint);
    assert_eq!(
        constraint.code,
        atm_core::error_codes::AtmErrorCode::StoreConstraintViolation
    );

    let query = crate::classify_store_error(rusqlite::Error::InvalidQuery, "query");
    assert_eq!(query.kind, StoreErrorKind::Query);
    assert_eq!(
        query.code,
        atm_core::error_codes::AtmErrorCode::StoreQueryFailed
    );

    let bootstrap_dir = TempDir::new().expect("tempdir");
    let bootstrap_path = bootstrap_dir.path().join("bootstrap-readonly.db");
    let bootstrap_connection = Connection::open(&bootstrap_path).expect("open bootstrap db");
    drop(bootstrap_connection);
    let mut readonly_uninitialized =
        Connection::open_with_flags(&bootstrap_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open readonly bootstrap db");
    let bootstrap = crate::bootstrap_schema(&mut readonly_uninitialized)
        .expect_err("readonly bootstrap should fail");
    assert_eq!(bootstrap.kind, StoreErrorKind::Bootstrap);
    assert_eq!(
        bootstrap.code,
        atm_core::error_codes::AtmErrorCode::StoreBootstrapFailed
    );

    let migration_dir = TempDir::new().expect("tempdir");
    let migration_path = migration_dir.path().join("migration-readonly.db");
    let mut migration_connection = Connection::open(&migration_path).expect("open migration db");
    crate::bootstrap_schema(&mut migration_connection).expect("bootstrap writable db");
    drop(migration_connection);
    let mut readonly_initialized =
        Connection::open_with_flags(&migration_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open readonly initialized db");
    let migration = crate::bootstrap_schema(&mut readonly_initialized)
        .expect_err("readonly migration should fail");
    assert_eq!(migration.kind, StoreErrorKind::Bootstrap);
    assert_eq!(
        migration.code,
        atm_core::error_codes::AtmErrorCode::StoreBootstrapFailed
    );

    let transaction_dir = TempDir::new().expect("tempdir");
    let store =
        RusqliteStore::open_path(transaction_dir.path().join("mail.db")).expect("open store");
    let transaction = store
        .with_transaction(|_| {
            Err::<(), _>(atm_core::store::StoreError::transaction(
                "synthetic rollback",
            ))
        })
        .expect_err("transaction helper should preserve transaction errors");
    assert_eq!(transaction.kind, StoreErrorKind::Transaction);
    assert_eq!(
        transaction.code,
        atm_core::error_codes::AtmErrorCode::StoreTransactionFailed
    );
}
