use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !root.exists() {
        return files;
    }
    let mut entries = fs::read_dir(root)
        .expect("source directory")
        .map(|entry| entry.expect("source entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}

fn parse(path: &Path) -> syn::File {
    syn::parse_file(&fs::read_to_string(path).expect("Rust source"))
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn test_only(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute
                .parse_args::<syn::Meta>()
                .is_ok_and(|meta| cfg_requires_test(&meta))
    })
}

fn cfg_requires_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("all") => {
            cfg_args(list).iter().any(cfg_requires_test)
        }
        syn::Meta::List(list) if list.path.is_ident("any") => {
            let args = cfg_args(list);
            !args.is_empty() && args.iter().all(cfg_requires_test)
        }
        _ => false,
    }
}

fn cfg_args(list: &syn::MetaList) -> Vec<syn::Meta> {
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .map(|args| args.into_iter().collect())
        .unwrap_or_default()
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::ExternCrate(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::ForeignMod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::TraitAlias(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

struct StoreImports {
    forbidden: BTreeSet<&'static str>,
    violations: Vec<String>,
}

impl StoreImports {
    fn inspect(&mut self, segments: &[String]) {
        if segments
            .first()
            .is_some_and(|segment| segment == "atm_storage")
            && segments
                .iter()
                .skip(1)
                .any(|segment| self.forbidden.contains(segment.as_str()))
        {
            self.violations.push(segments.join("::"));
        }
    }

    fn inspect_use(&mut self, prefix: &[String], tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(path) => {
                let mut segments = prefix.to_vec();
                segments.push(path.ident.to_string());
                self.inspect_use(&segments, &path.tree);
            }
            syn::UseTree::Name(name) => {
                let mut segments = prefix.to_vec();
                segments.push(name.ident.to_string());
                self.inspect(&segments);
            }
            syn::UseTree::Rename(rename) => {
                let mut segments = prefix.to_vec();
                segments.push(rename.ident.to_string());
                self.inspect(&segments);
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.inspect_use(prefix, item);
                }
            }
            syn::UseTree::Glob(_) => {
                if prefix
                    .first()
                    .is_some_and(|segment| segment == "atm_storage")
                {
                    self.violations.push(format!("{}::*", prefix.join("::")));
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for StoreImports {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !test_only(item_attrs(item)) {
            syn::visit::visit_item(self, item);
        }
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.inspect_use(&[], &item.tree);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.inspect(
            &path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
        );
        syn::visit::visit_path(self, path);
    }
}

// Limitation: paths inside macro token streams and renamed crate aliases (e.g. extern crate atm_storage as s) are not inspected.
#[test]
fn bk_hook_and_cron_types_reach_runtime_crates_only_through_atm_core_boundary() {
    let names = [
        "HookRegistrationStore",
        "HookRegistration",
        "HookName",
        "PhaseId",
        "CronScheduleStore",
        "CronSchedule",
        "NewCronSchedule",
        "CronScheduleId",
        "StopCondition",
        "HookExitCause",
        "HOOK_TIMEOUT_MAX_MS",
        "CRON_TIMEOUT_MAX_MS",
        "HOOK_ACTION_RESERVE_MS",
        "SERVER_REQUEST_BUDGET",
    ];
    for package in [
        "atm-http-runtime",
        "atm-daemon-bootstrap",
        "atm",
        "atm-cron",
    ] {
        for path in rust_files(&workspace_root().join("crates").join(package).join("src")) {
            let syntax = parse(&path);
            if test_only(&syntax.attrs) {
                continue;
            }
            let mut visitor = StoreImports {
                forbidden: names.into(),
                violations: Vec::new(),
            };
            visitor.visit_file(&syntax);
            assert!(
                visitor.violations.is_empty(),
                "{} imports hook/cron contracts directly: {:?}",
                path.display(),
                visitor.violations
            );
        }
    }
}

struct Implementations<'a> {
    name: &'a str,
    krate: &'a str,
    allowed: &'a [(&'a str, &'a str)],
    violations: Vec<String>,
}

impl<'ast> Visit<'ast> for Implementations<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !test_only(item_attrs(item)) {
            syn::visit::visit_item(self, item);
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if let Some((_, path, _)) = &item.trait_
            && path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == self.name)
        {
            let implementation = match item.self_ty.as_ref() {
                syn::Type::Path(path) => path
                    .path
                    .segments
                    .last()
                    .expect("implementer")
                    .ident
                    .to_string(),
                _ => "<non-path implementer>".to_owned(),
            };
            if !self
                .allowed
                .contains(&(self.krate, implementation.as_str()))
            {
                self.violations
                    .push(format!("{}::{}", self.krate, implementation));
            }
        }
        syn::visit::visit_item_impl(self, item);
    }
}

fn assert_known_implementations(name: &str, allowed: &[(&str, &str)]) {
    let crates = workspace_root().join("crates");
    let mut packages = fs::read_dir(crates)
        .expect("workspace crates")
        .map(|entry| entry.expect("workspace crate").path())
        .collect::<Vec<_>>();
    packages.sort();
    for package in packages {
        let krate = package
            .file_name()
            .and_then(|name| name.to_str())
            .expect("crate dir name");
        for path in rust_files(&package.join("src")) {
            let syntax = parse(&path);
            if test_only(&syntax.attrs) {
                continue;
            }
            let mut visitor = Implementations {
                name,
                krate,
                allowed,
                violations: Vec::new(),
            };
            visitor.visit_file(&syntax);
            assert!(
                visitor.violations.is_empty(),
                "{} has unauthorized {name} implementations: {:?}",
                path.display(),
                visitor.violations
            );
        }
    }
}

#[test]
fn bk_task_commands_has_only_known_implementations() {
    assert_known_implementations(
        "TaskCommands",
        &[
            ("atm-http-runtime", "NormalTaskCommands"),
            ("atm-http-runtime", "HookedTaskCommands"),
            ("atm-core", "RecordingTaskCommands"),
        ],
    );
}

#[test]
fn bk_hook_runner_has_only_known_implementations() {
    assert_known_implementations(
        "HookRunner",
        &[
            ("atm-daemon-bootstrap", "DaemonHookRunner"),
            ("atm-core", "DummyHookRunner"),
        ],
    );
}

#[test]
fn bk_cron_scheduler_has_only_known_implementations() {
    assert_known_implementations(
        "CronScheduler",
        &[
            ("atm-cron", "AtmCron"),
            ("atm-core", "RecordingCronScheduler"),
        ],
    );
}

#[test]
fn bk_hook_action_dispatcher_has_only_known_implementations() {
    assert_known_implementations(
        "HookActionDispatcher",
        &[
            ("atm-http-runtime", "UnhookedRouter"),
            ("atm-core", "RecordingHookActionDispatcher"),
        ],
    );
}

#[test]
fn bk_cfg_requires_test_matches_only_test_gated_predicates() {
    for item in [
        syn::parse_quote!(
            #[cfg(test)]
            struct Probe;
        ),
        syn::parse_quote!(
            #[cfg(all(test, unix))]
            struct Probe;
        ),
        syn::parse_quote!(
            #[cfg(any(test, all(test, unix)))]
            struct Probe;
        ),
    ] {
        assert!(test_only(item_attrs(&item)));
    }
    for item in [
        syn::parse_quote!(
            #[cfg(any(test, feature = "test-utils"))]
            struct Probe;
        ),
        syn::parse_quote!(
            #[cfg(not(test))]
            struct Probe;
        ),
        syn::parse_quote!(
            #[cfg(unix)]
            struct Probe;
        ),
    ] {
        assert!(!test_only(item_attrs(&item)));
    }
}

#[test]
fn bk_implementer_allowlist_is_bound_to_crate() {
    let trait_name: syn::Ident = syn::parse_quote!(TaskCommands);
    let normal: syn::Ident = syn::parse_quote!(NormalTaskCommands);
    let fake: syn::Ident = syn::parse_quote!(Fake);
    let syntax: syn::File = syn::parse_quote! {
        impl #trait_name for #normal {}
        #[cfg(all(test, unix))]
        impl #trait_name for #fake {}
    };
    let allowed = &[("atm-http-runtime", "NormalTaskCommands")];
    let mut wrong_crate = Implementations {
        name: "TaskCommands",
        krate: "atm-core",
        allowed,
        violations: Vec::new(),
    };
    wrong_crate.visit_file(&syntax);
    assert_eq!(wrong_crate.violations, ["atm-core::NormalTaskCommands"]);

    let mut allowed_crate = Implementations {
        name: "TaskCommands",
        krate: "atm-http-runtime",
        allowed,
        violations: Vec::new(),
    };
    allowed_crate.visit_file(&syntax);
    assert!(allowed_crate.violations.is_empty());
}
