#[cfg(unix)]
use interprocess::local_socket::Stream as LocalSocketStream;
#[cfg(unix)]
use interprocess::local_socket::traits::Stream as _;

use crate::lifecycle_control::LifecycleControlSourceAdapter;

pub(crate) struct LifecycleFlagResetGuard {
    lifecycle: LifecycleControlSourceAdapter,
}

impl LifecycleFlagResetGuard {
    pub(crate) fn install(lifecycle: LifecycleControlSourceAdapter) -> Self {
        lifecycle.set_terminate_for_test(false);
        lifecycle.set_reload_for_test(false);
        Self { lifecycle }
    }
}

impl Drop for LifecycleFlagResetGuard {
    fn drop(&mut self) {
        self.lifecycle.set_terminate_for_test(false);
        self.lifecycle.set_reload_for_test(false);
    }
}

#[cfg(unix)]
pub(crate) fn connect_daemon_local_ipc_until_ready(
    endpoint_path: &std::path::Path,
) -> LocalSocketStream {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match LocalSocketStream::connect(
            atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path).expect("ipc name"),
        ) {
            Ok(stream) => return stream,
            Err(error) if std::time::Instant::now() < deadline => {
                let _ = error;
                std::thread::yield_now();
            }
            Err(error) => panic!("connect daemon local ipc: {error}"),
        }
    }
}
