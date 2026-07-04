#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};

use atm_core::error::AtmError;
use atm_core::test_support::{lock_env, remove_env_var, set_env_var};
use atm_core::{LocalFileNonClaudeOutbound, LocalServiceRuntime, home::atm_home};
use atm_runtime::{
    RuntimeAssembly, RuntimeAssemblyInputs, RuntimeSqliteEvent, RuntimeSqliteObserver,
    assemble_sqlite_runtime,
};

static INSTALL_RETAINED_RUNTIME_FACTORY: Once = Once::new();
// Mutex required because sqlite retained runtimes are cached across concurrent
// tests; bulk clear() is safe because entries are deterministic per path and
// are rebuilt lazily on the next access.
static SQLITE_RUNTIME_CACHE: OnceLock<Mutex<HashMap<PathBuf, LocalServiceRuntime>>> =
    OnceLock::new();
const MAX_SQLITE_RUNTIME_CACHE_ENTRIES: usize = 16;
pub const SQLITE_RUNTIME_PATH_ENV: &str = "ATM_TEST_SQLITE_RUNTIME_PATH";

#[derive(Debug, Default)]
struct NoopRuntimeSqliteObserver;

impl RuntimeSqliteObserver for NoopRuntimeSqliteObserver {
    fn emit_sqlite_event(&self, _event: RuntimeSqliteEvent) -> Result<(), AtmError> {
        Ok(())
    }
}

pub fn install_sqlite_retained_runtime_factory() {
    INSTALL_RETAINED_RUNTIME_FACTORY.call_once(|| {
        atm_core::runtime_install_hooks::install_retained_runtime_factory_for_test_support(
            sqlite_retained_runtime,
        );
    });
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
    let config_current_dir = std::env::current_dir().map_err(|source| {
        AtmError::config("failed to resolve current directory for sqlite test runtime assembly")
            .with_recovery(
                "Run sqlite runtime tests from a readable ATM workspace so retained runtime composition can resolve config.",
            )
            .with_source(source)
    })?;
    {
        let _env_lock = lock_env();
        let _ = atm_home()?;
    }
    assemble_sqlite_runtime(RuntimeAssemblyInputs {
        sqlite_db_path: path.as_ref().to_path_buf(),
        config_current_dir,
        sqlite_observer: std::sync::Arc::new(NoopRuntimeSqliteObserver),
        non_claude_outbound: std::sync::Arc::new(LocalFileNonClaudeOutbound::new()),
    })
}

pub struct SqliteWriterLockGuard {
    _inner: atm_storage_rusqlite::TestOnlySqliteWriterLockGuard,
}

pub fn hold_sqlite_writer_lock(path: impl AsRef<Path>) -> Result<SqliteWriterLockGuard, AtmError> {
    atm_storage_rusqlite::hold_sqlite_writer_lock_for_test(path)
        .map(|inner| SqliteWriterLockGuard { _inner: inner })
}

fn sqlite_retained_runtime() -> Result<LocalServiceRuntime, AtmError> {
    let path = std::env::var_os(SQLITE_RUNTIME_PATH_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| {
            AtmError::daemon_unavailable(
                "sqlite retained runtime is unavailable because no sqlite test runtime path is installed",
            )
            .with_recovery(
                "Install a sqlite retained-runtime guard before running retained-runtime integration tests.",
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
