/// Verifies that the version field on the agent-team-mail-core path dependency
/// in crates/atm/Cargo.toml matches the workspace version, and that Cargo.lock
/// records the same version for both crates.
///
/// crates.io rejects a publish if path deps lack a version field, and it must
/// match the published crate version. This test catches mismatches at CI time.
#[test]
fn atm_core_dep_version_matches_workspace_version() {
    let workspace_toml = include_str!("../../../Cargo.toml");
    let atm_toml = include_str!("../Cargo.toml");
    let cargo_lock = include_str!("../../../Cargo.lock");

    let workspace_version = workspace_toml
        .lines()
        .find(|l| l.starts_with("version = "))
        .and_then(|l| l.split('"').nth(1))
        .expect("workspace version not found in Cargo.toml");

    // Verify dep version field matches workspace version
    let dep_version = atm_toml
        .lines()
        .find(|l| l.contains("agent-team-mail-core") && l.contains("version"))
        .and_then(|l| l.split("version").nth(1)?.split('"').nth(1))
        .expect(
            "version field missing on agent-team-mail-core dep in crates/atm/Cargo.toml \
             — add version = \"x.y.z\" matching the workspace version",
        );

    assert_eq!(
        workspace_version, dep_version,
        "crates/atm/Cargo.toml agent-team-mail-core dep version ({dep_version}) \
         does not match workspace version ({workspace_version})"
    );

    // Verify Cargo.lock records the same version for agent-team-mail and agent-team-mail-core
    for crate_name in ["agent-team-mail", "agent-team-mail-core"] {
        let lock_version = cargo_lock
            .split("\n[[package]]")
            .find(|chunk| {
                chunk.contains(&format!("name = \"{crate_name}\""))
            })
            .and_then(|chunk| {
                chunk
                    .lines()
                    .find(|l| l.starts_with("version = "))
                    .and_then(|l| l.split('"').nth(1))
            })
            .unwrap_or_else(|| panic!("{crate_name} not found in Cargo.lock"));

        assert_eq!(
            workspace_version, lock_version,
            "Cargo.lock version for {crate_name} ({lock_version}) \
             does not match workspace version ({workspace_version}) — run `cargo generate-lockfile`"
        );
    }
}
