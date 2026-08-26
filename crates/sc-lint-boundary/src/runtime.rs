use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use sc_lint_attributes::sc_lint;
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
use crate::source_scan::FileContext;
use crate::source_scan::ScopeKind;
use crate::source_scan::classify_scope;
use crate::source_scan::discover_source_files;
use crate::source_scan::span_start_line;

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

// Recursive-descent visitor: visit_items -> visit_item -> visit_item_mod -> visit_items;
// visit_item_impl follows the same recursive descent.
#[sc_lint(boundary.allow("cycle.type_method_self_loop"))]
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
        let scope = classify_scope(
            &item_fn.attrs,
            inherited_scope,
            Some(item_fn.sig.ident == "tests"),
        );
        if scope == ScopeKind::NonTest {
            self.visit_block(&item_fn.block, &item_fn.sig.ident.to_string());
        }
    }

    fn visit_item_mod(&mut self, item_mod: &ItemMod, inherited_scope: ScopeKind) {
        let scope = classify_scope(
            &item_mod.attrs,
            inherited_scope,
            Some(item_mod.ident == "tests"),
        );
        if let Some((_, items)) = &item_mod.content {
            self.visit_items(items, scope);
        }
    }

    fn visit_item_impl(&mut self, item_impl: &ItemImpl, inherited_scope: ScopeKind) {
        let scope = classify_scope(&item_impl.attrs, inherited_scope, None);
        if scope != ScopeKind::NonTest {
            return;
        }
        for item in &item_impl.items {
            if let ImplItem::Fn(item_fn) = item {
                let fn_scope = classify_scope(&item_fn.attrs, scope, None);
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
                    if discarded_timeout_wait_pattern(&local.pat) {
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

fn discarded_timeout_wait_pattern(pat: &Pat) -> bool {
    match pat {
        Pat::Wild(_) => true,
        Pat::Ident(pat_ident) => pat_ident.ident.to_string().starts_with('_'),
        Pat::Tuple(pat_tuple) => pat_tuple
            .elems
            .iter()
            .nth(1)
            .is_some_and(discarded_timeout_wait_pattern),
        _ => false,
    }
}
