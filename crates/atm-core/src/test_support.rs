#![allow(dead_code)]

#[cfg(test)]
use std::ffi::{OsStr, OsString};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

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

#[cfg(test)]
pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

#[cfg(test)]
impl EnvGuard {
    pub fn set_raw(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        set_env_var(key, value);
        Self { key, original }
    }
}

#[cfg(test)]
impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => set_env_var(self.key, value),
            None => remove_env_var(self.key),
        }
    }
}

#[cfg(test)]
pub fn set_env_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    // SAFETY: test callers acquire the shared test env lock before mutating
    // the process environment.
    unsafe { std::env::set_var(key, value) }
}

#[cfg(test)]
pub fn remove_env_var<K: AsRef<OsStr>>(key: K) {
    // SAFETY: test callers acquire the shared test env lock before mutating
    // the process environment.
    unsafe { std::env::remove_var(key) }
}
