#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};

use atm_core::LocalServiceRuntime;
use atm_core::error::AtmError;
use atm_rusqlite::SqliteBoundaryAssembly;

static INSTALL_RETAINED_RUNTIME_FACTORY: Once = Once::new();
static SQLITE_RUNTIME_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static SQLITE_RUNTIME_CACHE: OnceLock<Mutex<HashMap<PathBuf, LocalServiceRuntime>>> =
    OnceLock::new();

pub fn install_sqlite_retained_runtime_factory() {
    INSTALL_RETAINED_RUNTIME_FACTORY.call_once(|| {
        atm_core::install_default_runtime_factory(sqlite_retained_runtime);
    });
}

pub struct SqliteRuntimeGuard {
    previous: Option<PathBuf>,
}

impl SqliteRuntimeGuard {
    pub fn install(path: impl Into<PathBuf>) -> Self {
        install_sqlite_retained_runtime_factory();
        let runtime_path = SQLITE_RUNTIME_PATH.get_or_init(|| Mutex::new(None));
        let mut runtime_path = runtime_path.lock().expect("sqlite runtime path");
        let previous = runtime_path.replace(path.into());
        Self { previous }
    }
}

impl Drop for SqliteRuntimeGuard {
    fn drop(&mut self) {
        let runtime_path = SQLITE_RUNTIME_PATH.get_or_init(|| Mutex::new(None));
        let mut runtime_path = runtime_path.lock().expect("sqlite runtime path");
        *runtime_path = self.previous.take();
    }
}

pub fn open_sqlite_boundary(path: impl AsRef<Path>) -> Result<SqliteBoundaryAssembly, AtmError> {
    SqliteBoundaryAssembly::new(path.as_ref())
}

fn sqlite_retained_runtime() -> Result<LocalServiceRuntime, AtmError> {
    let path = {
        let runtime_path = SQLITE_RUNTIME_PATH.get_or_init(|| Mutex::new(None));
        runtime_path
            .lock()
            .expect("sqlite runtime path")
            .clone()
            .ok_or_else(|| {
                AtmError::daemon_unavailable(
                    "sqlite retained runtime is unavailable because no sqlite test runtime path is installed",
                )
                .with_recovery(
                    "Install a sqlite retained-runtime guard before running retained-runtime integration tests.",
                )
            })?
    };

    let runtime_cache = SQLITE_RUNTIME_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut runtime_cache = runtime_cache.lock().expect("sqlite runtime cache");
    if let Some(runtime) = runtime_cache.get(&path) {
        return Ok(runtime.clone());
    }

    let assembly = SqliteBoundaryAssembly::new(&path)?;
    let runtime = LocalServiceRuntime::new(
        assembly.mail_store_arc(),
        assembly.task_store_arc(),
        assembly.roster_store_arc(),
    );
    runtime_cache.insert(path, runtime.clone());
    Ok(runtime)
}
