// ARCHITECTURE ENFORCEMENT CRATE
// Every assertion in this file is a merge gate.
// Removing, weakening, or commenting out any assertion requires an explicit
// architecture decision recorded in docs/architecture.md.
// QA MUST FAIL any PR that loosens a boundary assertion without that record.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs,
    path::{Path, PathBuf},
};

use cargo_metadata::{DependencyKind, MetadataCommand};
use regex::Regex;
use serde::Deserialize;

const ACTIVE_SPRINT_ENV: &str = "ATM_ARCH_ACTIVE_SPRINT";
const DIFF_BASE_ENV: &str = "ATM_ARCH_DIFF_BASE";
const DIFF_HEAD_ENV: &str = "ATM_ARCH_DIFF_HEAD";
const ALLOW_GATE_EDITS_ENV: &str = "ATM_ARCH_ALLOW_GATE_CHANGES";

const EXPECTED_FORBIDDEN_EDGES: &[(&str, &str)] = &[
    ("atm", "atm-storage-rusqlite"),
    ("atm-daemon", "atm-storage-rusqlite"),
    ("atm-graft", "atm-daemon"),
    ("atm-graft", "atm-daemon-bootstrap"),
    ("atm-graft", "atm-storage-rusqlite"),
    ("atm-runtime", "atm-daemon"),
];

#[test]
fn atm_daemon_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm-daemon", "atm-storage-rusqlite");
}

#[test]
fn atm_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm", "atm-storage-rusqlite");
}

#[test]
fn atm_runtime_must_not_depend_on_atm_daemon() {
    assert_forbidden_edge_absent("atm-runtime", "atm-daemon");
}

#[test]
fn atm_graft_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm-graft", "atm-storage-rusqlite");
}

#[test]
fn boundary_toml_forbidden_edges_match_rust_guard_catalog() {
    let expected = expected_edge_set();
    let documented = documented_forbidden_edges();
    let missing = missing_forbidden_edges(&expected, &documented);
    let unexpected = missing_forbidden_edges(&documented, &expected);
    assert!(
        missing.is_empty() && unexpected.is_empty(),
        "boundary TOML forbidden_edges drifted from the Rust architecture guard; missing: {missing:?}; unexpected: {unexpected:?}"
    );
}

#[test]
fn daemon_boundary_tomls_must_not_allow_atm_storage_rusqlite() {
    let violations = daemon_boundary_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            boundary
                .dependencies
                .allowed_dependencies
                .contains(&"atm-storage-rusqlite".to_string())
                .then(|| path.display().to_string())
        })
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "daemon boundary TOMLs must not allow atm-storage-rusqlite directly; violating files: {violations:?}"
    );
}

#[test]
fn synthetic_daemon_boundary_relaxation_fixture_is_detected() {
    let fixture = BoundaryToml {
        dependencies: BoundaryDependencies {
            allowed_dependencies: vec![
                "atm-storage".to_string(),
                "atm-storage-rusqlite".to_string(),
            ],
            ..BoundaryDependencies::default()
        },
    };

    assert!(
        fixture
            .dependencies
            .allowed_dependencies
            .contains(&"atm-storage-rusqlite".to_string()),
        "synthetic fixture must demonstrate the daemon TOML relock would fail closed if atm-storage-rusqlite were re-added"
    );
}

#[test]
fn synthetic_boundary_relaxation_fixture_reports_removed_forbidden_edge() {
    // Synthetic fixture proving the guard will fail closed if a forbidden edge
    // is relaxed out of the Rust catalog or the boundary TOMLs.
    let mut relaxed = expected_edge_set();
    relaxed.remove(&("atm-daemon".to_string(), "atm-storage-rusqlite".to_string()));

    let missing = missing_forbidden_edges(&expected_edge_set(), &relaxed);
    assert_eq!(
        missing,
        vec!["atm-daemon -> atm-storage-rusqlite".to_string()],
        "synthetic relaxation fixture must demonstrate the removed daemon/sqlite edge is detected"
    );
}

#[test]
fn ag_delete_lists_must_have_no_forbidden_symbols_or_workaround_paths() {
    let violations = sprint_delete_manifests()
        .into_iter()
        .flat_map(|manifest| scan_delete_manifest(&manifest))
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "AG delete-list enforcement failed:\n{}",
        violations.join("\n")
    );
}

#[test]
fn active_sprint_diff_gate_must_hold_when_configured() {
    let Some(sprint) = env::var_os(ACTIVE_SPRINT_ENV).map(|value| value.to_string_lossy().into_owned())
    else {
        return;
    };

    let base = env::var(DIFF_BASE_ENV).unwrap_or_else(|_| {
        panic!(
            "{ACTIVE_SPRINT_ENV} is set to {sprint}, but {DIFF_BASE_ENV} is missing"
        )
    });
    let head = env::var(DIFF_HEAD_ENV).unwrap_or_else(|_| {
        panic!(
            "{ACTIVE_SPRINT_ENV} is set to {sprint}, but {DIFF_HEAD_ENV} is missing"
        )
    });

    let manifest = sprint_delete_manifests()
        .into_iter()
        .find(|manifest| manifest.sprint == sprint)
        .unwrap_or_else(|| panic!("no delete-list manifest found for active sprint {sprint}"));

    let changed_files = git_diff_name_only(&base, &head);
    let numstat = git_diff_numstat(&base, &head);
    let allow_gate_edits = env::var(ALLOW_GATE_EDITS_ENV)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let violations = validate_manifest_diff_gate(
        &manifest,
        &changed_files,
        &numstat,
        allow_gate_edits,
    );

    assert!(
        violations.is_empty(),
        "active sprint diff gate failed for {}:\n{}",
        manifest.sprint,
        violations.join("\n")
    );
}

fn assert_forbidden_edge_absent(source: &str, forbidden: &str) {
    let dependencies = direct_normal_workspace_dependencies();
    let actual = dependencies.get(source).cloned().unwrap_or_default();
    assert!(
        !actual.contains(forbidden),
        "{source} must not have a normal workspace dependency on {forbidden}; actual workspace deps: {actual:?}"
    );
}
fn direct_normal_workspace_dependencies() -> BTreeMap<String, BTreeSet<String>> {
    let metadata = MetadataCommand::new()
        .manifest_path(workspace_root().join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata must succeed for the workspace");

    let workspace_package_names = metadata
        .packages
        .iter()
        .map(|package| package.name.to_string())
        .collect::<BTreeSet<_>>();

    metadata
        .packages
        .into_iter()
        .map(|package| {
            let deps = package
                .dependencies
                .into_iter()
                .filter(|dependency| dependency.kind == DependencyKind::Normal)
                .filter(|dependency| {
                    workspace_package_names.contains::<str>(dependency.name.as_ref())
                })
                .map(|dependency| dependency.name.to_string())
                .collect::<BTreeSet<_>>();
            (package.name.to_string(), deps)
        })
        .collect()
}

fn documented_forbidden_edges() -> BTreeSet<(String, String)> {
    guarded_boundary_files()
        .into_iter()
        .flat_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            boundary
                .dependencies
                .forbidden_edges
                .into_iter()
                .map(|edge| {
                    let (source, target) = edge.split_once(" -> ").unwrap_or_else(|| {
                        panic!("invalid forbidden edge `{edge}` in {}", path.display())
                    });
                    (source.to_string(), target.to_string())
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn expected_edge_set() -> BTreeSet<(String, String)> {
    EXPECTED_FORBIDDEN_EDGES
        .iter()
        .map(|(source, target)| (source.to_string(), target.to_string()))
        .collect()
}

fn missing_forbidden_edges(
    expected: &BTreeSet<(String, String)>,
    actual: &BTreeSet<(String, String)>,
) -> Vec<String> {
    expected
        .difference(actual)
        .map(|(source, target)| format!("{source} -> {target}"))
        .collect()
}

fn guarded_boundary_files() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = vec![
        root.join("boundaries/atm-graft/shared-client-consumer.toml"),
        root.join("boundaries/atm-runtime/runtime-composition.toml"),
    ];
    let mut sqlite_files = fs::read_dir(root.join("boundaries/atm-storage-rusqlite"))
        .expect("boundaries/atm-storage-rusqlite directory must be readable")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    sqlite_files.sort();
    files.extend(sqlite_files);
    files
}

fn daemon_boundary_files() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = fs::read_dir(root.join("boundaries/atm-daemon"))
        .expect("boundaries/atm-daemon directory must be readable")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn sprint_delete_manifests() -> Vec<SprintDeleteManifest> {
    let mut files = fs::read_dir(workspace_root().join("crates/atm-architecture/delete-lists"))
        .expect("delete-lists directory must be readable")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    files.sort();
    files.into_iter()
        .map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            toml::from_str::<SprintDeleteManifest>(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
        })
        .collect()
}

fn scan_delete_manifest(manifest: &SprintDeleteManifest) -> Vec<String> {
    manifest
        .scan_files
        .iter()
        .flat_map(|relative_path| scan_manifest_file(manifest, relative_path))
        .collect()
}

fn scan_manifest_file(manifest: &SprintDeleteManifest, relative_path: &str) -> Vec<String> {
    let path = workspace_root().join(relative_path);
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    let mut violations = Vec::new();

    for literal in &manifest.forbidden_literals {
        if contents.contains(literal) {
            violations.push(format!(
                "{} ({}): forbidden literal `{}` present in {}",
                manifest.sprint, manifest.description, literal, relative_path
            ));
        }
    }

    for pattern in &manifest.forbidden_regexes {
        let regex = Regex::new(pattern).unwrap_or_else(|error| {
            panic!(
                "invalid regex `{}` in sprint {} delete manifest: {error}",
                pattern, manifest.sprint
            )
        });
        if regex.is_match(&contents) {
            violations.push(format!(
                "{} ({}): forbidden regex `{}` matched in {}",
                manifest.sprint, manifest.description, pattern, relative_path
            ));
        }
    }

    violations
}

fn validate_manifest_diff_gate(
    manifest: &SprintDeleteManifest,
    changed_files: &[String],
    numstat: &[GitNumstatRow],
    allow_gate_edits: bool,
) -> Vec<String> {
    let allowed = manifest
        .allowed_changed_files
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let changed = changed_files.iter().cloned().collect::<BTreeSet<_>>();
    let mut violations = Vec::new();

    for path in &changed {
        if path.starts_with("crates/atm-architecture/") && !allow_gate_edits {
            violations.push(format!(
                "{}: forbidden gate edit `{}`; set {}=1 only for dedicated architecture-gate work",
                manifest.sprint, path, ALLOW_GATE_EDITS_ENV
            ));
        }

        if !allowed.contains(path) {
            violations.push(format!(
                "{}: changed file `{}` is outside the sprint allowlist {:?}",
                manifest.sprint, path, manifest.allowed_changed_files
            ));
        }
    }

    let net_crates_loc = numstat
        .iter()
        .filter(|row| row.path.starts_with("crates/"))
        .map(|row| row.additions as isize - row.deletions as isize)
        .sum::<isize>();

    if net_crates_loc > manifest.min_net_crates_loc {
        violations.push(format!(
            "{}: net crates/ LOC delta {} exceeds allowed threshold {}",
            manifest.sprint, net_crates_loc, manifest.min_net_crates_loc
        ));
    }

    violations
}

fn git_diff_name_only(base: &str, head: &str) -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("diff")
        .arg("--name-only")
        .arg(format!("{base}..{head}"))
        .current_dir(workspace_root())
        .output()
        .expect("git diff --name-only must execute");
    assert!(
        output.status.success(),
        "git diff --name-only {}..{} failed: {}",
        base,
        head,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git diff --name-only output must be utf-8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn git_diff_numstat(base: &str, head: &str) -> Vec<GitNumstatRow> {
    let output = std::process::Command::new("git")
        .arg("diff")
        .arg("--numstat")
        .arg(format!("{base}..{head}"))
        .current_dir(workspace_root())
        .output()
        .expect("git diff --numstat must execute");
    assert!(
        output.status.success(),
        "git diff --numstat {}..{} failed: {}",
        base,
        head,
        String::from_utf8_lossy(&output.stderr)
    );
    parse_numstat(
        &String::from_utf8(output.stdout).expect("git diff --numstat output must be utf-8"),
    )
}

fn parse_numstat(text: &str) -> Vec<GitNumstatRow> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_numstat_line)
        .collect()
}

fn parse_numstat_line(line: &str) -> GitNumstatRow {
    let mut parts = line.splitn(3, '\t');
    let additions = parts
        .next()
        .unwrap_or_else(|| panic!("invalid numstat line `{line}`"));
    let deletions = parts
        .next()
        .unwrap_or_else(|| panic!("invalid numstat line `{line}`"));
    let path = parts
        .next()
        .unwrap_or_else(|| panic!("invalid numstat line `{line}`"))
        .to_string();

    GitNumstatRow {
        additions: parse_numstat_count(additions),
        deletions: parse_numstat_count(deletions),
        path,
    }
}

fn parse_numstat_count(value: &str) -> usize {
    if value == "-" {
        0
    } else {
        value
            .parse::<usize>()
            .unwrap_or_else(|error| panic!("invalid numstat count `{value}`: {error}"))
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("atm-architecture must live under crates/atm-architecture")
        .to_path_buf()
}

#[derive(Deserialize)]
struct BoundaryToml {
    dependencies: BoundaryDependencies,
}

#[derive(Default, Deserialize)]
struct BoundaryDependencies {
    #[serde(default)]
    allowed_dependencies: Vec<String>,
    #[serde(default)]
    forbidden_edges: Vec<String>,
}

#[derive(Deserialize)]
struct SprintDeleteManifest {
    sprint: String,
    description: String,
    scan_files: Vec<String>,
    allowed_changed_files: Vec<String>,
    min_net_crates_loc: isize,
    #[serde(default)]
    forbidden_literals: Vec<String>,
    #[serde(default)]
    forbidden_regexes: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct GitNumstatRow {
    additions: usize,
    deletions: usize,
    path: String,
}

#[test]
fn parse_numstat_supports_text_and_binary_rows() {
    let rows = parse_numstat("12\t30\tcrates/atm-core/src/protocol.rs\n-\t-\tassets/logo.png\n");
    assert_eq!(
        rows,
        vec![
            GitNumstatRow {
                additions: 12,
                deletions: 30,
                path: "crates/atm-core/src/protocol.rs".to_string(),
            },
            GitNumstatRow {
                additions: 0,
                deletions: 0,
                path: "assets/logo.png".to_string(),
            },
        ]
    );
}

#[test]
fn diff_gate_rejects_out_of_scope_and_positive_loc_growth() {
    let manifest = SprintDeleteManifest {
        sprint: "AG.18".to_string(),
        description: "fixture".to_string(),
        scan_files: Vec::new(),
        allowed_changed_files: vec!["crates/atm-core/src/protocol.rs".to_string()],
        min_net_crates_loc: -100,
        forbidden_literals: Vec::new(),
        forbidden_regexes: Vec::new(),
    };

    let changed_files = vec![
        "crates/atm-core/src/protocol.rs".to_string(),
        "crates/atm/src/composition.rs".to_string(),
        "crates/atm-architecture/tests/boundary_enforcement.rs".to_string(),
    ];
    let numstat = vec![
        GitNumstatRow {
            additions: 50,
            deletions: 10,
            path: "crates/atm-core/src/protocol.rs".to_string(),
        },
        GitNumstatRow {
            additions: 20,
            deletions: 0,
            path: "crates/atm/src/composition.rs".to_string(),
        },
    ];

    let violations = validate_manifest_diff_gate(&manifest, &changed_files, &numstat, false);
    assert!(violations.iter().any(|v| v.contains("outside the sprint allowlist")));
    assert!(violations.iter().any(|v| v.contains("forbidden gate edit")));
    assert!(violations.iter().any(|v| v.contains("net crates/ LOC delta")));
}

#[test]
fn diff_gate_accepts_negative_loc_within_allowlist() {
    let manifest = SprintDeleteManifest {
        sprint: "AG.19".to_string(),
        description: "fixture".to_string(),
        scan_files: Vec::new(),
        allowed_changed_files: vec![
            "crates/atm-core/src/ack/mod.rs".to_string(),
            "crates/atm/src/composition.rs".to_string(),
        ],
        min_net_crates_loc: -100,
        forbidden_literals: Vec::new(),
        forbidden_regexes: Vec::new(),
    };

    let changed_files = vec![
        "crates/atm-core/src/ack/mod.rs".to_string(),
        "crates/atm/src/composition.rs".to_string(),
    ];
    let numstat = vec![
        GitNumstatRow {
            additions: 5,
            deletions: 90,
            path: "crates/atm-core/src/ack/mod.rs".to_string(),
        },
        GitNumstatRow {
            additions: 0,
            deletions: 25,
            path: "crates/atm/src/composition.rs".to_string(),
        },
    ];

    let violations = validate_manifest_diff_gate(&manifest, &changed_files, &numstat, false);
    assert!(violations.is_empty(), "expected no violations, got {violations:?}");
}
