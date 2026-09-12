//! BA.3 escalation ownership guard.

use std::fs;
use std::path::{Path, PathBuf};

use quote::ToTokens;
use syn::visit::Visit;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_sources(directory: &Path) -> String {
    fs::read_dir(directory)
        .expect("source directory readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .map(|path| {
            if path.is_dir() {
                rust_sources(&path)
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                fs::read_to_string(path).expect("Rust source readable")
            } else {
                String::new()
            }
        })
        .collect()
}

fn is_cfg_test(attribute: &syn::Attribute) -> bool {
    attribute.path().is_ident("cfg")
        && attribute
            .meta
            .to_token_stream()
            .to_string()
            .contains("test")
}

#[derive(Default)]
struct RuntimeStateVisitor {
    function: Option<String>,
    in_test: bool,
    violations: Vec<String>,
}

impl RuntimeStateVisitor {
    fn permitted(&self) -> bool {
        matches!(
            self.function.as_deref(),
            Some("dispose" | "runtime_state" | "still_idle" | "observe" | "queue_drain_eligible")
        )
    }

    fn check_path(&mut self, path: &syn::Path) {
        let state = path
            .segments
            .iter()
            .any(|segment| segment.ident == "RuntimeMemberState");
        if !self.in_test && !self.permitted() && state {
            self.violations.push(format!(
                "{}: {}",
                self.function.as_deref().unwrap_or("<module>"),
                path.to_token_stream()
            ));
        }
    }
}

impl<'ast> Visit<'ast> for RuntimeStateVisitor {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let prior = self.function.replace(node.sig.ident.to_string());
        syn::visit::visit_item_fn(self, node);
        self.function = prior;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let prior = self.function.replace(node.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, node);
        self.function = prior;
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let prior = self.in_test;
        self.in_test = prior || node.attrs.iter().any(is_cfg_test);
        syn::visit::visit_item_mod(self, node);
        self.in_test = prior;
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        self.check_path(&node.path);
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_pat(&mut self, node: &'ast syn::Pat) {
        if let syn::Pat::Path(path) = node {
            self.check_path(&path.path);
        }
        syn::visit::visit_pat(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if !self.in_test
            && !self.permitted()
            && node.tokens.to_string().contains("RuntimeMemberState")
        {
            self.violations.push(format!(
                "{}: macro body names RuntimeMemberState",
                self.function.as_deref().unwrap_or("<module>")
            ));
        }
        syn::visit::visit_macro(self, node);
    }
}

#[test]
fn escalation_ownership_architecture_test() {
    let root = workspace_root();
    let core = rust_sources(&root.join("crates/atm-core/src"));
    for forbidden in ["escalate", "escalate_mail", "escalation_summary"] {
        assert!(
            !core.contains(forbidden),
            "forbidden core identifier: {forbidden}"
        );
    }

    let runtime = root.join("crates/atm-http-runtime/src");
    let mut visitor = RuntimeStateVisitor::default();
    for entry in fs::read_dir(&runtime)
        .expect("runtime source readable")
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("herdr_") && name.ends_with(".rs"))
        {
            let source = fs::read_to_string(&path).expect("Herdr source readable");
            visitor.visit_file(&syn::parse_file(&source).expect("Herdr source parses"));
        }
    }
    assert!(
        visitor.violations.is_empty(),
        "RuntimeMemberState escaped its ownership boundary: {:?}",
        visitor.violations
    );
    assert_eq!(
        rust_sources(&runtime).matches("PickerMemberStatus").count(),
        0
    );
}
