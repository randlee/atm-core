#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use atm_core::error::AtmError;
use atm_core::test_support::{lock_env, remove_env_var, set_env_var};
use atm_core::{
    LocalFileNonClaudeOutbound, LocalServiceRuntime,
    home::{atm_home, current_host_runtime_scope},
};
use atm_runtime::{RuntimeAssembly, RuntimeAssemblyInputs, assemble_runtime};
use atm_storage_rusqlite::SqliteStorageFactory;

pub use atm_storage_rusqlite::{TemplateAdmissionMessage, TemplateAdmissionSnapshot};

// Mutex required because sqlite retained runtimes are cached across concurrent
// tests; bulk clear() is safe because entries are deterministic per path and
// are rebuilt lazily on the next access.
static SQLITE_RUNTIME_CACHE: OnceLock<Mutex<HashMap<PathBuf, LocalServiceRuntime>>> =
    OnceLock::new();
const MAX_SQLITE_RUNTIME_CACHE_ENTRIES: usize = 16;
pub const SQLITE_RUNTIME_PATH_ENV: &str = "ATM_TEST_SQLITE_RUNTIME_PATH";

pub fn install_sqlite_retained_runtime_factory() {
    // The test runtime provider is process-global and production-style
    // composition tests may replace it. Reinstall the sqlite factory for each
    // fixture so retained-boundary tests cannot depend on test execution order.
    atm_core::runtime_install_hooks::install_retained_runtime_factory_for_test_support(
        sqlite_retained_runtime,
    );
}

pub struct SqliteRuntimeGuard {
    previous: Option<PathBuf>,
}

impl SqliteRuntimeGuard {
    pub fn install(path: impl Into<PathBuf>) -> Self {
        install_sqlite_retained_runtime_factory();
        let _env_lock = lock_env();
        let previous = std::env::var_os(SQLITE_RUNTIME_PATH_ENV).map(PathBuf::from);
        set_env_var(SQLITE_RUNTIME_PATH_ENV, path.into().into_os_string());
        Self { previous }
    }
}

impl Drop for SqliteRuntimeGuard {
    fn drop(&mut self) {
        let _env_lock = lock_env();
        match self.previous.take() {
            Some(previous) => set_env_var(SQLITE_RUNTIME_PATH_ENV, previous.into_os_string()),
            None => remove_env_var(SQLITE_RUNTIME_PATH_ENV),
        }
    }
}

pub fn open_sqlite_boundary(path: impl AsRef<Path>) -> Result<RuntimeAssembly, AtmError> {
    let config_current_dir = std::env::current_dir().map_err(|_source| {
        AtmError::config("failed to resolve current directory for sqlite test runtime assembly")
    })?;
    {
        let _env_lock = lock_env();
        let _ = atm_home()?;
    }
    assemble_runtime(RuntimeAssemblyInputs {
        host_runtime_scope: current_host_runtime_scope()?,
        storage_factory: std::sync::Arc::new(SqliteStorageFactory::at_path(
            path.as_ref().to_path_buf(),
        )),
        config_current_dir,
        non_claude_outbound: std::sync::Arc::new(LocalFileNonClaudeOutbound::new()),
        template_composer: None,
        workflow_telemetry: None,
    })
}

/// Open the graft endpoint registry for replacement-router tests without
/// exposing a concrete SQLite handle to the HTTP runtime.
pub fn open_graft_receiver_endpoint_store(
    path: impl AsRef<Path>,
) -> Result<Arc<dyn atm_core::GraftReceiverEndpointStore + Send + Sync>, AtmError> {
    let backend = atm_storage_rusqlite::SqliteStorageBackend::new(path)?;
    Ok(backend.graft_receiver_endpoint_store())
}

/// Build an isolated SQLite runtime from a test-owned directory.
///
/// Every caller receives a distinct database filename, so independent unit
/// fixtures can safely create their durable state even when another test owns
/// a transaction in the same process.
pub fn open_isolated_sqlite_boundary(root: impl AsRef<Path>) -> Result<RuntimeAssembly, AtmError> {
    open_sqlite_boundary(root.as_ref().join("runtime").join("mail.sqlite3"))
}

/// Install the current test's isolated runtime path before composing a
/// loopback client. The caller must hold the test environment guard while the
/// returned value remains alive.
pub fn install_isolated_sqlite_runtime(root: impl AsRef<Path>) -> SqliteRuntimeGuard {
    SqliteRuntimeGuard::install(root.as_ref().join("runtime").join("mail.sqlite3"))
}

pub struct SqliteWriterLockGuard {
    _inner: atm_storage_rusqlite::TestOnlySqliteWriterLockGuard,
}

pub fn hold_sqlite_writer_lock(path: impl AsRef<Path>) -> Result<SqliteWriterLockGuard, AtmError> {
    atm_storage_rusqlite::hold_sqlite_writer_lock_for_test(path)
        .map(|inner| SqliteWriterLockGuard { _inner: inner })
}

/// Configure the isolated SQLite fixture to reject every mailbox insert.
///
/// This is a deterministic durable-write failure probe for adapter tests. It
/// avoids using lock contention as a proxy for an error, which is sensitive to
/// SQLite busy-timeout scheduling across operating systems.
pub fn install_sqlite_message_write_failure(path: impl AsRef<Path>) -> Result<(), AtmError> {
    atm_storage_rusqlite::install_message_write_failure_for_test(path)
}

/// Inspects a SQLite fixture through test support, never from the replacement
/// HTTP runtime itself. Production callers must use storage contracts.
pub fn inspect_template_admission_for_test(
    path: impl AsRef<Path>,
    message_keys: &[String],
) -> Result<TemplateAdmissionSnapshot, AtmError> {
    atm_storage_rusqlite::inspect_template_admission_for_test(path, message_keys)
}

fn sqlite_retained_runtime() -> Result<LocalServiceRuntime, AtmError> {
    let path = std::env::var_os(SQLITE_RUNTIME_PATH_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| {
            AtmError::daemon_unavailable(
                "sqlite retained runtime is unavailable because no sqlite test runtime path is installed",
            )

        })?;

    let runtime_cache = SQLITE_RUNTIME_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut runtime_cache = runtime_cache.lock().expect("sqlite runtime cache");
    if let Some(runtime) = runtime_cache.get(&path) {
        return Ok(runtime.clone());
    }

    let assembly = open_sqlite_boundary(&path)?;
    let runtime = assembly.service_runtime.clone();
    if runtime_cache.len() >= MAX_SQLITE_RUNTIME_CACHE_ENTRIES {
        runtime_cache.clear();
    }
    runtime_cache.insert(path, runtime.clone());
    Ok(runtime)
}
