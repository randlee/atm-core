use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use proc_macro2::Span;
use syn::Attribute;
use syn::Item;

#[derive(Debug, Clone)]
pub(crate) struct FileContext {
    pub(crate) source_path: PathBuf,
    pub(crate) package: String,
    pub(crate) target: String,
    pub(crate) is_test_file: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeKind {
    Test,
    NonTest,
}

pub(crate) fn discover_source_files(root: &Path) -> Result<Vec<FileContext>> {
    let metadata = crate::graph::load_metadata(root)?;
    let workspace_members = metadata.workspace_members.clone();
    let mut files = Vec::new();
    let mut seen_paths = BTreeSet::new();

    for package in &metadata.packages {
        if !workspace_members.iter().any(|id| id == &package.id) {
            continue;
        }
        for target in &package.targets {
            if !crate::graph::is_supported_target(target) {
                continue;
            }
            let manifest_dir = package
                .manifest_path
                .as_std_path()
                .parent()
                .context("package manifest missing parent")?;
            let src_dir = manifest_dir.join("src");
            let tests_dir = manifest_dir.join("tests");
            collect_rust_files(
                &src_dir,
                false,
                &package.name,
                &target.name,
                &mut seen_paths,
                &mut files,
            )?;
            collect_rust_files(
                &tests_dir,
                true,
                &package.name,
                &target.name,
                &mut seen_paths,
                &mut files,
            )?;
        }
    }

    Ok(files)
}

fn collect_rust_files(
    dir: &Path,
    is_test_file: bool,
    package: &str,
    target: &str,
    seen_paths: &mut BTreeSet<PathBuf>,
    files: &mut Vec<FileContext>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_rust_files(&path, is_test_file, package, target, seen_paths, files)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        if !seen_paths.insert(path.clone()) {
            continue;
        }
        files.push(FileContext {
            source_path: path,
            package: package.to_string(),
            target: target.to_string(),
            is_test_file,
        });
    }
    Ok(())
}

pub(crate) fn span_start_line(span: Span) -> usize {
    span.start().line
}

pub(crate) fn attr_is_cfg_test(attr: &Attribute) -> bool {
    let path = attr.path();
    if !path.is_ident("cfg") {
        return false;
    }
    attr.parse_args::<syn::Ident>()
        .map(|ident| ident == "test")
        .unwrap_or(false)
}

pub(crate) fn attr_is_test(attr: &Attribute) -> bool {
    attr.path().is_ident("test")
}

pub(crate) fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item_const) => &item_const.attrs,
        Item::Enum(item_enum) => &item_enum.attrs,
        Item::ExternCrate(item_extern_crate) => &item_extern_crate.attrs,
        Item::Fn(item_fn) => &item_fn.attrs,
        Item::ForeignMod(item_foreign_mod) => &item_foreign_mod.attrs,
        Item::Impl(item_impl) => &item_impl.attrs,
        Item::Macro(item_macro) => &item_macro.attrs,
        Item::Mod(item_mod) => &item_mod.attrs,
        Item::Static(item_static) => &item_static.attrs,
        Item::Struct(item_struct) => &item_struct.attrs,
        Item::Trait(item_trait) => &item_trait.attrs,
        Item::TraitAlias(item_trait_alias) => &item_trait_alias.attrs,
        Item::Type(item_type) => &item_type.attrs,
        Item::Union(item_union) => &item_union.attrs,
        Item::Use(item_use) => &item_use.attrs,
        _ => &[],
    }
}

pub(crate) fn item_name_hint_is_tests(item: &Item) -> Option<bool> {
    match item {
        Item::Fn(item_fn) => Some(item_fn.sig.ident == "tests"),
        Item::Mod(item_mod) => Some(item_mod.ident == "tests"),
        _ => None,
    }
}

pub(crate) fn classify_scope(
    attrs: &[Attribute],
    inherited_scope: ScopeKind,
    name_hint_is_tests: Option<bool>,
) -> ScopeKind {
    if inherited_scope == ScopeKind::Test {
        return ScopeKind::Test;
    }
    if attrs.iter().any(attr_is_cfg_test) || attrs.iter().any(attr_is_test) {
        return ScopeKind::Test;
    }
    if name_hint_is_tests.unwrap_or(false) {
        return ScopeKind::Test;
    }
    ScopeKind::NonTest
}
