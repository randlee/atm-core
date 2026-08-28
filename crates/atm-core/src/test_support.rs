#![cfg(any(test, feature = "test-utils"))]

#[cfg(any(test, feature = "test-utils"))]
use std::collections::HashMap;
#[cfg(any(test, feature = "test-utils"))]
use std::ffi::{OsStr, OsString};
#[cfg(any(test, feature = "test-utils"))]
use std::sync::OnceLock;

#[cfg(any(test, feature = "test-utils"))]
use parking_lot::{RwLock, RwLockReadGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};

#[cfg(any(test, feature = "test-utils"))]
use crate::atm_temp::EnvSource;

pub const TEST_TEAM: &str = "test-team";
pub const TEST_SENDER: &str = "sender-a";
pub const TEST_RECIPIENT: &str = "recipient";
pub const TEST_QA: &str = "qa-a";
pub const TEST_QA_AGENT: &str = TEST_QA;
pub use crate::roles::ROLE_TEAM_LEAD;
pub const TEST_ARCH_CTM: &str = "test-arch-member";
pub const TEST_LEAD: &str = "test-lead";
pub const TEST_DAEMON: &str = "daemon";
pub const TEST_ORIGIN: &str = "host-a";
pub const TEST_SENDER_ADDRESS: &str = "sender-a@test-team";
pub const TEST_RECIPIENT_ADDRESS: &str = "recipient@test-team";
pub const TEST_LEAD_ADDRESS: &str = "test-lead@test-team";

#[cfg(any(test, feature = "test-utils"))]
fn env_lock() -> &'static RwLock<()> {
    static LOCK: OnceLock<RwLock<()>> = OnceLock::new();
    LOCK.get_or_init(|| RwLock::new(()))
}

#[cfg(any(test, feature = "test-utils"))]
pub type EnvLockGuard = RwLockReadGuard<'static, ()>;

#[cfg(any(test, feature = "test-utils"))]
pub fn lock_env() -> EnvLockGuard {
    env_lock().read()
}

#[cfg(any(test, feature = "test-utils"))]
pub struct EnvGuard {
    restorations: Vec<EnvRestore>,
    _guard: Option<RwLockUpgradableReadGuard<'static, ()>>,
}

#[cfg(any(test, feature = "test-utils"))]
struct EnvRestore {
    key: &'static str,
    original: Option<OsString>,
}

#[cfg(any(test, feature = "test-utils"))]
impl EnvGuard {
    pub fn set_raw(key: &'static str, value: &str) -> Self {
        Self::set_many([(key, Some(value))])
    }

    pub fn unset_raw(key: &'static str) -> Self {
        Self::set_many([(key, None)])
    }

    pub fn set_many<const N: usize>(changes: [(&'static str, Option<&str>); N]) -> Self {
        let guard = env_lock().write();
        let restorations = changes
            .into_iter()
            .map(|(key, value)| {
                let original = std::env::var_os(key);
                match value {
                    Some(value) => set_env_var(key, value),
                    None => remove_env_var(key),
                }
                EnvRestore { key, original }
            })
            .collect();
        let guard = RwLockWriteGuard::downgrade_to_upgradable(guard);
        Self {
            restorations,
            _guard: Some(guard),
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Drop for EnvGuard {
    fn drop(&mut self) {
        let guard = self._guard.take().expect("env guard lock");
        let _guard = RwLockUpgradableReadGuard::upgrade(guard);
        for restore in self.restorations.iter_mut().rev() {
            match restore.original.take() {
                Some(value) => set_env_var(restore.key, value),
                None => remove_env_var(restore.key),
            }
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

/// A deterministic, in-memory [`EnvSource`] for tests that need to inject
/// environment values into production code that reads through the
/// `EnvSource` seam, without mutating the real (process-global) process
/// environment via [`EnvGuard`].
///
/// Prefer this over `EnvGuard` whenever the code under test accepts an
/// `&dyn EnvSource` parameter: it needs no shared lock, has no cross-test
/// interference risk, and does not require `std::env::var` callers
/// elsewhere in the process to cooperate with a guard.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Default, Clone)]
pub struct FakeEnvSource(HashMap<String, String>);

#[cfg(any(test, feature = "test-utils"))]
impl FakeEnvSource {
    /// An env source that reports every key as unset.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Builds a fake env source from `(key, value)` pairs. A `None` value
    /// leaves that key unset, matching [`EnvGuard::set_many`]'s shape so
    /// call sites can switch between the two with a minimal diff.
    #[must_use]
    pub fn new<'a, const N: usize>(vars: [(&'a str, Option<&'a str>); N]) -> Self {
        let mut map = HashMap::new();
        for (key, value) in vars {
            if let Some(value) = value {
                map.insert(key.to_string(), value.to_string());
            }
        }
        Self(map)
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl EnvSource for FakeEnvSource {
    fn var(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

/// Captures `tracing` events emitted by `f` **on the calling thread**, into
/// a `String` of one formatted line per event.
///
/// This deliberately does **not** use `tracing::subscriber::with_default`
/// (the otherwise-obvious per-scope override): that mechanism triggers a
/// process-global callsite-interest-cache rebuild on every entry and exit,
/// and `cargo test`'s parallel harness runs many other tests concurrently
/// on other threads — including, for callsites this crate's own production
/// code shares across tests, non-capturing tests that exercise the very
/// same `tracing::warn!`/`tracing::info!` call site with no override active
/// at all. Two such rebuilds racing was observed in practice to
/// non-deterministically leave a callsite's cached interest at "never" at
/// the moment a capturing test's event fired, silently dropping the event
/// before any subscriber — including this one — was ever consulted.
///
/// Installing exactly one global subscriber for the whole test-binary
/// process (via `std::sync::Once`, the first time this function is called)
/// avoids that race structurally: there is only ever one subscriber, its
/// `enabled()` always returns `true`, and the interest cache converges once
/// and stays converged for the rest of the process — no further
/// installs/rebuilds ever compete with it. Each call reads only the
/// buffer keyed by the calling thread's `ThreadId`, so unrelated concurrent
/// callers (including nested/child capture calls on other threads) never
/// see each other's events.
#[cfg(any(test, feature = "test-utils"))]
pub fn capture_tracing<T>(f: impl FnOnce() -> T) -> (T, String) {
    tracing_capture::capture(f)
}

#[cfg(any(test, feature = "test-utils"))]
mod tracing_capture {
    use std::collections::HashMap;
    use std::fmt;
    use std::sync::{Mutex, OnceLock};
    use std::thread::ThreadId;

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    type Buffers = Mutex<HashMap<ThreadId, String>>;

    fn buffers() -> &'static Buffers {
        static BUFFERS: OnceLock<Buffers> = OnceLock::new();
        BUFFERS.get_or_init(Default::default)
    }

    #[derive(Default)]
    struct LineVisitor(String);

    impl Visit for LineVisitor {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.0.push(' ');
            self.0.push_str(field.name());
            self.0.push('=');
            self.0.push_str(&format!("{value:?}"));
        }
    }

    /// Zero-sized: every instance forwards to the same process-wide
    /// [`buffers`] static, so `tracing::Dispatch::new` (which takes
    /// ownership of the subscriber it wraps) can construct as many cheap
    /// copies as it needs without ever duplicating captured state.
    #[derive(Debug, Default, Clone, Copy)]
    struct GlobalCaptureProxy;

    impl Subscriber for GlobalCaptureProxy {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut visitor = LineVisitor::default();
            event.record(&mut visitor);
            let thread_id = std::thread::current().id();
            let mut buffers = buffers().lock().expect("capture buffers lock");
            let line = buffers.entry(thread_id).or_default();
            line.push_str(event.metadata().level().as_str());
            line.push_str(&visitor.0);
            line.push('\n');
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    fn ensure_installed() {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            // Ignored: if something else in this process already won the
            // race to install a global default, that default's `enabled()`
            // is what every thread sees regardless, and there is nothing
            // further this function can do about that.
            let _ = tracing::subscriber::set_global_default(GlobalCaptureProxy);
        });
    }

    pub(super) fn capture<T>(f: impl FnOnce() -> T) -> (T, String) {
        ensure_installed();
        let thread_id = std::thread::current().id();
        buffers()
            .lock()
            .expect("capture buffers lock")
            .remove(&thread_id);
        let result = f();
        let logs = buffers()
            .lock()
            .expect("capture buffers lock")
            .remove(&thread_id)
            .unwrap_or_default();
        (result, logs)
    }
}
