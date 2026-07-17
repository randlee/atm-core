use super::{
    DaemonRequestDispatcher, RuntimeStatusCache, install_retained_runtime_factory, test_team,
};
use atm_core::boundary::RequestDispatcher;
use atm_core::doctor::{DoctorQuery, DoctorSeverity, DoctorStatus};
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD};
use atm_core::types::IsoTimestamp;
use atm_runtime_test_support::{SQLITE_RUNTIME_PATH_ENV, open_sqlite_boundary};
use tempfile::TempDir;

use crate::tests::{install_test_roster, write_team_config};

#[test]
#[serial_test::serial(env)]
fn doctor_projects_cross_host_interface_and_allowlist_state_from_sqlite() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    let _env = EnvGuard::set_many([(
        SQLITE_RUNTIME_PATH_ENV,
        Some(db_path.to_str().expect("utf8 sqlite db path")),
    )]);

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    let interface_store = assembly.peer_interface_config_store.clone();
    let allowed_host_store = assembly.allowed_host_store.clone();
    interface_store
        .add_interface(
            atm_storage::AddPeerInterfaceCommand::new(
                "vpn0",
                "10.10.100.10".parse().expect("bind"),
                "10.10.100.10".parse().expect("advertise"),
                43101,
                atm_storage::PeerInterfaceKind::Vpn,
                "arch-ctm@atm-dev",
            )
            .expect("add interface"),
        )
        .expect("store interface");
    interface_store
        .set_interface_enabled(
            &atm_storage::PeerInterfaceKey::new(
                "vpn0",
                "10.10.100.10".parse().expect("bind"),
                43101,
            )
            .expect("key"),
            true,
        )
        .expect("enable interface");
    interface_store
        .record_binding_update(&atm_storage::PeerInterfaceBindingUpdate {
            key: atm_storage::PeerInterfaceKey::new(
                "vpn0",
                "10.10.100.10".parse().expect("bind"),
                43101,
            )
            .expect("key"),
            observed_at: Some(IsoTimestamp::now()),
            refresh_deadline_at: None,
            stale_at: None,
            last_bound_at: Some(IsoTimestamp::now()),
            last_bind_error: None,
        })
        .expect("binding update");
    allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "10.10.100.98",
                "arch-ctm@atm-dev",
                Some("windows".to_string()),
            )
            .expect("allow host"),
        )
        .expect("allow host");
    allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new("10.10.100.99", "arch-ctm@atm-dev", None)
                .expect("allow host 2"),
        )
        .expect("allow host 2");
    allowed_host_store
        .deny_host(
            &"10.10.100.99"
                .parse::<atm_storage::AllowedHostName>()
                .expect("host"),
        )
        .expect("deny host");

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    let doctor = dispatcher
        .dispatch(
            RequestEnvelope::Doctor(DoctorQuery {
                home_dir: atm_home.clone(),
                current_dir: atm_home,
                team_override: Some(test_team().clone()),
                ..DoctorQuery::default()
            }),
            None,
        )
        .expect("doctor response");

    match doctor {
        ResponseEnvelope::Doctor(report) => {
            if std::env::var_os("ATM_EMIT_DOCTOR_FIXTURE").is_some() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serialize doctor report")
                );
            }
            let cross_host = report.cross_host.expect("cross host");
            assert!(!cross_host.legacy_fallback_active);
            assert_eq!(cross_host.bound_endpoints, vec!["10.10.100.10:43101"]);
            assert_eq!(cross_host.interfaces.len(), 1);
            assert_eq!(cross_host.interfaces[0].interface_name, "vpn0");
            assert!(cross_host.interfaces[0].listener_bound);
            assert!(cross_host.allowlist.enforced);
            assert!(!cross_host.allowlist.empty);
            assert_eq!(cross_host.allowlist.hosts.len(), 2);
            assert!(
                cross_host
                    .allowlist
                    .hosts
                    .iter()
                    .any(|row| row.host_name == "10.10.100.98" && row.enabled)
            );
            assert!(
                cross_host
                    .allowlist
                    .hosts
                    .iter()
                    .any(|row| row.host_name == "10.10.100.99" && !row.enabled)
            );
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn doctor_stays_healthy_when_cross_host_is_unconfigured_and_unused() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    let _env = EnvGuard::set_many([(
        SQLITE_RUNTIME_PATH_ENV,
        Some(db_path.to_str().expect("utf8 sqlite db path")),
    )]);

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    let doctor = dispatcher
        .dispatch(
            RequestEnvelope::Doctor(DoctorQuery {
                home_dir: atm_home.clone(),
                current_dir: atm_home,
                team_override: Some(test_team().clone()),
                ..DoctorQuery::default()
            }),
            None,
        )
        .expect("doctor response");

    match doctor {
        ResponseEnvelope::Doctor(report) => {
            let cross_host = report.cross_host.expect("cross host");
            assert_eq!(report.summary.status, DoctorStatus::Healthy);
            assert!(cross_host.interfaces.is_empty());
            assert!(cross_host.bound_endpoints.is_empty());
            assert!(cross_host.allowlist.empty);
            assert!(report.findings.iter().any(|finding| {
                finding.code == AtmErrorCode::WarningCrossHostListenerUnconfigured
                    && finding.severity == DoctorSeverity::Info
            }));
            assert!(report.findings.iter().any(|finding| {
                finding.code == AtmErrorCode::WarningCrossHostAllowlistEmpty
                    && finding.severity == DoctorSeverity::Info
            }));
            assert!(
                report
                    .recommendations
                    .iter()
                    .any(|recommendation| { recommendation.contains("atm daemon interfaces add") })
            );
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn doctor_surfaces_degraded_cross_host_bind_state_and_staleness() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    let _env = EnvGuard::set_many([(
        SQLITE_RUNTIME_PATH_ENV,
        Some(db_path.to_str().expect("utf8 sqlite db path")),
    )]);

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    let interface_store = assembly.peer_interface_config_store.clone();
    let allowed_host_store = assembly.allowed_host_store.clone();
    interface_store
        .add_interface(
            atm_storage::AddPeerInterfaceCommand::new(
                "en0",
                "192.168.1.20".parse().expect("bind"),
                "192.168.1.20".parse().expect("advertise"),
                43101,
                atm_storage::PeerInterfaceKind::Lan,
                "arch-ctm@atm-dev",
            )
            .expect("add interface"),
        )
        .expect("store interface");
    interface_store
        .set_interface_enabled(
            &atm_storage::PeerInterfaceKey::new(
                "en0",
                "192.168.1.20".parse().expect("bind"),
                43101,
            )
            .expect("key"),
            true,
        )
        .expect("enable interface");
    interface_store
        .record_binding_update(&atm_storage::PeerInterfaceBindingUpdate {
            key: atm_storage::PeerInterfaceKey::new(
                "en0",
                "192.168.1.20".parse().expect("bind"),
                43101,
            )
            .expect("key"),
            observed_at: None,
            refresh_deadline_at: None,
            stale_at: Some(IsoTimestamp::now()),
            last_bound_at: None,
            last_bind_error: Some("address already in use".to_string()),
        })
        .expect("binding update");
    allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new("192.168.1.21", "arch-ctm@atm-dev", None)
                .expect("allow host"),
        )
        .expect("allow host");

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    let doctor = dispatcher
        .dispatch(
            RequestEnvelope::Doctor(DoctorQuery {
                home_dir: atm_home.clone(),
                current_dir: atm_home,
                team_override: Some(test_team().clone()),
                ..DoctorQuery::default()
            }),
            None,
        )
        .expect("doctor response");

    match doctor {
        ResponseEnvelope::Doctor(report) => {
            let cross_host = report.cross_host.expect("cross host");
            assert_eq!(cross_host.interfaces.len(), 1);
            assert!(!cross_host.interfaces[0].listener_bound);
            assert_eq!(
                cross_host.interfaces[0].last_bind_error.as_deref(),
                Some("address already in use")
            );
            assert!(cross_host.interfaces[0].stale_at.is_some());
            assert!(
                report.findings.iter().any(|finding| {
                    finding.code == AtmErrorCode::WarningCrossHostListenerDegraded
                })
            );
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}
