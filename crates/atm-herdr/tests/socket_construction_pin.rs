//! Keep AY.8's socket transport out of the production composition root.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};

fn rust_sources(root: &Path, output: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(root).expect("source directory");
    for entry in entries {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            rust_sources(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}

fn cfg_test_attribute(attribute: &syn::Attribute) -> bool {
    attribute.path().is_ident("cfg")
        && attribute
            .meta
            .require_list()
            .is_ok_and(|list| list.tokens.to_string().contains("test"))
}

fn collect_use_aliases(tree: &syn::UseTree, aliases: &mut BTreeSet<String>, herdr_path: bool) {
    match tree {
        syn::UseTree::Name(name) => {
            if herdr_path || name.ident == "HerdrIo" {
                aliases.insert(name.ident.to_string());
            }
        }
        syn::UseTree::Rename(rename) => {
            if herdr_path || rename.ident == "HerdrIo" {
                aliases.insert(rename.rename.to_string());
            }
        }
        syn::UseTree::Path(path) => {
            collect_use_aliases(&path.tree, aliases, herdr_path || path.ident == "HerdrIo");
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_aliases(tree, aliases, herdr_path);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

fn collect_aliases(file: &syn::File) -> BTreeSet<String> {
    let mut aliases = BTreeSet::from(["HerdrIo".to_owned()]);
    for item in &file.items {
        match item {
            syn::Item::Use(item) => collect_use_aliases(&item.tree, &mut aliases, false),
            syn::Item::Type(item) if matches!(&*item.ty, syn::Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "HerdrIo")) =>
            {
                aliases.insert(item.ident.to_string());
            }
            _ => {}
        }
    }
    aliases
}

struct SocketConstructorVisitor<'a> {
    aliases: &'a BTreeSet<String>,
    allow_construction: bool,
    permit_test_modules: bool,
    findings: Vec<String>,
}

impl<'ast> Visit<'ast> for SocketConstructorVisitor<'_> {
    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        let mut segments = expression.path.segments.iter().rev();
        let is_socket = segments
            .next()
            .is_some_and(|segment| segment.ident == "Socket");
        let owner = segments.next().map(|segment| segment.ident.to_string());
        if is_socket
            && owner.is_some_and(|owner| self.aliases.contains(&owner))
            && !self.allow_construction
        {
            self.findings.push("socket variant path".to_owned());
        }
        visit::visit_expr_path(self, expression);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let previous = self.allow_construction;
        self.allow_construction =
            previous || self.permit_test_modules && module.attrs.iter().any(cfg_test_attribute);
        if let Some((_, items)) = &module.content {
            for item in items {
                self.visit_item(item);
            }
        }
        self.allow_construction = previous;
    }
}

fn construction_findings(source: &str, permit_test_modules: bool) -> Vec<String> {
    let file = syn::parse_file(source).expect("Rust source must parse");
    let aliases = collect_aliases(&file);
    let mut visitor = SocketConstructorVisitor {
        aliases: &aliases,
        allow_construction: false,
        permit_test_modules,
        findings: Vec::new(),
    };
    visitor.visit_file(&file);
    visitor.findings
}

#[test]
fn socket_variant_constructed_only_in_tests() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    rust_sources(&manifest.join("src"), &mut sources);
    rust_sources(&manifest.join("tests"), &mut sources);

    for source in sources {
        let contents = fs::read_to_string(&source).expect("Rust source");
        let permits_test_only_module = source
            .file_name()
            .is_some_and(|name| name == "transport_socket.rs" || name == "lib.rs");
        let is_integration_test = source.starts_with(manifest.join("tests"));
        if is_integration_test {
            continue;
        }
        let findings = construction_findings(&contents, permits_test_only_module);
        assert!(
            findings.is_empty(),
            "socket construction escaped its AY.8 test scope in {}: {findings:?}",
            source.display()
        );
    }
}

#[test]
fn socket_transport_items_remain_crate_private() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/transport_socket.rs"))
            .expect("socket transport source");
    for item in [
        "pub struct HerdrHostEnv",
        "pub enum HerdrEndpoint",
        "pub struct SocketIo",
        "pub fn herdr_api_endpoint",
    ] {
        assert!(
            !source.contains(item),
            "AY.2 public-item pin widened: {item}"
        );
    }
    for item in [
        "pub(crate) struct HerdrHostEnv",
        "pub(crate) enum HerdrEndpoint",
        "pub(crate) struct SocketIo",
        "pub(crate) fn herdr_api_endpoint",
    ] {
        assert!(source.contains(item), "missing crate-private pin: {item}");
    }
}
