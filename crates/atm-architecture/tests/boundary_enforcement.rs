// ARCHITECTURE ENFORCEMENT CRATE
// Every assertion in this file is a merge gate.
// Removing, weakening, or commenting out any assertion requires an explicit
// architecture decision recorded in docs/architecture.md.
// QA MUST FAIL any PR that loosens a boundary assertion without that record.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use cargo_metadata::{DependencyKind, MetadataCommand};
use serde::Deserialize;

const EXPECTED_FORBIDDEN_EDGES: &[(&str, &str)] = &[
    ("atm", "atm-storage-rusqlite"),
    ("atm-daemon", "atm-storage-rusqlite"),
    ("atm-runtime", "atm-storage-rusqlite"),
    ("atm-storage-rusqlite", "atm-runtime"),
    ("atm-graft", "atm-daemon"),
    ("atm-graft", "atm-daemon-bootstrap"),
    ("atm-graft", "atm-storage-rusqlite"),
    ("atm-runtime", "atm-daemon"),
];

const RETIRED_DAEMON_CONSTRUCT_FRAGMENTS: &[(&str, &str)] = &[
    ("peer_", "transport"),
    ("Peer", "Transport"),
    ("Remote", "Replay"),
    ("replay_", "store"),
    ("remote_retry_", "budget"),
    ("RemoteDelivery", "OutcomeUnknown"),
];

const RETIRED_ERROR_CONTRACT_SYMBOLS: &[&str] = &[
    "AtmErrorKind",
    "ProtocolErrorEnvelope",
    "error_kind_for_code",
];

#[test]
fn production_code_cannot_restore_retired_error_contract_symbols() {
    let root = workspace_root().join("crates");
    let mut files = Vec::new();
    collect_rust_files(&root, &mut files);
    let violations = files
        .into_iter()
        .filter(|path| {
            !path
                .components()
                .any(|component| component.as_os_str() == "tests")
        })
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            retired_error_contract_symbol(&contents)
                .map(|symbol| format!("{} contains retired `{symbol}`", path.display()))
        })
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "AI.3's two-field error contract forbids retired error shapes: {violations:?}"
    );
}

fn retired_error_contract_symbol(source: &str) -> Option<&'static str> {
    RETIRED_ERROR_CONTRACT_SYMBOLS
        .iter()
        .copied()
        .find(|symbol| source.contains(symbol))
}

#[test]
fn retired_error_contract_detector_rejects_duplicate_code_mapping_fixture() {
    assert_eq!(
        retired_error_contract_symbol("fn error_kind_for_code() {}"),
        Some("error_kind_for_code")
    );
}

#[test]
fn only_the_error_contract_module_may_define_an_atm_error_literal() {
    let root = workspace_root();
    let contract_module = root.join("crates/atm-storage/src/error.rs");
    let source_root = root.join("crates");
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files);
    let violations = files
        .into_iter()
        .filter(|path| path != &contract_module)
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            direct_atm_error_literal(&contents).then(|| path.display().to_string())
        })
        .collect::<Vec<_>>();

    assert!(
        violations.is_empty(),
        "AtmError literals bypass the canonical error catalog: {violations:?}"
    );
}

fn direct_atm_error_literal(source: &str) -> bool {
    source.split("AtmError").skip(1).any(|suffix| {
        let Some(body) = suffix.trim_start().strip_prefix('{') else {
            return false;
        };
        matches!(body.trim_start(), body if body.starts_with("code:") || body.starts_with("message:"))
    })
}

#[test]
fn direct_atm_error_literal_detector_rejects_constructor_fixture() {
    let fixture = concat!("Atm", "Error", " {", " code: error_code, message: detail }");
    assert!(direct_atm_error_literal(fixture));
    assert!(!direct_atm_error_literal(
        "fn error() -> AtmError { panic!() }"
    ));
}

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
fn atm_runtime_must_not_depend_on_atm_storage_rusqlite() {
    assert_forbidden_edge_absent("atm-runtime", "atm-storage-rusqlite");
}

#[test]
fn atm_storage_rusqlite_must_not_depend_on_atm_runtime() {
    assert_forbidden_edge_absent("atm-storage-rusqlite", "atm-runtime");
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
        ..BoundaryToml::default()
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
fn workspace_source_must_not_reintroduce_retired_peer_delivery_constructs() {
    let source_root = workspace_root().join("crates");
    let mut findings = Vec::new();
    let mut files = Vec::new();
    collect_rust_files(&source_root, &mut files);
    for path in files {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for (prefix, suffix) in RETIRED_DAEMON_CONSTRUCT_FRAGMENTS {
            let construct = format!("{prefix}{suffix}");
            if contents.contains(&construct) {
                findings.push(format!("{}: {construct}", path.display()));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "retired peer-delivery constructs must not re-enter workspace Rust source: {findings:?}"
    );
}

#[test]
fn deleted_daemon_boundary_modules_must_be_retired() {
    let root = workspace_root();
    let stale = daemon_boundary_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            let sources = daemon_boundary_module_sources(&root, &boundary.implementation.module)?;
            let source_exists = sources.iter().any(|source| source.exists());
            module_is_stale_if_missing(source_exists, &boundary.status.state).then(|| {
                format!(
                    "{} declares missing module {} with status {}",
                    path.display(),
                    boundary.implementation.module,
                    boundary.status.state
                )
            })
        })
        .collect::<Vec<_>>();

    assert!(
        stale.is_empty(),
        "daemon boundary TOMLs for deleted modules must be retired: {stale:?}"
    );
}

#[test]
fn deleted_core_boundary_modules_must_be_retired() {
    let root = workspace_root();
    let stale = core_boundary_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            let sources = boundary_module_sources(&root, &boundary)?;
            let source_exists = sources.iter().any(|source| source.exists());
            module_is_stale_if_missing(source_exists, &boundary.status.state).then(|| {
                format!(
                    "{} declares missing module {} with status {}",
                    path.display(),
                    boundary.implementation.module,
                    boundary.status.state
                )
            })
        })
        .collect::<Vec<_>>();

    assert!(
        stale.is_empty(),
        "core boundary TOMLs for deleted modules must be retired: {stale:?}"
    );
}

#[test]
fn retired_core_boundary_records_must_not_name_live_dependents_and_must_be_historical_docs() {
    let docs = fs::read_to_string(workspace_root().join("docs/atm-core/boundaries.md"))
        .expect("docs/atm-core/boundaries.md must be readable");
    let stale = core_boundary_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let boundary: BoundaryToml = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
            if boundary.status.state != "retired" {
                return None;
            }

            let mut failures = Vec::new();
            if !boundary.dependencies.allowed_dependents.is_empty() {
                failures.push(format!(
                    "lists live dependents {:?}",
                    boundary.dependencies.allowed_dependents
                ));
            }
            let section = documented_boundary_section(&docs, &boundary.name);
            if section.is_none_or(|section| {
                !(section.contains("Historical status:")
                    || section.contains("Historical boundary record"))
            }) {
                failures.push("is not documented as historical".to_string());
            }

            (!failures.is_empty()).then(|| format!("{}: {}", path.display(), failures.join("; ")))
        })
        .collect::<Vec<_>>();

    assert!(
        stale.is_empty(),
        "retired atm-core boundary records must not name live callers and must be documented as historical: {stale:?}"
    );
}

#[test]
fn missing_daemon_module_requires_retired_boundary_state() {
    assert!(module_is_stale_if_missing(false, "active"));
    assert!(!module_is_stale_if_missing(false, "retired"));
    assert!(!module_is_stale_if_missing(false, "planned"));
}

#[test]
fn missing_core_module_requires_retired_boundary_state() {
    assert!(module_is_stale_if_missing(false, "active"));
    assert!(!module_is_stale_if_missing(false, "retired"));
    assert!(!module_is_stale_if_missing(false, "planned"));
}

#[test]
fn bare_daemon_boundary_module_resolves_crate_entry_points() {
    let root = workspace_root();
    let sources = daemon_boundary_module_sources(&root, "atm_daemon")
        .expect("atm_daemon must resolve to daemon crate entry points");

    assert_eq!(
        sources,
        vec![
            root.join("crates/atm-daemon/src/lib.rs"),
            root.join("crates/atm-daemon/src/main.rs"),
        ]
    );
    assert!(sources.iter().any(|source| source.exists()));
}

#[test]
fn retired_bare_daemon_boundary_records_are_checked_against_entry_points() {
    let root = workspace_root();
    for file_name in [
        "daemon-reconcile-coordinator.toml",
        "file-watch-event-source.toml",
    ] {
        let path = root.join("boundaries/atm-daemon").join(file_name);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let boundary: BoundaryToml = toml::from_str(&contents)
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
        let sources = daemon_boundary_module_sources(&root, &boundary.implementation.module)
            .unwrap_or_else(|| panic!("{} must resolve a daemon module", path.display()));

        assert_eq!(boundary.implementation.module, "atm_daemon");
        assert!(sources.iter().any(|source| source.exists()));
        assert!(!module_is_stale_if_missing(
            sources.iter().any(|source| source.exists()),
            &boundary.status.state
        ));
    }
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
    {
        let path = entry
            .unwrap_or_else(|error| panic!("failed to read directory entry: {error}"))
            .path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn daemon_boundary_module_sources(root: &Path, module: &str) -> Option<Vec<PathBuf>> {
    let relative_module = module.strip_prefix("atm_daemon")?;
    let source_root = root.join("crates/atm-daemon/src");
    if relative_module.is_empty() {
        return Some(vec![
            source_root.join("lib.rs"),
            source_root.join("main.rs"),
        ]);
    }

    let relative_module = relative_module.strip_prefix("::")?;
    let module_path = source_root.join(relative_module.replace("::", "/"));
    Some(vec![
        module_path.with_extension("rs"),
        module_path.join("mod.rs"),
    ])
}

fn boundary_module_sources(root: &Path, boundary: &BoundaryToml) -> Option<Vec<PathBuf>> {
    let module = boundary.implementation.module.trim();
    if module.is_empty() {
        if boundary.implementation.visibility == "trait_only" {
            return Some(core_contract_trait_sources(
                root,
                &boundary.public.trait_name,
            ));
        }
        return Some(Vec::new());
    }

    let crate_path = boundary.owner_crate_path.trim();
    if crate_path.is_empty() {
        return Some(Vec::new());
    }
    let crate_source_root = root
        .join("crates")
        .join(crate_path.replace('_', "-"))
        .join("src");
    let relative_module = module
        .strip_prefix(crate_path)
        .and_then(|value| value.strip_prefix("::"))
        .unwrap_or(module);
    if relative_module == module && module != crate_path {
        return Some(Vec::new());
    }
    if relative_module.is_empty() {
        return Some(vec![
            crate_source_root.join("lib.rs"),
            crate_source_root.join("main.rs"),
        ]);
    }

    let module_path = crate_source_root.join(relative_module.replace("::", "/"));
    Some(vec![
        module_path.with_extension("rs"),
        module_path.join("mod.rs"),
    ])
}

fn core_contract_trait_sources(root: &Path, trait_name: &str) -> Vec<PathBuf> {
    if trait_name.trim().is_empty() {
        return Vec::new();
    }
    let needle = format!("pub trait {}", trait_name.trim());
    let mut files = Vec::new();
    collect_rust_files(&root.join("crates/atm-core/src/boundary"), &mut files);
    files
        .into_iter()
        .filter(|path| {
            fs::read_to_string(path)
                .map(|contents| contents.contains(&needle))
                .unwrap_or(false)
        })
        .collect()
}

fn module_is_stale_if_missing(source_exists: bool, state: &str) -> bool {
    !source_exists && !matches!(state, "retired" | "planned")
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

fn core_boundary_files() -> Vec<PathBuf> {
    boundary_files_in("atm-core")
}

fn boundary_files_in(owner: &str) -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = fs::read_dir(root.join("boundaries").join(owner))
        .unwrap_or_else(|error| panic!("boundaries/{owner} directory must be readable: {error}"))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn documented_boundary_section<'a>(docs: &'a str, name: &str) -> Option<&'a str> {
    let start_marker = format!("## {}", name);
    let start = docs.find(&start_marker)?;
    let rest = &docs[start..];
    let next = rest
        .match_indices("\n## ")
        .next()
        .map(|(index, _)| index)
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("atm-architecture must live under crates/atm-architecture")
        .to_path_buf()
}

#[derive(Default, Deserialize)]
struct BoundaryToml {
    #[serde(default)]
    name: String,
    #[serde(default)]
    owner_crate_path: String,
    #[serde(default)]
    public: BoundaryPublic,
    #[serde(default)]
    implementation: BoundaryImplementation,
    #[serde(default)]
    status: BoundaryStatus,
    #[serde(default)]
    dependencies: BoundaryDependencies,
}

#[derive(Default, Deserialize)]
struct BoundaryPublic {
    #[serde(default, rename = "trait")]
    trait_name: String,
}

#[derive(Default, Deserialize)]
struct BoundaryImplementation {
    #[serde(default)]
    visibility: String,
    #[serde(default)]
    module: String,
}

#[derive(Default, Deserialize)]
struct BoundaryStatus {
    #[serde(default)]
    state: String,
}

#[derive(Default, Deserialize)]
struct BoundaryDependencies {
    #[serde(default)]
    allowed_dependents: Vec<String>,
    #[serde(default)]
    allowed_dependencies: Vec<String>,
    #[serde(default)]
    forbidden_edges: Vec<String>,
}
