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
                .is_ok_and(|meta| matches!(meta, syn::Meta::Path(path) if path.is_ident("test")))
    })
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
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !test_only(&module.attrs) {
            syn::visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if !test_only(&item.attrs) {
            self.inspect_use(&[], &item.tree);
        }
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        if !test_only(&function.attrs) {
            syn::visit::visit_item_fn(self, function);
        }
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
    allowed: &'a [&'a str],
    violations: Vec<String>,
}

impl<'ast> Visit<'ast> for Implementations<'_> {
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
            if !self.allowed.contains(&implementation.as_str()) {
                self.violations.push(implementation);
            }
        }
        syn::visit::visit_item_impl(self, item);
    }
}

fn assert_known_implementations(name: &str, allowed: &[&str]) {
    let crates = workspace_root().join("crates");
    let mut packages = fs::read_dir(crates)
        .expect("workspace crates")
        .map(|entry| entry.expect("workspace crate").path())
        .collect::<Vec<_>>();
    packages.sort();
    for package in packages {
        for path in rust_files(&package.join("src")) {
            let mut visitor = Implementations {
                name,
                allowed,
                violations: Vec::new(),
            };
            visitor.visit_file(&parse(&path));
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
            "NormalTaskCommands",
            "HookedTaskCommands",
            "RecordingTaskCommands",
        ],
    );
}

#[test]
fn bk_hook_runner_has_only_known_implementations() {
    assert_known_implementations("HookRunner", &["DaemonHookRunner", "DummyHookRunner"]);
}

#[test]
fn bk_cron_scheduler_has_only_known_implementations() {
    assert_known_implementations("CronScheduler", &["AtmCron", "RecordingCronScheduler"]);
}

#[test]
fn bk_hook_action_dispatcher_has_only_known_implementations() {
    assert_known_implementations(
        "HookActionDispatcher",
        &["UnhookedRouter", "RecordingHookActionDispatcher"],
    );
}
