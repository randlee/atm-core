use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

fn test_only(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute
                .parse_args::<syn::Meta>()
                .is_ok_and(|meta| configuration_requires_test(&meta))
    })
}

fn configuration_requires_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|requirements| requirements.iter().any(configuration_requires_test)),
        _ => false,
    }
}

fn files(directory: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).expect("source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() && path.file_name().is_some_and(|name| name != "tests") {
            paths.extend(files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

struct TestModules {
    directory: PathBuf,
    excluded: BTreeSet<PathBuf>,
}

impl<'ast> Visit<'ast> for TestModules {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let directory = self.directory.join(item.ident.to_string());
        if test_only(&item.attrs) {
            if item.content.is_none() {
                self.excluded.insert(directory.with_extension("rs"));
                self.excluded.insert(directory);
            }
            return;
        }
        let previous = std::mem::replace(&mut self.directory, directory);
        syn::visit::visit_item_mod(self, item);
        self.directory = previous;
    }
}

#[derive(Default)]
struct SpawnSites {
    aliases: BTreeMap<String, String>,
    kinds: BTreeSet<String>,
}

impl SpawnSites {
    fn import(&mut self, prefix: Vec<String>, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(path) => {
                let mut prefix = prefix;
                prefix.push(path.ident.to_string());
                self.import(prefix, &path.tree);
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.import(prefix.clone(), item);
                }
            }
            syn::UseTree::Name(name) => {
                self.record_alias(prefix, name.ident.to_string(), name.ident.to_string())
            }
            syn::UseTree::Rename(rename) => {
                self.record_alias(prefix, rename.ident.to_string(), rename.rename.to_string())
            }
            syn::UseTree::Glob(_) => {}
        }
    }

    fn record_alias(&mut self, prefix: Vec<String>, name: String, alias: String) {
        if name == "Command"
            && prefix.len() == 2
            && prefix[1] == "process"
            && matches!(prefix[0].as_str(), "std" | "tokio")
        {
            self.aliases.insert(alias, prefix[0].clone());
        }
    }

    fn scan(&mut self, file: &syn::File) {
        self.visit_file(file);
        self.visit_file(file);
    }
}

impl<'ast> Visit<'ast> for SpawnSites {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_fn(self, item);
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_impl(self, item);
        }
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_trait(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !test_only(&item.attrs) {
            syn::visit::visit_impl_item_fn(self, item);
        }
    }

    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        if !test_only(&item.attrs) {
            syn::visit::visit_impl_item_const(self, item);
        }
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_const(self, item);
        }
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_static(self, item);
        }
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if !test_only(&item.attrs) {
            syn::visit::visit_trait_item_fn(self, item);
        }
    }

    fn visit_trait_item_const(&mut self, item: &'ast syn::TraitItemConst) {
        if !test_only(&item.attrs) {
            syn::visit::visit_trait_item_const(self, item);
        }
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if !test_only(&item.attrs) {
            self.import(Vec::new(), &item.tree);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(expression) = call.func.as_ref() {
            let segments = expression
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>();
            if segments.last().is_some_and(|name| name == "new") {
                if segments.len() == 4
                    && segments[1] == "process"
                    && segments[2] == "Command"
                    && matches!(segments[0].as_str(), "std" | "tokio")
                {
                    self.kinds.insert(segments[0].clone());
                } else if segments.len() == 2
                    && let Some(kind) = self.aliases.get(&segments[0])
                {
                    self.kinds.insert(kind.clone());
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

#[test]
fn process_spawn_sites_are_the_recorded_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates");
    let mut parsed = Vec::new();
    let mut excluded = BTreeSet::new();
    for entry in fs::read_dir(root).expect("workspace crates") {
        let source = entry.expect("crate").path().join("src");
        if !source.is_dir() {
            continue;
        }
        for path in files(&source) {
            let file = syn::parse_file(&fs::read_to_string(&path).expect("Rust source"))
                .expect("valid Rust");
            let directory = if matches!(
                path.file_stem().and_then(|stem| stem.to_str()),
                Some("lib" | "main" | "mod")
            ) {
                path.parent().expect("source parent").to_path_buf()
            } else {
                path.with_extension("")
            };
            let mut visitor = TestModules {
                directory,
                excluded: BTreeSet::new(),
            };
            visitor.visit_file(&file);
            excluded.extend(visitor.excluded);
            parsed.push((path, file));
        }
    }
    let mut actual = BTreeSet::new();
    for (path, file) in parsed {
        if test_only(&file.attrs)
            || excluded
                .iter()
                .any(|excluded| path == *excluded || path.starts_with(excluded))
        {
            continue;
        }
        let mut visitor = SpawnSites::default();
        visitor.scan(&file);
        for kind in visitor.kinds {
            actual.insert((
                kind,
                path.strip_prefix(root)
                    .expect("crate source")
                    .to_string_lossy()
                    .replace('\\', "/"),
            ));
        }
    }
    let expected = [
        ("tokio", "atm-daemon-bootstrap/src/process_deadline.rs"),
        (
            "tokio",
            "atm-daemon-bootstrap/src/received_hook_selector.rs",
        ),
        ("tokio", "atm-herdr/src/transport_cli.rs"),
        ("tokio", "atm/src/commands/send_to.rs"),
        ("std", "atm-daemon-client/src/lib.rs"),
        ("std", "atm/src/commands/internal_nudge.rs"),
    ]
    .into_iter()
    .map(|(kind, path)| (kind.to_owned(), path.to_owned()))
    .collect();
    assert_eq!(actual, expected);
}

#[test]
fn aliases_and_qualified_calls_are_found_but_tests_and_unrelated_commands_are_not() {
    let file = syn::parse_file(r#"
        use tokio::{process::Command as AsyncCommand};
        use std::process::Command as ProcessCommand;
        fn production() { AsyncCommand::new("a"); ProcessCommand::new("b"); tokio::process::Command::new("c"); }
        #[cfg(test)] mod tests { fn helper() { std::process::Command::new("test"); } }
    "#).unwrap();
    let mut visitor = SpawnSites::default();
    visitor.scan(&file);
    assert_eq!(
        visitor.kinds,
        BTreeSet::from(["std".into(), "tokio".into()])
    );
    let mut visitor = SpawnSites::default();
    visitor.scan(&syn::parse_file("use cargo_metadata::MetadataCommand; fn run() { MetadataCommand::new(); } #[cfg(test)] fn test() { std::process::Command::new(\"x\"); }").unwrap());
    assert!(visitor.kinds.is_empty());
}

#[test]
fn cfg_test_items_are_excluded_across_supported_item_kinds() {
    let file = syn::parse_file(
        r#"
            use std::process::Command;
            struct Fixture;
            impl Fixture {
                #[cfg(test)]
                fn helper() { Command::new("impl-helper"); }
                #[cfg(all(test, unix))]
                const HELPER: () = { Command::new("impl-const"); };
            }
            #[cfg(all(test, unix))]
            const TEST_COMMAND: () = { Command::new("const"); };
            #[cfg(all(unix, test))]
            static TEST_FACTORY: () = { Command::new("static"); };
            #[cfg(test)]
            trait TestOnlyTrait {
                fn helper() { Command::new("trait-helper"); }
            }
        "#,
    )
    .unwrap();
    let mut visitor = SpawnSites::default();
    visitor.scan(&file);
    assert!(visitor.kinds.is_empty());
}
