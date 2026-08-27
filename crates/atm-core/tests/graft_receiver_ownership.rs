use atm_core::graft::GraftReceiverListener;
use atm_core::types::{AgentName, TeamName};

const CHILD_ROOT: &str = "ATM_GRAFT_RECLAIM_CHILD_ROOT";
const TEST_AGENT: &str = "qa-a";
const TEST_TEAM: &str = "test-team";

fn receiver_root_from_test_root() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os(CHILD_ROOT).expect("cross-process fixture root"))
}

/// Executed by the Python graft smoke as the crash child. `forget` prevents
/// `Drop` cleanup so the next process proves OS lock reclamation on exit.
#[test]
#[ignore = "orchestrated by scripts/test_atm_graft_python.py"]
fn child_owner_exits_without_drop() {
    let team = TeamName::from_validated(TEST_TEAM);
    let agent = AgentName::from_validated(TEST_AGENT);
    let listener =
        GraftReceiverListener::bind(&receiver_root_from_test_root(), &team, &agent, None)
            .expect("child owner");
    std::mem::forget(listener);
}

/// Executed immediately after the crash child by the same cross-process smoke.
#[test]
#[ignore = "orchestrated by scripts/test_atm_graft_python.py"]
fn parent_reclaims_child_owner_lock() {
    let team = TeamName::from_validated(TEST_TEAM);
    let agent = AgentName::from_validated(TEST_AGENT);
    drop(
        GraftReceiverListener::bind(&receiver_root_from_test_root(), &team, &agent, None)
            .expect("parent reclaims OS-released child ownership lock"),
    );
}
