use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use proc_macro2::Span;
use syn::Attribute;
use syn::Block;
use syn::Expr;
use syn::ExprMethodCall;
use syn::File;
use syn::ImplItem;
use syn::Item;
use syn::ItemFn;
use syn::ItemImpl;
use syn::ItemMod;
use syn::Pat;
use syn::Stmt;
use syn::spanned::Spanned;

use crate::Finding;
use crate::NodeId;
use crate::OwnerId;
use crate::RuleId;

#[derive(Debug, Clone)]
struct FileContext {
    source_path: PathBuf,
    package: String,
    target: String,
    is_test_file: bool,
}

#[derive(Debug, Clone)]
struct RuntimeFinding {
    rule_id: RuleId,
    kind: &'static str,
    message: String,
    source_path: PathBuf,
    line: usize,
    package: String,
    target: String,
    node_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Test,
    NonTest,
}

pub(crate) fn analyze_runtime_liveness(root: &Path) -> Result<Vec<Finding>> {
    let source_files = discover_source_files(root)?;
    let mut findings = Vec::new();

    for file_context in source_files {
        let parsed = syn::parse_file(&fs::read_to_string(&file_context.source_path).with_context(
            || {
                format!(
                    "failed to read Rust source for runtime liveness analysis: {}",
                    file_context.source_path.display()
                )
            },
        )?)
        .with_context(|| {
            format!(
                "failed to parse Rust source for runtime liveness analysis: {}",
                file_context.source_path.display()
            )
        })?;

        let mut collector = RuntimeCollector::new(&file_context);
        collector.visit_file_items(&parsed);
        findings.extend(collector.findings);
    }

    findings.sort_by(|left, right| {
        left.rule_id
            .as_str()
            .cmp(right.rule_id.as_str())
            .then_with(|| left.source_path.cmp(&right.source_path))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.message.cmp(&right.message))
    });

    Ok(findings
        .into_iter()
        .map(|finding| Finding {
            rule_id: finding.rule_id,
            kind: finding.kind.to_string(),
            message: format!(
                "{}:{}: {}",
                finding.source_path.display(),
                finding.line,
                finding.message
            ),
            owner_ids: vec![OwnerId::new(format!(
                "crate::{}::{}",
                finding.package, finding.target
            ))],
            node_ids: vec![NodeId::new(finding.node_label)],
        })
        .collect())
}

struct RuntimeCollector<'a> {
    file_context: &'a FileContext,
    findings: Vec<RuntimeFinding>,
}

impl<'a> RuntimeCollector<'a> {
    fn new(file_context: &'a FileContext) -> Self {
        Self {
            file_context,
            findings: Vec::new(),
        }
    }

    fn visit_file_items(&mut self, file: &File) {
        let initial_scope = if self.file_context.is_test_file {
            ScopeKind::Test
        } else {
            ScopeKind::NonTest
        };
        self.visit_items(&file.items, initial_scope);
    }

    fn visit_items(&mut self, items: &[Item], inherited_scope: ScopeKind) {
        for item in items {
            self.visit_item(item, inherited_scope);
        }
    }

    fn visit_item(&mut self, item: &Item, inherited_scope: ScopeKind) {
        match item {
            Item::Fn(item_fn) => self.visit_item_fn(item_fn, inherited_scope),
            Item::Mod(item_mod) => self.visit_item_mod(item_mod, inherited_scope),
            Item::Impl(item_impl) => self.visit_item_impl(item_impl, inherited_scope),
            _ => {}
        }
    }

    fn visit_item_fn(&mut self, item_fn: &ItemFn, inherited_scope: ScopeKind) {
        let scope = item_is_test_scope(
            &item_fn.attrs,
            inherited_scope,
            Some(item_fn.sig.ident == "tests"),
        );
        if scope == ScopeKind::NonTest {
            self.visit_block(&item_fn.block, &item_fn.sig.ident.to_string());
        }
    }

    fn visit_item_mod(&mut self, item_mod: &ItemMod, inherited_scope: ScopeKind) {
        let scope = item_is_test_scope(
            &item_mod.attrs,
            inherited_scope,
            Some(item_mod.ident == "tests"),
        );
        if let Some((_, items)) = &item_mod.content {
            self.visit_items(items, scope);
        }
    }

    fn visit_item_impl(&mut self, item_impl: &ItemImpl, inherited_scope: ScopeKind) {
        let scope = item_is_test_scope(&item_impl.attrs, inherited_scope, None);
        if scope != ScopeKind::NonTest {
            return;
        }
        for item in &item_impl.items {
            if let ImplItem::Fn(item_fn) = item {
                let fn_scope = item_is_test_scope(&item_fn.attrs, scope, None);
                if fn_scope == ScopeKind::NonTest {
                    self.visit_block(&item_fn.block, &item_fn.sig.ident.to_string());
                }
            }
        }
    }

    fn visit_block(&mut self, block: &Block, label: &str) {
        for stmt in &block.stmts {
            self.visit_stmt(stmt, label);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt, label: &str) {
        match stmt {
            Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    if matches!(&local.pat, Pat::Wild(_)) {
                        self.check_discarded_timeout_wait(&init.expr, label);
                    }
                    self.visit_expr(&init.expr, label);
                }
            }
            Stmt::Expr(expr, Some(_semi)) => {
                self.check_discarded_timeout_wait(expr, label);
                self.visit_expr(expr, label);
            }
            Stmt::Expr(expr, None) => self.visit_expr(expr, label),
            Stmt::Item(item) => self.visit_item(item, ScopeKind::NonTest),
            Stmt::Macro(_) => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expr, label: &str) {
        match expr {
            Expr::MethodCall(method_call) => {
                self.check_bare_wait(method_call, label);
                self.visit_expr(&method_call.receiver, label);
                for arg in &method_call.args {
                    self.visit_expr(arg, label);
                }
            }
            Expr::Block(expr_block) => self.visit_block(&expr_block.block, label),
            Expr::If(expr_if) => {
                self.visit_expr(&expr_if.cond, label);
                self.visit_block(&expr_if.then_branch, label);
                if let Some((_, else_expr)) = &expr_if.else_branch {
                    self.visit_expr(else_expr, label);
                }
            }
            Expr::Match(expr_match) => {
                self.visit_expr(&expr_match.expr, label);
                for arm in &expr_match.arms {
                    self.visit_expr(&arm.body, label);
                }
            }
            Expr::Call(expr_call) => {
                self.visit_expr(&expr_call.func, label);
                for arg in &expr_call.args {
                    self.visit_expr(arg, label);
                }
            }
            Expr::Tuple(expr_tuple) => {
                for elem in &expr_tuple.elems {
                    self.visit_expr(elem, label);
                }
            }
            Expr::Paren(expr_paren) => self.visit_expr(&expr_paren.expr, label),
            Expr::Unary(expr_unary) => self.visit_expr(&expr_unary.expr, label),
            Expr::Reference(expr_reference) => self.visit_expr(&expr_reference.expr, label),
            Expr::Try(expr_try) => self.visit_expr(&expr_try.expr, label),
            _ => {}
        }
    }

    fn check_bare_wait(&mut self, method_call: &ExprMethodCall, label: &str) {
        if method_call.method != "wait" || method_call.args.len() != 1 {
            return;
        }
        self.findings.push(RuntimeFinding {
            rule_id: RuleId::ScbRuntime001,
            kind: "condvar_wait_without_timeout",
            message: "SCB-RUNTIME-001 bare Condvar::wait(...) in non-test production code; use wait_timeout(...) or wait_timeout_while(...) and inspect the WaitTimeoutResult".to_string(),
            source_path: self.file_context.source_path.clone(),
            line: span_start_line(method_call.span()),
            package: self.file_context.package.clone(),
            target: self.file_context.target.clone(),
            node_label: format!(
                "crate::{}::{}::{}",
                self.file_context.package, self.file_context.target, label
            ),
        });
    }

    fn check_discarded_timeout_wait(&mut self, expr: &Expr, label: &str) {
        if !contains_timeout_wait_call(expr) {
            return;
        }
        self.findings.push(RuntimeFinding {
            rule_id: RuleId::ScbRuntime002,
            kind: "discarded_wait_timeout_result",
            message: "SCB-RUNTIME-002 wait_timeout* result discarded in non-test production code; inspect the returned WaitTimeoutResult before proceeding".to_string(),
            source_path: self.file_context.source_path.clone(),
            line: span_start_line(expr.span()),
            package: self.file_context.package.clone(),
            target: self.file_context.target.clone(),
            node_label: format!(
                "crate::{}::{}::{}",
                self.file_context.package, self.file_context.target, label
            ),
        });
    }
}

fn discover_source_files(root: &Path) -> Result<Vec<FileContext>> {
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

fn item_is_test_scope(
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

fn attr_is_cfg_test(attr: &Attribute) -> bool {
    let path = attr.path();
    if !path.is_ident("cfg") {
        return false;
    }
    attr.parse_args::<syn::Ident>()
        .map(|ident| ident == "test")
        .unwrap_or(false)
}

fn attr_is_test(attr: &Attribute) -> bool {
    attr.path().is_ident("test")
}

fn span_start_line(span: Span) -> usize {
    span.start().line
}

fn contains_timeout_wait_call(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall(method_call) => {
            matches!(
                method_call.method.to_string().as_str(),
                "wait_timeout" | "wait_timeout_while"
            ) || contains_timeout_wait_call(&method_call.receiver)
                || method_call.args.iter().any(contains_timeout_wait_call)
        }
        Expr::Try(expr_try) => contains_timeout_wait_call(&expr_try.expr),
        Expr::Paren(expr_paren) => contains_timeout_wait_call(&expr_paren.expr),
        Expr::Reference(expr_reference) => contains_timeout_wait_call(&expr_reference.expr),
        Expr::Unary(expr_unary) => contains_timeout_wait_call(&expr_unary.expr),
        Expr::Call(expr_call) => {
            contains_timeout_wait_call(&expr_call.func)
                || expr_call.args.iter().any(contains_timeout_wait_call)
        }
        _ => false,
    }
}
