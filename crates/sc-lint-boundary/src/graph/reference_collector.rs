use super::*;

#[derive(Default)]
pub(super) struct ReferenceCollector {
    owner_name: Option<String>,
    local_owner_names: BTreeSet<String>,
    references: BTreeSet<CollectedReference>,
}

impl ReferenceCollector {
    fn new(local_owner_names: &BTreeSet<String>, owner_name: Option<&str>) -> Self {
        Self {
            owner_name: owner_name.map(ToOwned::to_owned),
            local_owner_names: local_owner_names.clone(),
            references: BTreeSet::new(),
        }
    }

    fn into_references(self) -> BTreeSet<CollectedReference> {
        self.references
    }

    fn maybe_insert_path(&mut self, path: &syn::Path, kind: ReferenceKind) {
        let mut segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if segments.is_empty() {
            return;
        }

        if segments[0] == "Self" {
            if let Some(owner_name) = &self.owner_name {
                segments[0] = owner_name.clone();
                self.references.insert(CollectedReference {
                    path: segments.join("::"),
                    kind,
                });
            }
            return;
        }

        if matches!(kind, ReferenceKind::Expr) && segments.len() == 1 && segments[0] == "self" {
            return;
        }

        let first = &segments[0];
        // `segments` is non-empty because the early return above rejects empty paths.
        let last = segments.last().unwrap();
        let should_collect = matches!(first.as_str(), "crate" | "self" | "super")
            || self.local_owner_names.contains(first)
            || self.local_owner_names.contains(last);

        if should_collect {
            self.references.insert(CollectedReference {
                path: segments.join("::"),
                kind,
            });
        }
    }
}

impl<'ast> Visit<'ast> for ReferenceCollector {
    fn visit_type_path(&mut self, type_path: &'ast syn::TypePath) {
        self.maybe_insert_path(&type_path.path, ReferenceKind::Type);
        syn::visit::visit_type_path(self, type_path);
    }

    fn visit_expr_path(&mut self, expr_path: &'ast syn::ExprPath) {
        self.maybe_insert_path(&expr_path.path, ReferenceKind::Expr);
        syn::visit::visit_expr_path(self, expr_path);
    }

    fn visit_receiver(&mut self, _receiver: &'ast Receiver) {}
}

pub(super) fn collect_references_with(
    local_owner_names: &BTreeSet<String>,
    owner_name: Option<&str>,
    visit: impl FnOnce(&mut ReferenceCollector),
) -> BTreeSet<CollectedReference> {
    let mut collector = ReferenceCollector::new(local_owner_names, owner_name);
    visit(&mut collector);
    collector.into_references()
}
