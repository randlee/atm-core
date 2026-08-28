//! Structural ownership guard for the AQ3 `MemberStateTransitionSink` seam.
//!
//! `boundaries/atm-http-runtime/member-state-transition-sink.toml` declares
//! that `atm-http-runtime` owns the sealed `MemberStateTransitionSink` trait
//! (`implementation.visibility = "trait_only"`: the owner crate never
//! implements it itself), that `atm-daemon-bootstrap` is the sole allowed
//! external implementer (`dependencies.allowed_dependents`), and that
//! `atm-http-runtime` must never gain a normal dependency on
//! `atm-daemon-bootstrap` or `atm-storage-rusqlite`
//! (`dependencies.forbidden_edges`). This file parses that manifest directly
//! and cross-checks each declaration against the real Cargo dependency graph
//! (`cargo_metadata`) and the real workspace source (`syn`), so a violation
//! -- a stray second implementer, or a forbidden dependency edge -- fails
//! `cargo test -p atm-architecture` mechanically instead of relying on
//! review alone.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use cargo_metadata::{DependencyKind, MetadataCommand};
use serde::Deserialize;
use syn::visit::Visit;

const MANIFEST_RELATIVE_PATH: &str =
    "boundaries/atm-http-runtime/member-state-transition-sink.toml";
const TRAIT_NAME: &str = "MemberStateTransitionSink";
const OWNER_CRATE: &str = "atm-http-runtime";

#[derive(Default, Deserialize)]
struct BoundaryToml {
    #[serde(default)]
    owner_package: String,
    #[serde(default)]
    dependencies: BoundaryDependencies,
}

#[derive(Default, Deserialize)]
struct BoundaryDependencies {
    #[serde(default)]
    allowed_dependents: Vec<String>,
    #[serde(default)]
    forbidden_edges: Vec<String>,
}

fn manifest() -> BoundaryToml {
    let path = workspace_root().join(MANIFEST_RELATIVE_PATH);
    let contents = read_source(&path);
    toml::from_str(&contents)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

#[test]
fn manifest_declares_the_expected_owner_and_at_least_one_forbidden_edge() {
    let manifest = manifest();
    assert_eq!(
        manifest.owner_package, OWNER_CRATE,
        "the seam manifest must keep declaring atm-http-runtime as its owner"
    );
    assert!(
        !manifest.dependencies.forbidden_edges.is_empty(),
        "the seam manifest must document at least one forbidden edge for this guard to exercise"
    );
    assert_eq!(
        manifest.dependencies.allowed_dependents,
        vec!["atm-daemon-bootstrap".to_string()],
        "atm-daemon-bootstrap must remain the sole documented external implementer"
    );
}

#[test]
fn manifest_forbidden_edges_are_absent_from_the_real_cargo_dependency_graph() {
    let manifest = manifest();
    let dependencies = direct_normal_workspace_dependencies();
    for edge in &manifest.dependencies.forbidden_edges {
        let (source, target) = edge.split_once(" -> ").unwrap_or_else(|| {
            panic!("invalid forbidden edge `{edge}` in {MANIFEST_RELATIVE_PATH}")
        });
        let actual = dependencies.get(source).cloned().unwrap_or_default();
        assert!(
            !actual.contains(target),
            "manifest forbidden edge `{edge}` is violated by the real Cargo dependency graph: \
             {source} actually has a normal dependency on {target} (actual deps: {actual:?})"
        );
    }
}

#[test]
fn only_the_manifest_declared_dependent_implements_the_sink_outside_its_owner_crate() {
    let manifest = manifest();
    let root = workspace_root();
    let package_roots = workspace_package_roots();

    let mut files = Vec::new();
    collect_rust_files(&root.join("crates"), &mut files);

    let mut implementers = BTreeSet::new();
    for path in &files {
        let source = read_source(path);
        let syntax = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
        let mut visitor = SinkImplVisitor::default();
        visitor.visit_file(&syntax);
        if visitor.found
            && let Some(crate_name) = owning_crate(&package_roots, path)
        {
            implementers.insert(crate_name);
        }
    }

    assert!(
        !implementers.contains(OWNER_CRATE),
        "{OWNER_CRATE} declares implementation.visibility = \"trait_only\" for {TRAIT_NAME}; \
         it must never implement its own sealed trait outside a #[cfg(test)] module"
    );

    let unexpected: Vec<_> = implementers
        .iter()
        .filter(|crate_name| {
            !manifest
                .dependencies
                .allowed_dependents
                .contains(crate_name)
        })
        .cloned()
        .collect();
    assert!(
        unexpected.is_empty(),
        "only {:?} may implement {TRAIT_NAME} outside {OWNER_CRATE}; found unexpected implementer(s): {unexpected:?}",
        manifest.dependencies.allowed_dependents
    );

    assert!(
        implementers.contains("atm-daemon-bootstrap"),
        "the manifest's sole allowed dependent, atm-daemon-bootstrap, must actually implement {TRAIT_NAME} \
         (status.state = \"concrete_landed\"); found implementers: {implementers:?}"
    );
}

#[derive(Default)]
struct SinkImplVisitor {
    found: bool,
    in_test_module: bool,
}

impl<'ast> Visit<'ast> for SinkImplVisitor {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let previous = self.in_test_module;
        self.in_test_module = previous || node.attrs.iter().any(is_cfg_test_attribute);
        syn::visit::visit_item_mod(self, node);
        self.in_test_module = previous;
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if !self.in_test_module
            && node.trait_.as_ref().is_some_and(|(_, path, _)| {
                path.segments
                    .last()
                    .is_some_and(|segment| segment.ident == TRAIT_NAME)
            })
        {
            self.found = true;
        }
        syn::visit::visit_item_impl(self, node);
    }
}

fn is_cfg_test_attribute(attribute: &syn::Attribute) -> bool {
    attribute.path().is_ident("cfg")
        && attribute
            .parse_args::<syn::Ident>()
            .is_ok_and(|ident| ident == "test")
}

/// Maps each workspace member's crate-root directory to its Cargo package
/// name, derived from `cargo_metadata` rather than guessed from directory
/// naming (which is unreliable here: for example the `crates/atm-core`
/// directory's real package name is `agent-team-mail-core`).
fn workspace_package_roots() -> Vec<(PathBuf, String)> {
    let metadata = MetadataCommand::new()
        .manifest_path(workspace_root().join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata must succeed for the workspace");
    metadata
        .packages
        .into_iter()
        .map(|package| {
            let manifest_path = PathBuf::from(package.manifest_path.as_str());
            let crate_root = manifest_path
                .parent()
                .expect("package manifest must have a parent directory")
                .to_path_buf();
            (crate_root, package.name.to_string())
        })
        .collect()
}

fn owning_crate(package_roots: &[(PathBuf, String)], file: &Path) -> Option<String> {
    package_roots
        .iter()
        .filter(|(root, _)| file.starts_with(root))
        .max_by_key(|(root, _)| root.components().count())
        .map(|(_, name)| name.clone())
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

fn read_source(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("atm-architecture must live under crates/atm-architecture")
        .to_path_buf()
}
