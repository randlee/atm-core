use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use proc_macro2::Span;

#[derive(Debug, Clone)]
pub(crate) struct FileContext {
    pub(crate) source_path: PathBuf,
    pub(crate) package: String,
    pub(crate) target: String,
    pub(crate) is_test_file: bool,
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
