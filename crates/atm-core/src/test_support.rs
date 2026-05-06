#[cfg(any(test, feature = "test-utils"))]
use std::ffi::{OsStr, OsString};
#[cfg(any(test, feature = "test-utils"))]
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const TEST_TEAM: &str = "test-team";
pub const TEST_SENDER: &str = "sender-a";
pub const TEST_RECIPIENT: &str = "recipient";
pub const TEST_QA: &str = "qa-a";
pub const TEST_QA_AGENT: &str = TEST_QA;
pub use crate::roles::ROLE_TEAM_LEAD;
pub const TEST_LEAD: &str = "test-lead";
pub const TEST_DAEMON: &str = "daemon";
pub const TEST_ORIGIN: &str = "host-a";
pub const TEST_SENDER_ADDRESS: &str = "sender-a@test-team";
pub const TEST_RECIPIENT_ADDRESS: &str = "recipient@test-team";
pub const TEST_LEAD_ADDRESS: &str = "test-lead@test-team";

#[cfg(any(test, feature = "test-utils"))]
pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(any(test, feature = "test-utils"))]
pub struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
    _guard: MutexGuard<'static, ()>,
}

#[cfg(any(test, feature = "test-utils"))]
impl EnvGuard {
    pub fn set_raw(key: &'static str, value: &str) -> Self {
        let guard = env_lock().lock().expect("env lock");
        let original = std::env::var_os(key);
        set_env_var(key, value);
        Self {
            key,
            original,
            _guard: guard,
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => set_env_var(self.key, value),
            None => remove_env_var(self.key),
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub fn set_env_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    // SAFETY: test callers acquire the shared test env lock before mutating
    // the process environment.
    unsafe { std::env::set_var(key, value) }
}

#[cfg(any(test, feature = "test-utils"))]
pub fn remove_env_var<K: AsRef<OsStr>>(key: K) {
    // SAFETY: test callers acquire the shared test env lock before mutating
    // the process environment.
    unsafe { std::env::remove_var(key) }
}
