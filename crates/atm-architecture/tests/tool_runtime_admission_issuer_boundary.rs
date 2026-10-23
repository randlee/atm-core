//! Mechanical allowlist for virtual-tool runtime-admission issuance.
//!
//! ADR-001 permits audited cross-crate adapter implementations of a workspace
//! convention seal.  A resolved tool sender is inspection data, so only the
//! documented registry adapter seam may provide the record storage turns into
//! an opaque `ToolRuntimeAdmission`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cargo_metadata::{DependencyKind, MetadataCommand};
use serde::Deserialize;
use syn::visit::Visit;

const MANIFEST: &str = "boundaries/atm-storage/tool-sender-registry.toml";
const TRAIT: &str = "ToolRuntimeAdmissionIssuer";
const DOUBLE_IMPLEMENTATION: &str = "crates/atm-storage/src/testing/tool_sender_registry.rs";

#[derive(Deserialize)]
struct BoundaryManifest {
    dependencies: Dependencies,
    testing: Testing,
}

#[derive(Deserialize)]
struct Dependencies {
    allowed_dependents: Vec<String>,
}

#[derive(Deserialize)]
struct Testing {
    allowed_test_double_paths: Vec<String>,
}

#[test]
fn manifest_declares_the_future_sqlite_adapter_and_current_test_double() {
    let manifest: BoundaryManifest = toml::from_str(&read(&root().join(MANIFEST)))
        .expect("tool sender boundary manifest parses");
    assert!(
        manifest
            .dependencies
            .allowed_dependents
            .contains(&"atm-storage-rusqlite".to_owned()),
        "the SQLite registry adapter must remain the only authorized production issuer"
    );
    assert!(
        manifest.testing.allowed_test_double_paths.contains(
            &"crate::testing::tool_sender_registry::InMemoryToolRuntimeAdmissionIssuer".to_owned()
        ),
        "the deterministic in-memory issuer must remain documented"
    );
}

#[test]
fn only_the_documented_issuer_implements_the_seam_before_sqlite_lands() {
    let root = root();
    let actual = rust_sources(&root.join("crates"))
        .into_iter()
        .filter(|path| implements_issuer(path))
        .map(|path| relative(&root, &path))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual,
        BTreeSet::from([DOUBLE_IMPLEMENTATION.to_owned()]),
        "a resolve result cannot gain an admission issuer outside the documented adapter seam"
    );
}

#[test]
fn sqlite_adapter_depends_on_storage_without_reversing_the_edge() {
    let dependencies = normal_workspace_dependencies();
    assert!(
        dependencies
            .get("atm-storage-rusqlite")
            .is_some_and(|deps| deps.contains("atm-storage")),
        "the authorized SQLite adapter must compile against the storage contract"
    );
    assert!(
        !dependencies
            .get("atm-storage")
            .is_some_and(|deps| deps.contains("atm-storage-rusqlite")),
        "the storage contract must not depend on its SQLite adapter"
    );
}

#[derive(Default)]
struct IssuerImplVisitor {
    found: bool,
}

impl<'ast> Visit<'ast> for IssuerImplVisitor {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if item.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == TRAIT)
        }) {
            self.found = true;
        }
        syn::visit::visit_item_impl(self, item);
    }
}

fn implements_issuer(path: &Path) -> bool {
    let syntax = syn::parse_file(&read(path))
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
    let mut visitor = IssuerImplVisitor::default();
    visitor.visit_file(&syntax);
    visitor.found
}

fn rust_sources(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(directory, &mut files);
    files
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
    {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn normal_workspace_dependencies() -> std::collections::BTreeMap<String, BTreeSet<String>> {
    let metadata = MetadataCommand::new()
        .manifest_path(root().join("Cargo.toml"))
        .no_deps()
        .exec()
        .expect("cargo metadata succeeds");
    let names = metadata
        .packages
        .iter()
        .map(|package| package.name.to_string())
        .collect::<BTreeSet<_>>();
    metadata
        .packages
        .into_iter()
        .map(|package| {
            let dependencies = package
                .dependencies
                .into_iter()
                .filter(|dependency| dependency.kind == DependencyKind::Normal)
                .filter(|dependency| names.contains::<str>(dependency.name.as_ref()))
                .map(|dependency| dependency.name.to_string())
                .collect();
            (package.name.to_string(), dependencies)
        })
        .collect()
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("path below workspace root")
        .to_string_lossy()
        .into_owned()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}
