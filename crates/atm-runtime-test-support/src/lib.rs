#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, Once, OnceLock};

use atm_core::error::AtmError;
use atm_core::test_support::{remove_env_var, set_env_var};
use atm_core::{
    LocalFileNonClaudeOutbound, LocalFileNotificationSink, LocalServiceRuntime,
    home::{atm_home, host_runtime_dir_from_home},
};
use atm_rusqlite::{SqliteBoundaryAssembly, SqliteWriterLockGuard};

static INSTALL_RETAINED_RUNTIME_FACTORY: Once = Once::new();
// Mutex required because sqlite retained runtimes are cached across concurrent
// tests; bulk clear() is safe because entries are deterministic per path and
// are rebuilt lazily on the next access.
static SQLITE_RUNTIME_CACHE: OnceLock<Mutex<HashMap<PathBuf, LocalServiceRuntime>>> =
    OnceLock::new();
const MAX_SQLITE_RUNTIME_CACHE_ENTRIES: usize = 16;
pub const SQLITE_RUNTIME_PATH_ENV: &str = "ATM_TEST_SQLITE_RUNTIME_PATH";

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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
    _env_lock: MutexGuard<'static, ()>,
}

impl SqliteRuntimeGuard {
    pub fn install(path: impl Into<PathBuf>) -> Self {
        install_sqlite_retained_runtime_factory();
        let env_guard = env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var_os(SQLITE_RUNTIME_PATH_ENV).map(PathBuf::from);
        set_env_var(SQLITE_RUNTIME_PATH_ENV, path.into().into_os_string());
        Self {
            previous,
            _env_lock: env_guard,
        }
    }
}

impl Drop for SqliteRuntimeGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => set_env_var(SQLITE_RUNTIME_PATH_ENV, previous.into_os_string()),
            None => remove_env_var(SQLITE_RUNTIME_PATH_ENV),
        }
    }
}

pub fn open_sqlite_boundary(path: impl AsRef<Path>) -> Result<SqliteBoundaryAssembly, AtmError> {
    SqliteBoundaryAssembly::new(path.as_ref())
}

pub fn hold_sqlite_writer_lock(path: impl AsRef<Path>) -> Result<SqliteWriterLockGuard, AtmError> {
    atm_rusqlite::hold_sqlite_writer_lock(path)
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

    let assembly = SqliteBoundaryAssembly::new(&path)?;
    let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        assembly.mail_store_arc(),
        assembly.task_store_arc(),
        assembly.roster_store_arc(),
        std::sync::Arc::new(LocalFileNonClaudeOutbound::new()),
        std::sync::Arc::new(LocalFileNotificationSink::at_path(
            host_runtime_dir_from_home(&atm_home()?).join("notifications.jsonl"),
        )),
    );
    if runtime_cache.len() >= MAX_SQLITE_RUNTIME_CACHE_ENTRIES {
        runtime_cache.clear();
    }
    runtime_cache.insert(path, runtime.clone());
    Ok(runtime)
}
