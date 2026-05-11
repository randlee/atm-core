use atm_core::error::AtmError;
use serial_test::serial;

use crate::{
    DaemonExitCode, daemon_exit_code_for_error, lifecycle_control::LifecycleControlSourceAdapter,
    test_support::LifecycleFlagResetGuard,
};

#[test]
fn daemon_shutdown_signals_for_test_are_isolated() {
    let first = LifecycleControlSourceAdapter::new_for_test();
    first.set_terminate_for_test(true);
    first.set_reload_for_test(true);
    let second = LifecycleControlSourceAdapter::new_for_test();

    assert!(!second.terminate_requested());
    assert!(!second.reload_requested_for_test());
}

#[test]
#[serial]
fn daemon_shutdown_signal_install_reuses_shared_flags() {
    let first = LifecycleControlSourceAdapter::install().expect("install first");
    let _reset = LifecycleFlagResetGuard::install(first.clone());

    let second = LifecycleControlSourceAdapter::install().expect("install second");
    first.set_reload_for_test(true);
    second.set_terminate_for_test(true);

    assert!(second.reload_requested_for_test());
    assert!(first.terminate_requested());
}

#[test]
fn daemon_exit_code_mapping_matches_supervisor_contract() {
    assert_eq!(
        daemon_exit_code_for_error(&AtmError::daemon_lifecycle_wedge("wedged")),
        DaemonExitCode::LifecycleWedge
    );
    assert_eq!(
        daemon_exit_code_for_error(&AtmError::daemon_launch_gate_rejected("launch gate")),
        DaemonExitCode::DoNotRestart
    );
    assert_eq!(
        daemon_exit_code_for_error(&AtmError::daemon_unavailable("listener died")),
        DaemonExitCode::TransportFatal
    );
    assert_eq!(
        daemon_exit_code_for_error(&AtmError::config("bad config")),
        DaemonExitCode::DoNotRestart
    );
    assert_eq!(
        daemon_exit_code_for_error(&AtmError::validation("buggy invariant")),
        DaemonExitCode::InternalBug
    );
}
