//! Keeps generation-scoped cron permits exclusive to the scheduler issuer.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use syn::visit::Visit;

const MANIFEST: &str = "boundaries/atm-core/cron-run-permit.toml";
const TRAIT: &str = "CronRunPermitIssuer";

#[derive(Deserialize)]
struct BoundaryManifest {
    issuer: Issuer,
}

#[derive(Deserialize)]
struct Issuer {
    allowed_impl_paths: BTreeSet<String>,
}

#[test]
fn only_the_scheduler_implements_the_permit_issuer_seal() {
    let root = root();
    let manifest: BoundaryManifest =
        toml::from_str(&read(&root.join(MANIFEST))).expect("cron permit boundary manifest parses");
    let actual = rust_sources(&root.join("crates"))
        .into_iter()
        .filter(|path| implements_trait(path))
        .map(|path| relative(&root, &path))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual, manifest.issuer.allowed_impl_paths,
        "CronRunPermitIssuer must remain implemented only by the authorized private scheduler record"
    );
}

struct ImplVisitor {
    found: bool,
}

impl<'ast> Visit<'ast> for ImplVisitor {
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

fn implements_trait(path: &Path) -> bool {
    let syntax = syn::parse_file(&read(path))
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
    let mut visitor = ImplVisitor { found: false };
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
        .replace('\\', "/")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}
