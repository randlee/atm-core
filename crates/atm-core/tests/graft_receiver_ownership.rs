use atm_core::graft::{GraftReceiverListener, graft_receiver_record_path_from_home};
use atm_core::types::{AgentName, TeamName};

const CHILD_ROOT: &str = "ATM_GRAFT_RECLAIM_CHILD_ROOT";
const TEST_AGENT: &str = "qa-a";
const TEST_TEAM: &str = "test-team";

fn receiver_record_path_from_test_root() -> std::path::PathBuf {
    let team = TeamName::from_validated(TEST_TEAM);
    let agent = AgentName::from_validated(TEST_AGENT);
    let root = std::env::var_os(CHILD_ROOT).expect("cross-process fixture root");
    graft_receiver_record_path_from_home(std::path::Path::new(&root), &team, &agent)
}

/// Executed by the Python graft smoke as the crash child. `forget` prevents
/// `Drop` cleanup so the next process proves OS lock reclamation on exit.
#[test]
#[ignore = "orchestrated by scripts/test_atm_graft_python.py"]
fn child_owner_exits_without_drop() {
    let listener = GraftReceiverListener::bind(&receiver_record_path_from_test_root(), None)
        .expect("child owner");
    std::mem::forget(listener);
}

/// Executed immediately after the crash child by the same cross-process smoke.
#[test]
#[ignore = "orchestrated by scripts/test_atm_graft_python.py"]
fn parent_reclaims_child_owner_lock() {
    let record_path = receiver_record_path_from_test_root();
    drop(
        GraftReceiverListener::bind(&record_path, None)
            .expect("parent reclaims OS-released child ownership lock"),
    );
}
