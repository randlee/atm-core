//! Mechanical allowlist for the committed cron-notice provenance projection.
//!
//! ADR-001 permits audited cross-crate adapter implementations of a workspace
//! convention seal. This guard parses every workspace Rust source file and
//! permits this trait only at the SQLite adapter seam and the shared approved
//! in-memory double; a new implementation or re-exported trait path fails.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cargo_metadata::{DependencyKind, MetadataCommand};
use serde::Deserialize;
use syn::visit::Visit;

const MANIFEST: &str = "boundaries/atm-storage/cron-transition-store.toml";
const TRAIT: &str = "CronCommittedNoticeRecord";
const SQLITE_IMPLEMENTATION: &str = "crates/atm-storage-rusqlite/src/cron_transition_store.rs";
const SQLITE_EXTERNAL_FIXTURE: &str =
    "crates/atm-storage-rusqlite/tests/cron_committed_notice_record_adapter.rs";
const DOUBLE_IMPLEMENTATION: &str = "crates/atm-storage/src/testing/cron_transition_store.rs";

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
fn manifest_declares_the_authorized_adapter_and_test_double() {
    let manifest: BoundaryManifest = toml::from_str(&read(&root().join(MANIFEST)))
        .expect("cron transition boundary manifest parses");
    assert!(
        manifest
            .dependencies
            .allowed_dependents
            .contains(&"atm-storage-rusqlite".to_owned()),
        "the SQLite adapter must remain the authorized external record implementation"
    );
    assert!(
        manifest.testing.allowed_test_double_paths.contains(
            &"crate::testing::cron_transition_store::InMemoryCronTransitionStore".to_owned()
        ),
        "the shared in-memory double path must remain documented"
    );
}

#[test]
fn only_authorized_sources_implement_the_committed_notice_record() {
    let root = root();
    let actual = rust_sources(&root.join("crates"))
        .into_iter()
        .filter(|path| implements_record(path))
        .map(|path| relative(&root, &path))
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        DOUBLE_IMPLEMENTATION.to_owned(),
        SQLITE_EXTERNAL_FIXTURE.to_owned(),
        SQLITE_IMPLEMENTATION.to_owned(),
    ]);

    assert_eq!(
        actual, expected,
        "{TRAIT} may be implemented only by its documented SQLite adapter seam and approved in-memory double"
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
struct RecordImplVisitor {
    found: bool,
}

impl<'ast> Visit<'ast> for RecordImplVisitor {
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

fn implements_record(path: &Path) -> bool {
    let source = read(path);
    let syntax = syn::parse_file(&source)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
    let mut visitor = RecordImplVisitor::default();
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

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn relative(root: &Path, path: &Path) -> String {
    normalize_path(
        path.strip_prefix(root)
            .expect("workspace source path")
            .to_string_lossy()
            .as_ref(),
    )
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[test]
fn normalize_path_uses_manifest_separators_on_windows_style_input() {
    assert_eq!(
        normalize_path(r"crates\atm-storage\src\testing\cron_transition_store.rs"),
        "crates/atm-storage/src/testing/cron_transition_store.rs"
    );
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("atm-architecture resides in the workspace")
        .to_path_buf()
}
